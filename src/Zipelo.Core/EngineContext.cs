using System.Security;

namespace Zipelo.Core;

public sealed class EngineContext
{
    private readonly Guid jobId;
    private readonly ArchiveAction action;
    private readonly IOutputTransaction output;
    private readonly PasswordProvider passwords;
    private readonly Action<JobState> reportState;
    private readonly CancellationTokenSource cancellation;
    private readonly int maxPasswordAttempts;
    private readonly SemaphoreSlim serial = new(1, 1);
    private readonly SemaphoreSlim passwordSerial = new(1, 1);
    private readonly object passwordStateGate = new();
    private bool waitingForPassword;
    private readonly HashSet<string> entryIds = new(StringComparer.Ordinal);
    private readonly HashSet<string> paths = new(OutputPathPolicy.Comparer(OutputPathPolicy.CurrentPolicy));
    private readonly List<EntryResult> results = [];
    private int passwordAttempts;
    private ArchiveError failure;

    internal EngineContext(Guid jobId, ArchiveAction action, JobBudget budget, IOutputTransaction output,
        PasswordProvider passwords, Action<JobState> reportState, CancellationTokenSource cancellation, int maxPasswordAttempts)
    {
        this.jobId = jobId;
        this.action = action;
        Budget = budget;
        this.output = output;
        this.passwords = passwords;
        this.reportState = reportState;
        this.cancellation = cancellation;
        this.maxPasswordAttempts = maxPasswordAttempts;
    }

    public JobBudget Budget { get; }
    public CancellationToken CancellationToken => cancellation.Token;
    internal IReadOnlyList<EntryResult> Results => results.AsReadOnly();
    internal bool HasOutput => paths.Count != 0;
    internal ArchiveError Failure => failure;

    public async ValueTask ListEntryAsync(ArchiveEntry entry)
    {
        await serial.WaitAsync(CancellationToken).ConfigureAwait(false);
        try
        {
            Register(entry);
            results.Add(new(entry, EntryOutcome.Listed, VerificationState.NotChecked, ArchiveError.None));
            ReportRunning();
        }
        catch (ArchiveException error)
        {
            failure = error.Code;
            throw;
        }
        finally { serial.Release(); }
    }

    // 해제된 바이트를 직접 복사하여 헤더 크기와 무관하게 예산을 적용함.
    // 검증 콜백은 스트림 끝에서 CRC 또는 인증 결과를 반환해야 함.
    public async Task WriteEntryAsync(ArchiveEntry entry, Stream decoded,
        Func<CancellationToken, ValueTask<VerificationState>> verify)
    {
        ArgumentNullException.ThrowIfNull(decoded);
        ArgumentNullException.ThrowIfNull(verify);
        await serial.WaitAsync(CancellationToken).ConfigureAwait(false);
        try
        {
            if (action is not (ArchiveAction.Extract or ArchiveAction.Create or ArchiveAction.Recover))
            {
                throw new ArchiveException(ArchiveError.UnsupportedOperation);
            }

            Register(entry);
            if (entry.Kind != EntryKind.File)
            {
                throw new ArchiveException(ArchiveError.UnsupportedEntry);
            }

            var segments = OutputPathPolicy.Validate(entry.Name, OutputPathPolicy.CurrentPolicy);
            var key = string.Join('/', segments);
            if (paths.Any(existing => existing.Equals(key, OutputPathPolicy.Comparison(OutputPathPolicy.CurrentPolicy)) ||
                existing.StartsWith(key + "/", OutputPathPolicy.Comparison(OutputPathPolicy.CurrentPolicy)) ||
                key.StartsWith(existing + "/", OutputPathPolicy.Comparison(OutputPathPolicy.CurrentPolicy))))
            {
                throw new ArchiveException(ArchiveError.OutputConflict);
            }

            paths.Add(key);
            await using (var destination = await output.CreateFileAsync(segments, CancellationToken).ConfigureAwait(false))
            {
                var buffer = new byte[65536];
                try
                {
                    while (true)
                    {
                        Budget.Check(CancellationToken);
                        var count = await decoded.ReadAsync(buffer.AsMemory(), CancellationToken).ConfigureAwait(false);
                        if (count == 0) { break; }
                        Budget.ChargeProcessed(count, CancellationToken);
                        Budget.ReserveOutput(count, CancellationToken);
                        await destination.WriteAsync(buffer.AsMemory(0, count), CancellationToken).ConfigureAwait(false);
                        ReportRunning();
                    }
                    await destination.FlushAsync(CancellationToken).ConfigureAwait(false);
                }
                finally { System.Security.Cryptography.CryptographicOperations.ZeroMemory(buffer); }
            }

            var verification = await verify(CancellationToken).ConfigureAwait(false);
            Budget.Check(CancellationToken);
            if (verification == VerificationState.Failed)
            {
                results.Add(new(entry, EntryOutcome.Failed, verification, ArchiveError.IntegrityFailure));
                throw new ArchiveException(ArchiveError.IntegrityFailure);
            }

            results.Add(new(entry, EntryOutcome.Staged, verification, ArchiveError.None));
        }
        catch (ArchiveException error)
        {
            failure = error.Code;
            if (entryIds.Contains(entry.Id) && results.All(result => result.EntryId != entry.Id))
            {
                results.Add(new(entry, EntryOutcome.Failed, VerificationState.NotChecked, error.Code));
            }
            throw;
        }
        catch (Exception error)
        {
            failure = error is OperationCanceledException && CancellationToken.IsCancellationRequested
                ? ArchiveError.Cancelled : ArchiveError.EngineFailure;
            if (entry is not null && entryIds.Contains(entry.Id) && results.All(result => result.EntryId != entry.Id))
            {
                results.Add(new(entry, EntryOutcome.Failed, VerificationState.NotChecked, failure));
            }
            throw;
        }
        finally { serial.Release(); }
    }

    // 제공자 호출을 직렬화함. 소비자는 잠금 밖에서 암호를 사용하고 재요청할 수 있음.
    public async ValueTask<T> UsePasswordAsync<T>(Func<SecureString, CancellationToken, ValueTask<T>> consume)
    {
        ArgumentNullException.ThrowIfNull(consume);
        await passwordSerial.WaitAsync(CancellationToken).ConfigureAwait(false);
        SecureString? password = null;
        var waiting = false;
        try
        {
            try
            {
                Budget.Check(CancellationToken);
                if (passwordAttempts >= maxPasswordAttempts)
                {
                    failure = ArchiveError.PasswordAttemptsExceeded;
                    throw new ArchiveException(ArchiveError.PasswordAttemptsExceeded);
                }

                var attempt = ++passwordAttempts;
                waiting = true;
                SetPasswordWaiting(true);
                password = await passwords(new(jobId, Guid.NewGuid(), attempt), CancellationToken).ConfigureAwait(false);
                if (password is null)
                {
                    cancellation.Cancel();
                    throw new OperationCanceledException(CancellationToken);
                }
                Budget.Check(CancellationToken);
            }
            finally
            {
                if (waiting) { SetPasswordWaiting(false); }
                passwordSerial.Release();
            }

            Budget.Check(CancellationToken);
            return await consume(password, CancellationToken).ConfigureAwait(false);
        }
        finally { password?.Dispose(); }
    }

    private void SetPasswordWaiting(bool waiting)
    {
        lock (passwordStateGate)
        {
            waitingForPassword = waiting;
            reportState(CancellationToken.IsCancellationRequested ? JobState.Cancelling :
                waiting ? JobState.WaitingForPassword : JobState.Running);
        }
    }

    private void ReportRunning()
    {
        lock (passwordStateGate)
        {
            // 다른 항목의 진행 보고가 활성 암호 요청 상태를 덮지 않도록 함.
            if (!waitingForPassword)
            {
                reportState(CancellationToken.IsCancellationRequested ? JobState.Cancelling : JobState.Running);
            }
        }
    }

    private void Register(ArchiveEntry entry)
    {
        ArgumentNullException.ThrowIfNull(entry);
        Budget.AddEntry(CancellationToken);
        if (string.IsNullOrWhiteSpace(entry.Id) || !entryIds.Add(entry.Id) || entry.DeclaredSize < 0)
        {
            throw new ArchiveException(ArchiveError.UnsupportedEntry);
        }
    }
}

namespace Zipelo.Core;

public sealed class ArchiveJob
{
    private readonly Guid id;
    private readonly JobLimits limits;
    private int started;

    public ArchiveJob(Guid id, JobLimits limits)
    {
        if (id == Guid.Empty) { throw new ArgumentException("작업 ID가 필요함", nameof(id)); }
        this.id = id;
        this.limits = limits;
    }

    // 입력 수명은 호출자가 소유함. 출력 트랜잭션은 작업이 소유하여 종료 전에 정리함.
    public async Task<JobResult> ExecuteAsync(ArchiveAction action, string format, IArchiveEngine engine,
        IReadOnlyList<Stream> readOnlyInputs, IOutputTransaction output, PasswordProvider passwords,
        IProgress<JobEvent>? progress, CancellationToken cancellationToken)
    {
        ArgumentNullException.ThrowIfNull(engine);
        ArgumentNullException.ThrowIfNull(readOnlyInputs);
        ArgumentNullException.ThrowIfNull(output);
        ArgumentNullException.ThrowIfNull(passwords);
        if (Interlocked.Exchange(ref started, 1) != 0)
        {
            throw new InvalidOperationException("작업 인스턴스는 재사용할 수 없음");
        }

        JobBudget? budget = null;
        EngineContext? context = null;
        CancellationTokenSource? linked = null;
        CancellationTokenRegistration cancellationRegistration = default;
        var eventGate = new object();
        long sequence = 0;
        void Report(JobState reportedState)
        {
            lock (eventGate)
            {
                if ((reportedState is JobState.Running or JobState.WaitingForPassword) &&
                    (cancellationToken.IsCancellationRequested || linked?.IsCancellationRequested == true))
                {
                    reportedState = JobState.Cancelling;
                }

                // 관찰자 오류가 정리나 실제 작업 결과를 바꾸지 않도록 함.
                try { progress?.Report(new(id, ++sequence, reportedState, budget?.Usage.ProcessedBytes ?? 0)); }
                catch { }
            }
        }

        var state = JobState.Failed;
        var error = ArchiveError.None;
        var committed = false;
        try
        {
            // 출력 소유권을 받은 뒤의 초기화도 정리 범위에 포함함.
            budget = new JobBudget(limits);
            linked = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
            cancellationRegistration = linked.Token.Register(() => Report(JobState.Cancelling));
            var inputs = readOnlyInputs.ToArray();
            context = new EngineContext(id, action, budget, output, passwords, Report, linked, limits.MaxPasswordAttempts);
            budget.Check(linked.Token);
            if (inputs.Length == 0 || inputs.Any(input => input is null || !input.CanRead || input.CanWrite))
            {
                throw new ArchiveException(ArchiveError.UnsupportedOperation);
            }
            if (!engine.Capabilities.Any(capability => capability.Action == action &&
                string.Equals(capability.Format, format, StringComparison.OrdinalIgnoreCase)))
            {
                throw new ArchiveException(engine is UnavailableEngine ? ArchiveError.EngineUnavailable : ArchiveError.UnsupportedOperation);
            }

            Report(JobState.Running);
            await engine.ExecuteAsync(action, Array.AsReadOnly(inputs), context, linked.Token).ConfigureAwait(false);
            budget.Check(linked.Token);
            if (context.Failure != ArchiveError.None)
            {
                throw new ArchiveException(context.Failure);
            }
            if (context.Results.Any(result => result.Error != ArchiveError.None || result.Verification == VerificationState.Failed))
            {
                throw new ArchiveException(ArchiveError.IntegrityFailure);
            }

            if (context.HasOutput)
            {
                await output.CommitAsync(linked.Token).ConfigureAwait(false);
                committed = true;
            }
            // 게시 성공 뒤의 늦은 취소로 실제 출력을 취소 결과로 바꾸지 않음.
            state = JobState.Completed;
        }
        catch (OperationCanceledException) when (linked?.IsCancellationRequested == true || cancellationToken.IsCancellationRequested)
        {
            state = JobState.Cancelled;
            error = ArchiveError.Cancelled;
        }
        catch (ArgumentException) when (context is null) { error = ArchiveError.UnsupportedOperation; }
        catch (ArchiveException failure) { error = failure.Code; }
        catch { error = ArchiveError.EngineFailure; }
        finally
        {
            try { await output.DisposeAsync().ConfigureAwait(false); }
            catch
            {
                state = JobState.Failed;
                if (error == ArchiveError.None) { error = ArchiveError.OutputUnavailable; }
            }
            finally
            {
                // 종료 이벤트 뒤에 취소 이벤트가 나오지 않도록 등록을 먼저 해제함.
                cancellationRegistration.Dispose();
                linked?.Dispose();
            }
        }

        var results = (context?.Results ?? Array.Empty<EntryResult>()).Select(result => result.Outcome == EntryOutcome.Staged
            ? result with { Outcome = committed ? EntryOutcome.Extracted : EntryOutcome.Failed,
                Error = committed ? ArchiveError.None : error }
            : result).ToArray();
        Report(state);
        return new(id, state, Array.AsReadOnly(results), error);
    }
}

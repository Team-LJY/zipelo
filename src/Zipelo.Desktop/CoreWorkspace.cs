using System.Security;
using Zipelo.Core;

namespace Zipelo.Desktop;

// 호스트는 별도 작업 프로세스를 사용하고 입력과 출력 정리를 마친 뒤 반환해야 함.
public delegate Task<JobResult> CoreJobRunner(
    WorkspaceRequest request,
    IProgress<JobEvent> progress,
    PasswordProvider passwords,
    CancellationToken cancellationToken);

public sealed class CoreWorkspace : IArchiveWorkspace, IWorkspaceCapabilities
{
    private readonly CoreJobRunner run;
    private readonly string? availabilityMessage;
    private readonly IReadOnlyList<EngineCapability> capabilities;

    public CoreWorkspace(CoreJobRunner run, IEnumerable<EngineCapability> capabilities, string? availabilityMessage = null)
    {
        ArgumentNullException.ThrowIfNull(run);
        ArgumentNullException.ThrowIfNull(capabilities);
        this.run = run;
        this.availabilityMessage = availabilityMessage;
        this.capabilities = Array.AsReadOnly(capabilities.ToArray());
    }

    public bool IsAvailable => capabilities.Any(capability =>
        capability.Action is ArchiveAction.List or ArchiveAction.Create);

    public string AvailabilityMessage => availabilityMessage ?? (IsAvailable
        ? "연결된 형식과 기능만 사용할 수 있습니다. 압축 풀기는 안전 검증 전까지 준비 중입니다."
        : "작업 프로세스가 아직 연결되지 않았습니다. 파일을 처리할 수 없습니다.");

    public bool CanExecute(WorkspaceAction action, string format)
    {
        // 안전한 출력 계층의 Windows 검증 전까지 해제를 닫아 둠.
        return action != WorkspaceAction.Extract && capabilities.Any(capability =>
            capability.Action == MapAction(action) &&
            string.Equals(capability.Format, format, StringComparison.OrdinalIgnoreCase));
    }

    public async Task<WorkspaceResult> ExecuteAsync(
        WorkspaceRequest request,
        IProgress<WorkspaceProgress> progress,
        Func<PasswordChallenge, CancellationToken, Task<SecureString?>> requestPassword,
        CancellationToken cancellationToken)
    {
        ArgumentNullException.ThrowIfNull(request);
        ArgumentNullException.ThrowIfNull(progress);
        ArgumentNullException.ThrowIfNull(requestPassword);
        if (request.JobId == Guid.Empty || request.Sources.Count == 0)
        {
            throw new ArchiveException(ArchiveError.UnsupportedOperation);
        }
        if (!CanExecute(request.Action, request.Format))
        {
            return new(WorkspaceOutcome.Failed, Array.Empty<FileRow>())
            {
                JobId = request.JobId,
                Error = IsAvailable ? ArchiveError.UnsupportedOperation : ArchiveError.EngineUnavailable
            };
        }

        var gate = new object();
        long lastSequence = 0;
        var finished = false;
        var events = new ForwardProgress<JobEvent>(value =>
        {
            lock (gate)
            {
                if (finished || value.JobId != request.JobId || value.Sequence <= lastSequence)
                {
                    return;
                }
                lastSequence = value.Sequence;
                progress.Report(new(value.ProcessedBytes, null)
                {
                    JobId = value.JobId,
                    Sequence = value.Sequence,
                    State = value.State
                });
            }
        });

        async ValueTask<SecureString?> Passwords(Zipelo.Core.PasswordChallenge challenge, CancellationToken token)
        {
            if (challenge.JobId != request.JobId || challenge.RequestId == Guid.Empty)
            {
                throw new ArchiveException(ArchiveError.InvalidPasswordResponse);
            }
            // 성공한 전달 이후 암호는 Core가 소비와 폐기를 소유함.
            return await requestPassword(new(challenge.JobId, challenge.RequestId)
            {
                Attempt = challenge.Attempt
            }, token).ConfigureAwait(false);
        }

        try
        {
            var result = await run(request, events, Passwords, cancellationToken).ConfigureAwait(false);
            if (result.JobId != request.JobId)
            {
                throw new ArchiveException(ArchiveError.EngineFailure);
            }
            return MapResult(result);
        }
        finally
        {
            lock (gate) { finished = true; }
        }
    }

    private static ArchiveAction MapAction(WorkspaceAction action) => action switch
    {
        WorkspaceAction.List => ArchiveAction.List,
        WorkspaceAction.Create => ArchiveAction.Create,
        WorkspaceAction.Extract => ArchiveAction.Extract,
        _ => throw new ArchiveException(ArchiveError.UnsupportedOperation)
    };

    internal static WorkspaceResult MapResult(JobResult result)
    {
        var terminal = result.State is JobState.Completed or JobState.Partial or JobState.Cancelled or JobState.Failed;
        var outcome = result.State switch
        {
            JobState.Completed => WorkspaceOutcome.Completed,
            JobState.Partial => WorkspaceOutcome.Partial,
            JobState.Cancelled => WorkspaceOutcome.Cancelled,
            _ => WorkspaceOutcome.Failed
        };
        var rows = result.Entries.Select(value => new FileRow(
            value.Entry.Name,
            value.Entry.DeclaredSize is long size ? $"{size:N0}바이트" : "알 수 없음",
            value.Verification switch
            {
                VerificationState.Passed => "검증 통과",
                VerificationState.Failed => "검증 실패",
                VerificationState.Unavailable => "검증 불가",
                _ => "검증 전"
            })
        {
            EntryId = value.Entry.Id,
            Kind = value.Entry.Kind,
            Outcome = value.Outcome,
            Error = value.Error
        }).ToArray();
        if (outcome == WorkspaceOutcome.Completed && result.Error != ArchiveError.None)
        {
            outcome = WorkspaceOutcome.Failed;
        }
        return new(outcome, Array.AsReadOnly(rows))
        {
            JobId = result.JobId,
            Error = terminal ? result.Error : ArchiveError.EngineFailure
        };
    }

    private sealed class ForwardProgress<T>(Action<T> report) : IProgress<T>
    {
        public void Report(T value) => report(value);
    }
}

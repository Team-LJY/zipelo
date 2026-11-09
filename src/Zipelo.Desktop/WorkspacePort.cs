using System.Security;
using Zipelo.Core;

namespace Zipelo.Desktop;

public enum WorkspaceAction
{
    List,
    Extract,
    Create
}

public enum WorkspaceOutcome
{
    Completed,
    Partial,
    Cancelled,
    Failed
}

public sealed record WorkspaceRequest(
    Guid JobId,
    WorkspaceAction Action,
    IReadOnlyList<string> Sources,
    string? Destination = null)
{
    public string Format { get; init; } = "zip";
}

public sealed record FileRow(string Name, string Size, string Verification)
{
    public string EntryId { get; init; } = "";
    public EntryKind Kind { get; init; }
    public EntryOutcome Outcome { get; init; }
    public ArchiveError Error { get; init; }
}

public sealed record WorkspaceProgress(long ProcessedBytes, long? TotalBytes)
{
    public Guid JobId { get; init; }
    public long Sequence { get; init; }
    public JobState State { get; init; } = JobState.Running;
}

public sealed record PasswordChallenge(Guid JobId, Guid RequestId)
{
    public int Attempt { get; init; } = 1;
}

public sealed record WorkspaceResult(
    WorkspaceOutcome Outcome,
    IReadOnlyList<FileRow> Entries)
{
    public Guid JobId { get; init; }
    public ArchiveError Error { get; init; }
    public Zipelo.Ui.CreationResult? Creation { get; init; }
}

// UI 측 연결 경계임. 실제 코어 계약과 연결하는 어댑터는 별도 구현함.
public interface IArchiveWorkspace
{
    bool IsAvailable { get; }

    Task<WorkspaceResult> ExecuteAsync(
        WorkspaceRequest request,
        IProgress<WorkspaceProgress> progress,
        Func<PasswordChallenge, CancellationToken, Task<SecureString?>> requestPassword,
        CancellationToken cancellationToken);
}

public sealed class UnavailableWorkspace : IArchiveWorkspace, IWorkspaceCapabilities
{
    public bool IsAvailable => false;
    public string AvailabilityMessage { get; internal set; } =
        "작업 프로세스가 아직 연결되지 않았습니다. 파일을 처리할 수 없습니다.";
    public bool CanExecute(WorkspaceAction action, string format) => false;

    public Task<WorkspaceResult> ExecuteAsync(
        WorkspaceRequest request,
        IProgress<WorkspaceProgress> progress,
        Func<PasswordChallenge, CancellationToken, Task<SecureString?>> requestPassword,
        CancellationToken cancellationToken)
    {
        throw new NotSupportedException("압축 엔진이 아직 연결되지 않았습니다.");
    }
}

// 기능 조회는 실제 연결된 호스트가 제공함. 해제 검증 전에는 해제 능력을 게시하지 않음.
public interface IWorkspaceCapabilities
{
    bool CanExecute(WorkspaceAction action, string format);
    string AvailabilityMessage { get; }
}

using System.Security;

namespace Zipelo.Core;

public enum ArchiveAction { List, Extract, Create, Verify, Preview, Recover }
public enum JobState { Created, Running, WaitingForPassword, Cancelling, Completed, Partial, Cancelled, Failed }
public enum EntryKind { File, Directory, SymbolicLink, HardLink, Other }
public enum EntryOutcome { Listed, Staged, Extracted, Skipped, Failed }
public enum VerificationState { NotChecked, Passed, Failed, Unavailable }
public enum ArchiveError
{
    None,
    EngineUnavailable,
    UnsupportedOperation,
    UnsafePath,
    UnsupportedEntry,
    OutputUnavailable,
    OutputConflict,
    BudgetExceeded,
    PasswordAttemptsExceeded,
    InvalidPasswordResponse,
    IntegrityFailure,
    Cancelled,
    EngineFailure
}

public sealed class ArchiveException(ArchiveError code) : Exception(code.ToString())
{
    public ArchiveError Code { get; } = code;
}

public sealed record EngineCapability(string Format, ArchiveAction Action);
public sealed record ArchiveEntry(string Id, string Name, EntryKind Kind, long? DeclaredSize);
public sealed record EntryResult(ArchiveEntry Entry, EntryOutcome Outcome, VerificationState Verification, ArchiveError Error)
{
    public string EntryId => Entry.Id;
}
public sealed record JobEvent(Guid JobId, long Sequence, JobState State, long ProcessedBytes);
public sealed record PasswordChallenge(Guid JobId, Guid RequestId, int Attempt);
public sealed record JobResult(Guid JobId, JobState State, IReadOnlyList<EntryResult> Entries, ArchiveError Error);

// 호출자는 반환된 암호의 소유권을 코어에 넘김. 코어가 소비 종료 시 폐기함.
// 제공자는 작업별로 한 번에 하나만 호출됨. 같은 작업의 암호 제공자를 재진입하지 않음.
public delegate ValueTask<SecureString?> PasswordProvider(PasswordChallenge challenge, CancellationToken cancellationToken);

public interface IArchiveEngine
{
    IReadOnlyCollection<EngineCapability> Capabilities { get; }

    // 원본 경로나 출력 경로를 받지 않고 제한된 스트림과 작업 문맥만 사용함.
    // 시작한 문맥 작업을 모두 await하고 실패를 숨기지 않아야 함. 입력 스트림은 폐기하지 않음.
    Task ExecuteAsync(ArchiveAction action, IReadOnlyList<Stream> inputs, EngineContext context, CancellationToken cancellationToken);
}

public sealed class UnavailableEngine : IArchiveEngine
{
    public IReadOnlyCollection<EngineCapability> Capabilities => Array.Empty<EngineCapability>();

    public Task ExecuteAsync(ArchiveAction action, IReadOnlyList<Stream> inputs, EngineContext context, CancellationToken cancellationToken)
        => Task.FromException(new ArchiveException(ArchiveError.EngineUnavailable));
}

// 저장 계층은 링크와 junction, 경로 교체 경쟁, 기존 파일 충돌을 실제 핸들 기준으로 차단해야 함.
// 원본과 기존 파일은 수정하지 않으며 작업 소유 임시 출력만 폐기해야 함.
// CommitAsync는 전체 게시 성공에만 정상 반환함. 실패나 취소 시 이미 게시한 작업 소유 출력도 원복해야 함.
// 게시 성공 뒤에는 늦은 취소로 예외를 던지지 않음. 부분 게시만 가능한 저장 계층은 이 계약에 연결하지 않음.
// DisposeAsync는 성공한 게시를 유지하고 미확정 출력을 정리함. 원복이나 정리 실패를 숨기지 않음.
public interface IOutputTransaction : IAsyncDisposable
{
    ValueTask<Stream> CreateFileAsync(IReadOnlyList<string> relativeSegments, CancellationToken cancellationToken);
    ValueTask CommitAsync(CancellationToken cancellationToken);
}

public sealed class UnavailableOutput : IOutputTransaction
{
    public ValueTask<Stream> CreateFileAsync(IReadOnlyList<string> relativeSegments, CancellationToken cancellationToken)
        => ValueTask.FromException<Stream>(new ArchiveException(ArchiveError.OutputUnavailable));

    public ValueTask CommitAsync(CancellationToken cancellationToken)
        => ValueTask.FromException(new ArchiveException(ArchiveError.OutputUnavailable));

    public ValueTask DisposeAsync() => ValueTask.CompletedTask;
}

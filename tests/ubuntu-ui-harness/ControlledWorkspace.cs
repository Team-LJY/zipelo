using Zipelo.Core;
using Zipelo.Ui;
using Zipelo.Ubuntu;

namespace Zipelo.UbuntuUiHarness;

internal sealed class ListCall(Guid id, CancellationToken token)
{
    private readonly TaskCompletionSource<JobResult> result = new(TaskCreationOptions.RunContinuationsAsynchronously);
    public Guid Id { get; } = id;
    public CancellationToken Token { get; } = token;
    public int CancelCount { get; private set; }
    public TaskCompletionSource Returned { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    public void ObserveCancellation() => CancelCount++;
    public void Complete(JobState state = JobState.Completed, Guid? resultId = null, bool empty = false)
    {
        UiSession.Check(result.TrySetResult(new JobResult(resultId ?? Id, state,
            state == JobState.Completed && !empty ? Rows : Array.Empty<EntryResult>(),
            state == JobState.Cancelled ? ArchiveError.Cancelled : ArchiveError.None)), "result already fixed");
    }
    public void Cancel() => UiSession.Check(result.TrySetCanceled(Token), "result already fixed");
    public void ReleaseForCleanup() => result.TrySetCanceled();
    public async Task<JobResult> RunAsync()
    {
        using var registration = Token.Register(ObserveCancellation);
        try { return await result.Task; }
        finally { Returned.TrySetResult(); }
    }
    private static readonly EntryResult[] Rows =
    [
        new(new ArchiveEntry("a", "첫 파일.txt", EntryKind.File, 1), EntryOutcome.Listed,
            VerificationState.NotChecked, ArchiveError.None),
        new(new ArchiveEntry("b", "긴 한글 파일 이름의 줄바꿈과 선택 보존을 확인하는 두 번째 파일.txt", EntryKind.File, 1),
            EntryOutcome.Listed, VerificationState.NotChecked, ArchiveError.None)
    ];
}

internal sealed class ControlledWorkspace : IUbuntuArchiveWorkspace
{
    public bool CanListZip => true;
    public bool CanCreateZip => false;
    public bool CanExtractZip => false;
    public string AvailabilityMessage => "테스트 전용 제어된 목록 Task입니다. 실제 Worker를 실행하지 않습니다.";
    public List<ListCall> Calls { get; } = [];
    public async Task<JobResult> ListAsync(Guid jobId, Stream input, CancellationToken cancellationToken)
    {
        UiSession.Check(Calls.Count < 1, "workspace call bound exceeded");
        var bytes = new byte[Fixture.Bytes.Length];
        await input.ReadExactlyAsync(bytes, cancellationToken);
        UiSession.Check(bytes.SequenceEqual(Fixture.Bytes), "fixture input differs");
        UiSession.Check(await input.ReadAsync(new byte[1], cancellationToken) == 0, "fixture exceeds bound");
        var call = new ListCall(jobId, cancellationToken);
        Calls.Add(call);
        return await call.RunAsync();
    }
    public Task<CreationResult> CreateAsync(Guid jobId, IReadOnlyList<CreationSource> sources,
        string destination, CancellationToken cancellationToken) => throw new InvalidOperationException("creation forbidden");
    public Task<ExtractionResult> ExtractAsync(Guid jobId, Func<CancellationToken, Task<Stream>> openRead,
        string destination, CancellationToken cancellationToken) => throw new InvalidOperationException("extraction forbidden");
    public async Task DrainAsync()
    {
        foreach (var call in Calls) { call.ReleaseForCleanup(); }
        await Task.WhenAll(Calls.Select(call => call.Returned.Task)).WaitAsync(TimeSpan.FromSeconds(5));
    }
}

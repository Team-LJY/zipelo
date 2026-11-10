using Zipelo.Runtime.Streaming;
using Zipelo.StreamingExtractionTests;

namespace Zipelo.PublicationBarrierTests;

internal sealed class Barrier(Guid job)
{
    private static Barrier? current;
    private readonly TaskCompletionSource release = new(TaskCreationOptions.RunContinuationsAsynchronously);
    private int visits;
    private int failed;
    internal TaskCompletionSource<string> Arrival { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    internal string Nonce { get; } = Guid.NewGuid().ToString("N");
    internal bool Returned { get; private set; }

    internal static Barrier Install(Guid job)
    {
        var value = new Barrier(job);
        Check.That(Interlocked.CompareExchange(ref current, value, null) is null, "장벽 중복 등록");
        return value;
    }

    internal static void Reach(string temporaryName, string leaf, CancellationToken token)
    {
        var barrier = current ?? throw new InvalidOperationException("장벽 등록 없음");
        Check.That(Interlocked.Increment(ref barrier.visits) == 1 && leaf == "result" &&
            !token.IsCancellationRequested, "정확한 게시 전 경계 도달 실패");
        barrier.Record("probe.barrier.arrive");
        barrier.Arrival.SetResult(temporaryName);
        // 작업 토큰을 사용하지 않으며 기한 실패는 취소 성공으로 분류하지 않음.
        barrier.release.Task.WaitAsync(TimeSpan.FromSeconds(15)).GetAwaiter().GetResult();
        Check.That(Volatile.Read(ref barrier.failed) == 0, "장벽 제어 실패");
        barrier.Record("probe.barrier.return", token.IsCancellationRequested ? 1 : 0);
        barrier.Returned = true;
    }

    internal void Record(string stage, long value = 0) =>
        StreamingExtractionDiagnostics.Record(job, stage, value, identity: Nonce);

    internal void Release()
    {
        Record("probe.barrier.release");
        release.TrySetResult();
    }

    internal void FailAndRelease()
    {
        Interlocked.Exchange(ref failed, 1);
        release.TrySetResult();
    }
}

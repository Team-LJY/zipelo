using Zipelo.GzipBoundaryProbe;

// 검증과 정리 이후에만 시험 결과를 확정함. 파일 게시를 구현하지 않음.
internal sealed class PendingValidation
{
    private bool used;
    public int CommitCount { get; private set; }
    public string? CommittedHash { get; private set; }
    public BoundaryResult? CommittedBoundary { get; private set; }

    public void Execute(ReadOnlyMemory<byte> input, ProbeBudget budget, bool selected,
        CancellationToken token = default, int feedSize = ProbeBudget.ChunkSize,
        Action? afterChunk = null, Action? beforeCommit = null, IRawInflater? inflater = null,
        Action? onSinkDispose = null)
    {
        if (used)
        {
            throw new InvalidOperationException("실패 여부와 관계없이 시험 인스턴스를 재사용하지 않음");
        }
        used = true;
        BoundaryResult result;
        string? pendingHash = null;
        using (var sink = new TemporarySink(onSinkDispose))
        {
            result = BoundaryProbe.Read(input, budget, selected, bytes =>
            {
                if (!selected)
                {
                    throw new InvalidOperationException("비선택 출력이 전달됨");
                }
                sink.Append(bytes);
                afterChunk?.Invoke();
            }, token, feedSize, inflater);
            budget.Check(token);
            if (selected)
            {
                sink.Complete(result);
                pendingHash = sink.CommittedHash;
            }
        }
        beforeCommit?.Invoke();
        budget.Check(token);
        CommittedBoundary = result;
        CommittedHash = pendingHash;
        CommitCount = 1;
    }

    // 실제 해시 자원을 정리한 뒤 Dispose 내부에서 시험 오류를 주입함.
    private sealed class TemporarySink(Action? onDispose) : IDisposable
    {
        private readonly HashSink inner = new();
        public string? CommittedHash => inner.CommittedHash;
        public void Append(ReadOnlyMemory<byte> bytes) => inner.Append(bytes);
        public void Complete(BoundaryResult result) => inner.Complete(result);

        public void Dispose()
        {
            inner.Dispose();
            onDispose?.Invoke();
        }
    }

}

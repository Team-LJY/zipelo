using Zipelo.Core;
using Zipelo.Runtime;

namespace Zipelo.Worker;

internal sealed class BoundedMemoryStream(long limit) : MemoryStream
{
    private void CheckWrite(int count)
    {
        if (count < 0 || Position > limit - count) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
    }

    public override void Write(byte[] buffer, int offset, int count)
    {
        CheckWrite(count);
        base.Write(buffer, offset, count);
    }

    public override void Write(ReadOnlySpan<byte> buffer)
    {
        CheckWrite(buffer.Length);
        base.Write(buffer);
    }

    public override void WriteByte(byte value)
    {
        CheckWrite(1);
        base.WriteByte(value);
    }

    public override Task WriteAsync(byte[] buffer, int offset, int count, CancellationToken token)
    {
        token.ThrowIfCancellationRequested();
        Write(buffer, offset, count);
        return Task.CompletedTask;
    }

    public override ValueTask WriteAsync(ReadOnlyMemory<byte> buffer, CancellationToken token = default)
    {
        token.ThrowIfCancellationRequested();
        Write(buffer.Span);
        return ValueTask.CompletedTask;
    }

    public override void SetLength(long value)
    {
        if (value < 0 || value > limit) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
        base.SetLength(value);
    }
}

// 디스크 저장 없이 단일 ZIP 생성물을 메모리에 확정함.
internal sealed class BoundedMemoryOutput : IOutputTransaction
{
    private BoundedMemoryStream? pending;
    public byte[]? CommittedBytes { get; private set; }

    public ValueTask<Stream> CreateFileAsync(IReadOnlyList<string> segments, CancellationToken token)
    {
        token.ThrowIfCancellationRequested();
        if (pending is not null || segments.Count != 1 || segments[0] != "archive.zip")
        {
            throw new ArchiveException(ArchiveError.OutputConflict);
        }
        pending = new BoundedMemoryStream(WorkerProtocol.MaxDataBytes);
        return ValueTask.FromResult<Stream>(pending);
    }

    public ValueTask CommitAsync(CancellationToken token)
    {
        token.ThrowIfCancellationRequested();
        if (pending is null) { throw new ArchiveException(ArchiveError.OutputUnavailable); }
        CommittedBytes = pending.ToArray();
        return ValueTask.CompletedTask;
    }

    public ValueTask DisposeAsync()
    {
        pending?.Dispose();
        pending = null;
        return ValueTask.CompletedTask;
    }
}

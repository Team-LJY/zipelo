namespace Zipelo.StreamProtocolTests;

// 작은 메모리 입력과 취소 가능한 한 개의 장벽만 사용하는 I/O 대역임.
internal sealed class ControlledStream(byte[]? input = null) : Stream
{
    private readonly MemoryStream inner = input is null ? new() : new(input, false);
    public TaskCompletionSource Entered { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    public TaskCompletionSource Release { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    public int ReadCalls { get; private set; }
    public int WriteCalls { get; private set; }
    public int FlushCalls { get; private set; }
    public long BytesRead { get; private set; }
    public int LargestReadRequest { get; private set; }
    public int ReadFragment { get; init; } = int.MaxValue;
    public int BlockRead { get; set; } = -1;
    public int BlockWrite { get; set; } = -1;
    public int BlockFlush { get; set; } = -1;
    public int FailRead { get; set; } = -1;
    public int FailWrite { get; set; } = -1;
    public long ReadCeiling { get; set; } = long.MaxValue;
    public IOException Failure { get; } = new("테스트 I/O 실패");
    public byte[] Written => inner.ToArray();

    private async Task Wait(CancellationToken token)
    {
        Entered.TrySetResult();
        await Release.Task.WaitAsync(token);
    }

    public override async ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken token = default)
    {
        ReadCalls++;
        LargestReadRequest = Math.Max(LargestReadRequest, buffer.Length);
        if (ReadCalls == FailRead) { throw Failure; }
        if (ReadCalls == BlockRead) { await Wait(token); }
        token.ThrowIfCancellationRequested();
        if (buffer.Length != 0 && inner.Position >= ReadCeiling)
        {
            throw new InvalidOperationException("거절해야 할 본문에 접근함");
        }
        var count = await inner.ReadAsync(buffer[..Math.Min(buffer.Length, ReadFragment)], token);
        BytesRead += count;
        return count;
    }

    public override async ValueTask WriteAsync(ReadOnlyMemory<byte> buffer, CancellationToken token = default)
    {
        WriteCalls++;
        if (WriteCalls == FailWrite) { throw Failure; }
        if (WriteCalls == BlockWrite) { await Wait(token); }
        await inner.WriteAsync(buffer, token);
    }

    public override async Task FlushAsync(CancellationToken token)
    {
        FlushCalls++;
        if (FlushCalls == BlockFlush) { await Wait(token); }
        token.ThrowIfCancellationRequested();
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing) { inner.Dispose(); }
        base.Dispose(disposing);
    }

    public override bool CanRead => inner.CanRead;
    public override bool CanWrite => inner.CanWrite;
    public override bool CanSeek => false;
    public override long Length => inner.Length;
    public override long Position { get => inner.Position; set => throw new NotSupportedException(); }
    public override int Read(byte[] buffer, int offset, int count) => throw new NotSupportedException();
    public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
    public override void Flush() => throw new NotSupportedException();
    public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
    public override void SetLength(long value) => throw new NotSupportedException();
}

namespace Zipelo.StreamingExtractionTests;

internal sealed class ObservedSource(byte[] bytes, string mode = "") : Stream
{
    private readonly MemoryStream inner = new(bytes, false);
    private int rewinds;
    private int reads;
    public TaskCompletionSource Entered { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    public int MaximumRead { get; private set; }
    public override bool CanRead => true;
    public override bool CanWrite => false;
    public override bool CanSeek => mode != "nonseek";
    public override long Length => mode == "oversize" ? 16777217 : inner.Length + (mode == "length-change" && reads > 0 ? 1 : 0);
    public override long Position
    {
        get => inner.Position;
        set
        {
            if (value == 0 && inner.Position != 0) { rewinds++; }
            inner.Position = value;
        }
    }
    public override async ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken token = default)
    {
        MaximumRead = Math.Max(MaximumRead, buffer.Length);
        if (mode == "wait")
        {
            Entered.TrySetResult();
            await Task.Delay(Timeout.InfiniteTimeSpan, token);
        }
        var count = await inner.ReadAsync(buffer, token);
        reads++;
        if (mode == "content-change" && rewinds != 0 && count > 0) { buffer.Span[0] ^= 1; }
        return count;
    }
    public override int Read(byte[] buffer, int offset, int count) => throw new InvalidOperationException("동기 원본 읽기");
    public override long Seek(long offset, SeekOrigin origin) => inner.Seek(offset, origin);
    public override void Flush() => throw new NotSupportedException();
    public override void SetLength(long value) => throw new NotSupportedException();
    public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
    protected override void Dispose(bool disposing)
    {
        if (disposing) { inner.Dispose(); }
        base.Dispose(disposing);
    }
}

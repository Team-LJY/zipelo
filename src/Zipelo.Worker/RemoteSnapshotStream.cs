using Zipelo.Core;
using Zipelo.Runtime.Streaming;

namespace Zipelo.Worker;

// 호스트가 소유한 스냅샷에 범위 읽기만 요청함.
internal sealed class RemoteSnapshotStream(
    Stream replies, Stream requests, Guid id, long length, JobBudget budget, CancellationToken token) : Stream
{
    private long position;
    private ulong sequence;
    private long requested;
    public override bool CanRead => true;
    public override bool CanSeek => true;
    public override bool CanWrite => false;
    public override long Length => length;
    public override long Position
    {
        get => position;
        set
        {
            if (value < 0 || value > length) { throw new ArgumentOutOfRangeException(nameof(value)); }
            position = value;
        }
    }
    public override int Read(byte[] buffer, int offset, int count) => Read(buffer.AsSpan(offset, count));
    public override int Read(Span<byte> buffer)
    {
        budget.Check(token);
        var count = (int)Math.Min(Math.Min(buffer.Length, ExtractionWire.ChunkBytes), length - position);
        if (count == 0) { return 0; }
        if (sequence >= ExtractionWire.MaxRangeRequests || count > ExtractionWire.MaxRangeBytes - requested)
        {
            throw new ArchiveException(ArchiveError.BudgetExceeded);
        }
        budget.ChargeProcessed(count, token);
        var header = ExtractionWire.RangeHeader(id, sequence, position, count);
        requests.Write(header);
        requests.Flush();
        var echoed = new byte[ExtractionWire.RangeHeaderBytes];
        replies.ReadExactly(echoed);
        if (!echoed.AsSpan().SequenceEqual(header)) { throw new ArchiveException(ArchiveError.EngineFailure); }
        replies.ReadExactly(buffer[..count]);
        budget.Check(token);
        sequence++;
        requested += count;
        position += count;
        return count;
    }
    public override long Seek(long offset, SeekOrigin origin)
    {
        Position = checked((origin switch
        {
            SeekOrigin.Begin => 0,
            SeekOrigin.Current => position,
            SeekOrigin.End => length,
            _ => throw new ArgumentOutOfRangeException(nameof(origin))
        }) + offset);
        return position;
    }
    public override void Flush() { }
    public override void SetLength(long value) => throw new NotSupportedException();
    public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
}

internal sealed class PackedEntryStream(Stream source, long start, long length) : Stream
{
    private long position;
    public override bool CanRead => true;
    public override bool CanSeek => false;
    public override bool CanWrite => false;
    public override long Length => length;
    public override long Position { get => position; set => throw new NotSupportedException(); }
    public override int Read(byte[] buffer, int offset, int count) => Read(buffer.AsSpan(offset, count));
    public override int Read(Span<byte> buffer)
    {
        var count = (int)Math.Min(buffer.Length, length - position);
        if (count == 0) { return 0; }
        source.Position = start + position;
        var read = source.Read(buffer[..count]);
        if (read == 0) { throw new EndOfStreamException(); }
        position += read;
        return read;
    }
    public override void Flush() { }
    public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
    public override void SetLength(long value) => throw new NotSupportedException();
    public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
}

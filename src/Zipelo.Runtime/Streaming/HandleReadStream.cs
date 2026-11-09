using Microsoft.Win32.SafeHandles;

namespace Zipelo.Runtime.Streaming;

// 핸들 수명은 임시 트리가 소유하며 이 스트림은 읽기 위치만 관리함.
internal sealed class HandleReadStream(SafeFileHandle handle, long length) : Stream
{
    private long position;
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
        var count = (int)Math.Min(buffer.Length, length - position);
        var read = RandomAccess.Read(handle, buffer[..count], position);
        position += read;
        return read;
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

using System.Buffers.Binary;
using Zipelo.Runtime.Streaming;

namespace Zipelo.StreamProtocolTests;

internal static class Check
{
    public static void That(bool condition, string message)
    {
        if (!condition) { throw new InvalidOperationException(message); }
    }

    public static void Equal<T>(T expected, T actual) =>
        That(EqualityComparer<T>.Default.Equals(expected, actual), $"기대값 {expected}, 실제값 {actual}");

    public static void Bytes(ReadOnlySpan<byte> expected, ReadOnlySpan<byte> actual) =>
        That(expected.SequenceEqual(actual), "전송 바이트가 다름");

    public static T Throws<T>(Action run) where T : Exception
    {
        try { run(); }
        catch (T exception) { return exception; }
        throw new InvalidOperationException($"예상 예외가 없음: {typeof(T).Name}");
    }

    public static async Task<T> ThrowsAsync<T>(Func<Task> run) where T : Exception
    {
        try { await run(); }
        catch (T exception) { return exception; }
        throw new InvalidOperationException($"예상 예외가 없음: {typeof(T).Name}");
    }

    public static async Task Error(WorkerStreamError expected, Func<Task> run) =>
        Equal(expected, (await ThrowsAsync<WorkerStreamException>(run)).Error);
}

internal static class Wire
{
    public static readonly Guid Job = new("00112233-4455-6677-8899-aabbccddeeff");
    public static readonly Guid Item = new("10213243-5465-7687-98a9-bacbdcedfe0f");
    public static readonly Guid OtherItem = new("20314253-6475-8697-a8b9-cadbecfd0e1f");

    public static byte[] Length(ulong value)
    {
        var result = new byte[8];
        BinaryPrimitives.WriteUInt64LittleEndian(result, value);
        return result;
    }

    public static WorkerStreamFrame Frame(WorkerStreamFrameKind kind, ulong sequence,
        byte[]? payload = null, Guid? item = null, Guid? job = null)
    {
        var bytes = payload ?? [];
        return new(new(kind, job ?? Job, item ?? Guid.Empty, sequence, bytes.Length), bytes);
    }

    public static WorkerStreamFrame Begin => Frame(WorkerStreamFrameKind.BeginJob, 0);
    public static WorkerStreamFrame End(ulong sequence, byte status = 1) =>
        Frame(WorkerStreamFrameKind.EndJob, sequence, [status]);

    // 제품 인코더를 호출하지 않는 문서 기준 바이트 픽스처임.
    public static byte[] Encode(WorkerStreamHeader header, ReadOnlySpan<byte> payload)
    {
        var bytes = new byte[64 + payload.Length];
        "ZIP2"u8.CopyTo(bytes);
        BinaryPrimitives.WriteUInt16LittleEndian(bytes.AsSpan(4), 2);
        BinaryPrimitives.WriteUInt16LittleEndian(bytes.AsSpan(6), (ushort)header.Kind);
        BinaryPrimitives.WriteUInt32LittleEndian(bytes.AsSpan(8), unchecked((uint)header.PayloadLength));
        header.JobId.TryWriteBytes(bytes.AsSpan(16, 16));
        header.ItemId.TryWriteBytes(bytes.AsSpan(32, 16));
        BinaryPrimitives.WriteUInt64LittleEndian(bytes.AsSpan(48), header.Sequence);
        payload.CopyTo(bytes.AsSpan(64));
        return bytes;
    }

    public static byte[] Encode(WorkerStreamFrame frame) => Encode(frame.Header, frame.Payload.Span);

    public static byte[] Join(params byte[][] parts)
    {
        using var stream = new MemoryStream();
        foreach (var part in parts) { stream.Write(part); }
        return stream.ToArray();
    }

    public static WorkerStreamFrame[] JobFrames(params int[] sizes)
    {
        var frames = new List<WorkerStreamFrame> { Begin };
        if (sizes.Length != 0)
        {
            var length = (ulong)sizes.Sum();
            frames.Add(Frame(WorkerStreamFrameKind.BeginItem, 1, Length(length), Item));
            foreach (var size in sizes)
            {
                if (size == 0) { continue; }
                var payload = Enumerable.Range(0, size).Select(index => (byte)(index % 251)).ToArray();
                frames.Add(Frame(WorkerStreamFrameKind.Data, (ulong)frames.Count, payload, Item));
            }
            frames.Add(Frame(WorkerStreamFrameKind.EndItem, (ulong)frames.Count, Length(length), Item));
        }
        frames.Add(End((ulong)frames.Count));
        return frames.ToArray();
    }

    public static WorkerStreamValidator Active(ulong length = 1, WorkerStreamLimits? limits = null)
    {
        var validator = new WorkerStreamValidator(Job, limits);
        Accept(validator, Begin);
        Accept(validator, Frame(WorkerStreamFrameKind.BeginItem, 1, Length(length), Item));
        return validator;
    }

    public static void Accept(WorkerStreamValidator validator, WorkerStreamFrame frame) =>
        validator.Accept(frame.Header, frame.Payload.Span);

    public static async Task Write(WorkerStreamChannel channel, WorkerStreamFrame frame,
        CancellationToken token = default) => await channel.WriteAsync(frame.Header, frame.Payload, token);
}

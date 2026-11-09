using System.Buffers.Binary;
using Zipelo.Runtime.Streaming;

namespace Zipelo.StreamProtocolTests;

internal static class ChannelTests
{
    private static readonly TimeSpan Guard = TimeSpan.FromSeconds(4);

    public static async Task RoundTrip(int[] sizes, int fragment = int.MaxValue)
    {
        var frames = Wire.JobFrames(sizes);
        using var output = new ControlledStream();
        using (var writer = new WorkerStreamChannel(output, WorkerStreamDirection.Write, Wire.Job))
        {
            foreach (var frame in frames) { await Wire.Write(writer, frame); }
            Check.Equal(WorkerStreamPhase.Terminal, writer.Phase);
            Check.Equal((long)sizes.Sum(), writer.TotalDataBytes);
            Check.Equal(frames.Length, writer.FramesAccepted);
            Check.Equal((long)output.Written.Length, writer.TotalWireBytes);
        }
        Check.That(output.CanWrite, "채널이 호출자 스트림을 닫음");
        Check.Bytes(Wire.Join(frames.Select(Wire.Encode).ToArray()), output.Written);
        using var input = new ControlledStream(output.Written) { ReadFragment = fragment };
        using var reader = new WorkerStreamChannel(input, WorkerStreamDirection.Read, Wire.Job);
        foreach (var expected in frames)
        {
            var actual = await reader.ReadAsync() ?? throw new InvalidOperationException("조기 EOF임");
            Check.Equal(expected.Header, actual.Header);
            Check.Bytes(expected.Payload.Span, actual.Payload.Span);
        }
        Check.Equal(WorkerStreamPhase.Terminal, reader.Phase);
        Check.That(await reader.ReadAsync() is null, "종료 후 EOF 확인에 실패함");
        Check.Equal(WorkerStreamPhase.InputCompleted, reader.Phase);
        Check.Equal<WorkerStreamTerminalStatus?>(WorkerStreamTerminalStatus.Completed, reader.TerminalStatus);
        var calls = input.ReadCalls;
        Check.That(await reader.ReadAsync() is null, "완료된 수신을 재개함");
        Check.Equal(calls, input.ReadCalls);
        Check.Equal((long)sizes.Sum(), reader.TotalDataBytes);
        Check.Equal((long)output.Written.Length, reader.TotalWireBytes);
        Check.That(input.LargestReadRequest <= 65536, "청크 상한보다 큰 읽기를 요청함");
    }

    public static async Task GoldenHeader()
    {
        Check.Equal(64, WorkerStreamChannel.HeaderBytes);
        Check.Equal(65536, WorkerStreamChannel.MaxChunkBytes);
        Check.Equal((ushort)2, WorkerStreamChannel.Version);
        var expected = Convert.FromHexString(
            "5A49503202000100000000000000000033221100554477668899AABBCCDDEEFF" + new string('0', 64));
        using var stream = new ControlledStream();
        using var writer = new WorkerStreamChannel(stream, WorkerStreamDirection.Write, Wire.Job);
        await Wire.Write(writer, Wire.Begin);
        Check.Bytes(expected, stream.Written);
        Check.Bytes(expected, Wire.Encode(Wire.Begin));
    }

    private static async Task Reject(byte[] bytes, int accepted, WorkerStreamError error, long? ceiling = null)
    {
        using var stream = new ControlledStream(bytes) { ReadCeiling = ceiling ?? long.MaxValue };
        using var reader = new WorkerStreamChannel(stream, WorkerStreamDirection.Read, Wire.Job);
        for (var index = 0; index < accepted; index++)
        {
            Check.That(await reader.ReadAsync() is not null, "정상 접두 프레임이 거절됨");
        }
        var allocatedBefore = GC.GetAllocatedBytesForCurrentThread();
        var rejected = reader.ReadAsync().AsTask();
        Check.That(rejected.IsCompleted, "메모리 입력 검사가 예기치 않게 대기함");
        await Check.Error(error, async () => { await rejected; });
        var allocated = GC.GetAllocatedBytesForCurrentThread() - allocatedBefore;
        Check.That(allocated >= 0 && allocated < 1024 * 1024, "작은 오류 프레임에 1 MiB 이상 할당함");
        Check.Equal(WorkerStreamPhase.Faulted, reader.Phase);
        Check.Equal<WorkerStreamTerminalStatus?>(null, reader.TerminalStatus);
        await Check.Error(WorkerStreamError.Faulted, async () => { await reader.ReadAsync(); });
        if (ceiling is not null) { Check.Equal(ceiling.Value, stream.BytesRead); }
        Check.That(stream.LargestReadRequest <= 65536, "큰 본문 읽기를 요청함");
    }

    public static async Task TruncatedHeaders()
    {
        var bytes = Wire.Encode(Wire.Begin);
        for (var cut = 0; cut < 64; cut++) { await Reject(bytes[..cut], 0, WorkerStreamError.Truncated); }
    }

    public static async Task TruncatedBodies()
    {
        var item = Wire.Encode(Wire.Frame(WorkerStreamFrameKind.BeginItem, 1, Wire.Length(8), Wire.Item));
        for (var cut = 0; cut < 8; cut++)
        {
            await Reject(Wire.Join(Wire.Encode(Wire.Begin), item[..(64 + cut)]), 1, WorkerStreamError.Truncated);
        }
        var frames = Wire.JobFrames(8);
        var data = Wire.Encode(frames[2]);
        for (var cut = 0; cut < 8; cut++)
        {
            await Reject(Wire.Join(Wire.Encode(frames[0]), Wire.Encode(frames[1]), data[..(64 + cut)]),
                2, WorkerStreamError.Truncated);
        }
    }

    public static async Task FixedFields()
    {
        foreach (var offset in new[] { 0, 3, 4, 5, 12, 13, 14, 15, 56, 57, 58, 59, 60, 61, 62, 63 })
        {
            var bytes = Wire.Encode(Wire.Begin);
            bytes[offset] ^= 0x80;
            await Reject(bytes, 0, WorkerStreamError.InvalidHeader, 64);
        }
    }

    public static async Task KindsAndLengths()
    {
        foreach (var kind in new ushort[] { 0, 6, ushort.MaxValue })
        {
            var bytes = Wire.Encode(Wire.Begin);
            BinaryPrimitives.WriteUInt16LittleEndian(bytes.AsSpan(6), kind);
            await Reject(bytes, 0, WorkerStreamError.InvalidLength, 64);
        }
        foreach (var (kind, length) in new (ushort, uint)[]
        {
            (1, 1), (2, 0), (2, 7), (2, 9), (3, 0), (3, 65537),
            (3, int.MaxValue), (3, uint.MaxValue), (4, 0), (4, 9), (5, 0), (5, 2)
        })
        {
            var bytes = Wire.Encode(Wire.Begin);
            BinaryPrimitives.WriteUInt16LittleEndian(bytes.AsSpan(6), kind);
            BinaryPrimitives.WriteUInt32LittleEndian(bytes.AsSpan(8), length);
            await Reject(bytes, 0, WorkerStreamError.InvalidLength, 64);
        }
    }

    public static async Task HugeDeclarations()
    {
        foreach (var length in new ulong[] { 16777217, int.MaxValue, (ulong)long.MaxValue, ulong.MaxValue })
        {
            var beginItem = Wire.Frame(WorkerStreamFrameKind.BeginItem, 1, Wire.Length(length), Wire.Item);
            await Reject(Wire.Join(Wire.Encode(Wire.Begin), Wire.Encode(beginItem)), 1,
                WorkerStreamError.BudgetExceeded, 136);
        }
        var limitedItem = Wire.Frame(WorkerStreamFrameKind.BeginItem, 1, Wire.Length(0), Wire.Item);
        using var stream = new ControlledStream(Wire.Join(Wire.Encode(Wire.Begin), Wire.Encode(limitedItem)))
        {
            ReadCeiling = 128
        };
        using var reader = new WorkerStreamChannel(stream, WorkerStreamDirection.Read, Wire.Job,
            new() { MaxWireBytes = 129 });
        await reader.ReadAsync();
        await Check.Error(WorkerStreamError.BudgetExceeded, async () => { await reader.ReadAsync(); });
        Check.Equal(128L, stream.BytesRead);
        Check.Equal(WorkerStreamPhase.Faulted, reader.Phase);
    }

    public static async Task EarlyEof()
    {
        var frames = Wire.JobFrames(1);
        for (var count = 0; count < frames.Length; count++)
        {
            await Reject(Wire.Join(frames.Take(count).Select(Wire.Encode).ToArray()),
                count, WorkerStreamError.Truncated);
        }
    }

    public static async Task TrailingData()
    {
        var frames = Wire.JobFrames();
        var complete = Wire.Join(frames.Select(Wire.Encode).ToArray());
        foreach (var suffix in new[] { new byte[] { 0 }, Wire.Encode(Wire.End(2)) })
        {
            await Reject(Wire.Join(complete, suffix), 2, WorkerStreamError.TrailingData);
        }
    }

    public static async Task FailedTerminals()
    {
        foreach (var status in new byte[] { 2, 3 })
        {
            var frames = new[] { Wire.Begin,
                Wire.Frame(WorkerStreamFrameKind.BeginItem, 1, Wire.Length(10), Wire.Item), Wire.End(2, status) };
            using var stream = new ControlledStream(Wire.Join(frames.Select(Wire.Encode).ToArray()));
            using var reader = new WorkerStreamChannel(stream, WorkerStreamDirection.Read, Wire.Job);
            foreach (var unused in frames) { Check.That(await reader.ReadAsync() is not null, "조기 종료임"); }
            Check.That(await reader.ReadAsync() is null, "EOF가 아님");
            Check.Equal(WorkerStreamPhase.InputCompleted, reader.Phase);
            Check.Equal<WorkerStreamTerminalStatus?>((WorkerStreamTerminalStatus)status, reader.TerminalStatus);
        }
    }

    public static async Task PreCancelled()
    {
        foreach (var direction in new[] { WorkerStreamDirection.Read, WorkerStreamDirection.Write })
        {
            using var stream = new ControlledStream();
            using var channel = new WorkerStreamChannel(stream, direction, Wire.Job);
            using var cancellation = new CancellationTokenSource();
            cancellation.Cancel();
            await Check.ThrowsAsync<OperationCanceledException>(() => direction == WorkerStreamDirection.Read
                ? Read(channel, cancellation.Token) : Wire.Write(channel, Wire.Begin, cancellation.Token));
            Check.Equal(0, stream.ReadCalls + stream.WriteCalls);
            Check.Equal(WorkerStreamPhase.Faulted, channel.Phase);
            await Check.Error(WorkerStreamError.Faulted, () => direction == WorkerStreamDirection.Read
                ? Read(channel, default) : Wire.Write(channel, Wire.Begin));
        }
    }

    private static async Task Read(WorkerStreamChannel channel, CancellationToken token) =>
        _ = await channel.ReadAsync(token);

    public static async Task BlockedRead(bool body, bool deadline, bool resume = false)
    {
        var frames = Wire.JobFrames(8);
        using var stream = new ControlledStream(Wire.Join(frames.Select(Wire.Encode).ToArray()));
        using var channel = new WorkerStreamChannel(stream, WorkerStreamDirection.Read, Wire.Job,
            new() { MaxDuration = deadline ? TimeSpan.FromMilliseconds(180) : TimeSpan.FromSeconds(10) });
        await channel.ReadAsync();
        await channel.ReadAsync();
        stream.BlockRead = stream.ReadCalls + (body ? 3 : 1);
        using var cancellation = new CancellationTokenSource();
        var pending = channel.ReadAsync(cancellation.Token).AsTask();
        try
        {
            await stream.Entered.Task.WaitAsync(Guard);
            Check.That(!pending.IsCompleted, "생산자를 기다리지 않고 읽기를 완료함");
            Check.Equal(2, channel.FramesAccepted);
            if (resume)
            {
                stream.Release.TrySetResult();
                var frame = await pending.WaitAsync(Guard) ?? throw new InvalidOperationException();
                Check.Bytes(frames[2].Payload.Span, frame.Payload.Span);
                return;
            }
            if (!deadline) { cancellation.Cancel(); }
            if (deadline) { await Check.Error(WorkerStreamError.BudgetExceeded, async () => { await pending.WaitAsync(Guard); }); }
            else { await Check.ThrowsAsync<OperationCanceledException>(async () => { await pending.WaitAsync(Guard); }); }
            Check.Equal(WorkerStreamPhase.Faulted, channel.Phase);
            Check.Equal<WorkerStreamTerminalStatus?>(null, channel.TerminalStatus);
            if (!deadline) { await Check.Error(WorkerStreamError.Faulted, () => Read(channel, default)); }
        }
        finally
        {
            cancellation.Cancel();
            try { await pending.WaitAsync(Guard); }
            catch (OperationCanceledException) { }
            catch (WorkerStreamException) { }
        }
    }

    public static async Task BlockedWrite(string stage, bool deadline, bool resume = false)
    {
        using var stream = new ControlledStream();
        using var channel = new WorkerStreamChannel(stream, WorkerStreamDirection.Write, Wire.Job,
            new() { MaxDuration = deadline ? TimeSpan.FromMilliseconds(180) : TimeSpan.FromSeconds(10) });
        await Wire.Write(channel, Wire.Begin);
        await Wire.Write(channel, Wire.Frame(WorkerStreamFrameKind.BeginItem, 1, Wire.Length(3), Wire.Item));
        if (stage == "header") { stream.BlockWrite = stream.WriteCalls + 1; }
        if (stage == "body") { stream.BlockWrite = stream.WriteCalls + 2; }
        if (stage == "flush") { stream.BlockFlush = stream.FlushCalls + 1; }
        byte[] payload = [1, 2, 3];
        var frame = Wire.Frame(WorkerStreamFrameKind.Data, 2, payload, Wire.Item);
        using var cancellation = new CancellationTokenSource();
        var pending = Wire.Write(channel, frame, cancellation.Token);
        try
        {
            await stream.Entered.Task.WaitAsync(Guard);
            Check.That(!pending.IsCompleted, "소비자나 Flush를 기다리지 않고 완료함");
            if (resume)
            {
                payload.AsSpan().Fill(99);
                stream.Release.TrySetResult();
                await pending.WaitAsync(Guard);
                Check.Bytes(new byte[] { 1, 2, 3 }, stream.Written.AsSpan(200, 3));
                Check.Equal(3L, channel.TotalDataBytes);
                return;
            }
            if (!deadline) { cancellation.Cancel(); }
            if (deadline) { await Check.Error(WorkerStreamError.BudgetExceeded, () => pending.WaitAsync(Guard)); }
            else { await Check.ThrowsAsync<OperationCanceledException>(() => pending.WaitAsync(Guard)); }
            Check.Equal(WorkerStreamPhase.Faulted, channel.Phase);
            Check.Equal<WorkerStreamTerminalStatus?>(null, channel.TerminalStatus);
            if (!deadline) { await Check.Error(WorkerStreamError.Faulted, () => Wire.Write(channel, Wire.Begin)); }
        }
        finally
        {
            cancellation.Cancel();
            try { await pending.WaitAsync(Guard); }
            catch (OperationCanceledException) { }
            catch (WorkerStreamException) { }
        }
    }

    public static async Task MissingEof()
    {
        using var stream = new ControlledStream(Wire.Join(Wire.Encode(Wire.Begin), Wire.Encode(Wire.End(1))));
        using var channel = new WorkerStreamChannel(stream, WorkerStreamDirection.Read, Wire.Job,
            new() { MaxDuration = TimeSpan.FromMilliseconds(180) });
        await channel.ReadAsync();
        await channel.ReadAsync();
        stream.BlockRead = stream.ReadCalls + 1;
        await Check.Error(WorkerStreamError.BudgetExceeded, async () => { await channel.ReadAsync().AsTask().WaitAsync(Guard); });
        Check.Equal(WorkerStreamPhase.Faulted, channel.Phase);
        Check.Equal<WorkerStreamTerminalStatus?>(null, channel.TerminalStatus);
    }

    public static async Task FinalStatePriority()
    {
        foreach (var status in new byte[] { 1, 2, 3 })
        {
            using var stream = new ControlledStream(Wire.Join(Wire.Encode(Wire.Begin), Wire.Encode(Wire.End(1, status))));
            using var reader = new WorkerStreamChannel(stream, WorkerStreamDirection.Read, Wire.Job,
                new() { MaxDuration = TimeSpan.FromMilliseconds(250) });
            await reader.ReadAsync();
            await reader.ReadAsync();
            Check.That(await reader.ReadAsync() is null, "종료 EOF가 아님");
            var calls = stream.ReadCalls;
            using var cancelled = new CancellationTokenSource();
            cancelled.Cancel();
            Check.That(await reader.ReadAsync(cancelled.Token) is null, "확정 EOF를 취소가 바꿈");
            await Task.Delay(400);
            Check.That(await reader.ReadAsync() is null, "확정 EOF를 기한이 바꿈");
            Check.Equal(WorkerStreamPhase.InputCompleted, reader.Phase);
            Check.Equal<WorkerStreamTerminalStatus?>((WorkerStreamTerminalStatus)status, reader.TerminalStatus);
            Check.Equal(calls, stream.ReadCalls);
        }

        var bytes = Wire.Encode(Wire.Begin);
        bytes[0] = 0;
        using var input = new ControlledStream(bytes);
        using var faulted = new WorkerStreamChannel(input, WorkerStreamDirection.Read, Wire.Job,
            new() { MaxDuration = TimeSpan.FromMilliseconds(250) });
        await Check.Error(WorkerStreamError.InvalidHeader, () => Read(faulted, default));
        var reads = input.ReadCalls;
        using var cancellation = new CancellationTokenSource();
        cancellation.Cancel();
        await Check.Error(WorkerStreamError.Faulted, () => Read(faulted, cancellation.Token));
        await Task.Delay(400);
        await Check.Error(WorkerStreamError.Faulted, () => Read(faulted, default));
        Check.Equal(WorkerStreamPhase.Faulted, faulted.Phase);
        Check.Equal<WorkerStreamTerminalStatus?>(null, faulted.TerminalStatus);
        Check.Equal(reads, input.ReadCalls);
    }

    public static async Task BetweenCallsDeadline()
    {
        using var stream = new ControlledStream();
        using var channel = new WorkerStreamChannel(stream, WorkerStreamDirection.Write, Wire.Job,
            new() { MaxDuration = TimeSpan.FromMilliseconds(40) });
        await Task.Delay(100);
        await Check.Error(WorkerStreamError.BudgetExceeded, () => Wire.Write(channel, Wire.Begin));
        Check.Equal(0, stream.WriteCalls);
        Check.Equal(WorkerStreamPhase.Faulted, channel.Phase);
    }

    public static async Task IoFailure(bool read)
    {
        using var stream = new ControlledStream(read ? Wire.Encode(Wire.Begin) : null)
        {
            FailRead = read ? 2 : -1, FailWrite = read ? -1 : 2
        };
        using var channel = new WorkerStreamChannel(stream,
            read ? WorkerStreamDirection.Read : WorkerStreamDirection.Write, Wire.Job);
        var exception = await Check.ThrowsAsync<IOException>(() => read
            ? Read(channel, default) : Wire.Write(channel, Wire.Begin));
        Check.That(ReferenceEquals(stream.Failure, exception), "원래 I/O 오류를 바꿈");
        Check.Equal(WorkerStreamPhase.Faulted, channel.Phase);
        await Check.Error(WorkerStreamError.Faulted, () => read
            ? Read(channel, default) : Wire.Write(channel, Wire.Begin));
    }

    public static async Task ConcurrentCalls()
    {
        using var stream = new ControlledStream(Wire.Encode(Wire.Begin)) { BlockRead = 1 };
        using var channel = new WorkerStreamChannel(stream, WorkerStreamDirection.Read, Wire.Job);
        using var cancellation = new CancellationTokenSource();
        var pending = channel.ReadAsync(cancellation.Token).AsTask();
        try
        {
            await stream.Entered.Task.WaitAsync(Guard);
            await Check.ThrowsAsync<InvalidOperationException>(() => Read(channel, default));
            Check.Throws<InvalidOperationException>(channel.Dispose);
            Check.Equal(WorkerStreamPhase.Ready, channel.Phase);
            stream.Release.TrySetResult();
            Check.Equal(Wire.Begin.Header, (await pending.WaitAsync(Guard))!.Header);
        }
        finally
        {
            cancellation.Cancel();
            try { await pending.WaitAsync(Guard); }
            catch (OperationCanceledException) { }
        }
    }

    public static async Task DirectionAndLifetime()
    {
        using var stream = new ControlledStream();
        using var channel = new WorkerStreamChannel(stream, WorkerStreamDirection.Write, Wire.Job);
        await Check.ThrowsAsync<InvalidOperationException>(() => Read(channel, default));
        Check.Equal(WorkerStreamPhase.Ready, channel.Phase);
        await Wire.Write(channel, Wire.Begin);
        channel.Dispose();
        channel.Dispose();
        Check.That(stream.CanWrite, "호출자 스트림을 닫음");
        await Check.ThrowsAsync<ObjectDisposedException>(() => Wire.Write(channel, Wire.End(1)));
    }

    public static async Task InvalidWrite()
    {
        foreach (var length in new[] { -1, 1, 65537, int.MaxValue })
        {
            using var stream = new ControlledStream();
            using var writer = new WorkerStreamChannel(stream, WorkerStreamDirection.Write, Wire.Job);
            var frame = Wire.Begin with { Header = Wire.Begin.Header with { PayloadLength = length } };
            await Check.Error(WorkerStreamError.InvalidLength, () => Wire.Write(writer, frame));
            Check.Equal(0, stream.WriteCalls);
            Check.Equal(WorkerStreamPhase.Faulted, writer.Phase);
        }
        using var output = new ControlledStream();
        using var channel = new WorkerStreamChannel(output, WorkerStreamDirection.Write, Wire.Job);
        await Check.Error(WorkerStreamError.InvalidLength,
            () => channel.WriteAsync(Wire.Begin.Header, new byte[] { 1 }).AsTask());
        Check.Equal(0, output.WriteCalls);
    }
}

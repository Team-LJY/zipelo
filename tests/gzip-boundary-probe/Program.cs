using System.Buffers.Binary;
using System.IO.Compression;
using System.Security.Cryptography;
using System.Text;
using ICSharpCode.SharpZipLib.Zip.Compression;
using Zipelo.GzipBoundaryProbe;

internal static class Program
{
    private static int passed;
    private static int failed;

    public static int Main(string[] args)
    {
        if (args.Length != 0) { return 2; }
        Run("crc.known-vector", () => Equal(Fixture.Crc("123456789"u8), 0xcbf43926U));
        var text = "123456789"u8.ToArray();
        var large = Enumerable.Range(0, 200003).Select(i => (byte)(i * 17)).ToArray();
        foreach (var data in new[] { Array.Empty<byte>(), text, large })
        {
            var fixture = Fixture.Create(data);
            foreach (var feed in new[] { 1, 2, 3, 7, 8, 9, 31, 512, 65536 })
            {
                Run($"valid.{data.Length}.{feed}", () => Verify(fixture, data, feed));
            }
            Run($"unselected.{data.Length}", () =>
            {
                var budget = new ProbeBudget(new(Output: 0));
                var result = BoundaryProbe.Read(fixture.Bytes, budget, false,
                    _ => throw new Exception("선택하지 않은 바이트가 전달됨"));
                Equal(result.InflatedBytes, (long)data.Length);
                Equal(budget.Output, 0L);
                Equal(budget.Processed, (long)fixture.Bytes.Length + data.Length);
            });
        }
        foreach (byte flags in Enumerable.Range(0, 32).Select(i => (byte)i))
        {
            var fixture = Fixture.Create(text, flags);
            Run($"header.flags.{flags}", () => Verify(fixture, text, 1));
        }
        foreach (var length in new[] { 65520, 65521, 65535, 65536, 65537, 131073 })
        {
            var data = large[..length];
            var stored = Fixture.Create(data, level: CompressionLevel.NoCompression);
            Run($"boundary.stored-chunks.{length}", () => Verify(stored, data, 65536));
        }
        var basic = Fixture.Create(text);
        Run("boundary.readahead", () =>
        {
            var result = Verify(basic, text, 65536);
            Equal(result.ReadAheadBytes, 8);
            Equal(result.OfferedBytes, (long)basic.RawLength + 8);
        });
        Run("boundary.byte-feed", () =>
        {
            var result = Verify(basic, text, 1);
            Check(result.ReadAheadBytes <= 1);
        });
        foreach (var raw in new[] { new byte[] { 3, 0 }, new byte[] { 1, 0, 0, 255, 255 } })
        {
            var fixture = Fixture.Wrap(raw, [], 0);
            Run($"empty.raw.{raw.Length}", () => Verify(fixture, [], 1));
        }
        Run("raw.finished-consumed-unread", RawContract);
        Run("crc.known-trailer", () =>
        {
            var trailer = BinaryPrimitives.ReadUInt32LittleEndian(basic.Bytes.AsSpan(basic.Bytes.Length - 8));
            Equal(trailer, 0xcbf43926U);
        });
        foreach (var offset in new[] { 0, 1, 2 })
        {
            Run($"header.invalid.{offset}", () => Reject(Mutate(basic.Bytes, offset), Failure.Header));
        }
        foreach (var bit in new[] { 0x20, 0x40, 0x80 })
        {
            Run($"header.reserved.{bit}", () => Reject(Mutate(basic.Bytes, 3, (byte)bit), Failure.Header));
        }
        for (var i = 0; i < 4; i++)
        {
            var offset = basic.Bytes.Length - 8 + i;
            Run($"crc.byte.{i}", () => Reject(Mutate(basic.Bytes, offset), Failure.Crc));
            offset += 4;
            var sizeOffset = offset;
            Run($"isize.byte.{i}", () => Reject(Mutate(basic.Bytes, sizeOffset), Failure.Size));
        }
        var optional = Fixture.Create(text, 31);
        Run("header.fhcrc", () => Reject(Mutate(optional.Bytes, optional.HeaderLength - 2), Failure.Header));
        Run("header.fhcrc-covers-name", () => Reject(Mutate(optional.Bytes, 18), Failure.Header));
        Run("header.fname-no-path-use", () => Verify(Fixture.Create(text, 8, "../../escape"), text, 7));
        Run("header.unterminated-name", () => Reject([31, 139, 8, 8, 0, 0, 0, 0, 0, 255, 65], Failure.Truncated));
        Run("header.unterminated-comment", () => Reject([31, 139, 8, 16, 0, 0, 0, 0, 0, 255, 65], Failure.Truncated));
        Run("header.extra-truncated", () => Reject([31, 139, 8, 4, 0, 0, 0, 0, 0, 255, 255, 255, 1], Failure.Truncated));
        Run("deflate.invalid-block", () => Reject(Fixture.Wrap([7], [], 0).Bytes, Failure.Deflate));
        foreach (var suffix in new[] { new byte[] { 0 }, new byte[] { 255 }, new byte[512], basic.Bytes,
            Fixture.Create([]).Bytes, new byte[] { 31, 139 } })
        {
            Run($"trailing.{suffix.Length}.{suffix[0]}", () => Reject([.. basic.Bytes, .. suffix], Failure.TrailingData));
        }
        for (var length = 0; length < optional.Bytes.Length; length++)
        {
            var prefix = optional.Bytes[..length];
            Run($"truncate.{length}", () => RejectAny(prefix));
        }
        Run("sink.pending-after-crc-failure", () =>
        {
            using var sink = new HashSink();
            Expect(Failure.Crc, () => BoundaryProbe.Read(Mutate(basic.Bytes, basic.Bytes.Length - 8),
                new(new()), true, sink.Append));
            Equal(sink.Bytes, (long)text.Length);
            Check(sink.CommittedHash == null);
        });
        Run("unselected.bad-crc", () => Reject(Mutate(basic.Bytes, basic.Bytes.Length - 8), Failure.Crc, selected: false));
        Run("unselected.trailing", () => Reject([.. basic.Bytes, 0], Failure.TrailingData, selected: false));
        Run("budget.input", () => Reject(basic.Bytes, Failure.Budget, new(Input: basic.Bytes.Length - 1)));
        Run("budget.processed", () => Reject(basic.Bytes, Failure.Budget,
            new(Processed: basic.Bytes.Length + text.Length - 1)));
        Run("budget.output", () => Reject(basic.Bytes, Failure.Budget, new(Output: text.Length - 1)));
        Run("budget.metadata", () => Reject(optional.Bytes, Failure.Budget, new(Metadata: optional.HeaderLength - 1)));
        Run("budget.exact", () => Verify(optional, text, 65536,
            new(Input: optional.Bytes.Length, Processed: optional.Bytes.Length + text.Length,
                Output: text.Length, Metadata: optional.HeaderLength)));
        Run("budget.metadata-cap", () => Reject(Fixture.Create([], 8, new string('a', 1024 * 1024)).Bytes,
            Failure.Budget, new(Metadata: long.MaxValue)));
        Run("budget.input-cap", () => Reject(new byte[16 * 1024 * 1024 + 1], Failure.Budget, new(Input: long.MaxValue)));
        Run("budget.clamps", () =>
        {
            var budget = new ProbeBudget(new(long.MaxValue, long.MaxValue, long.MaxValue, long.MaxValue, TimeSpan.MaxValue));
            Equal(budget.InputLimit, 16777216L);
            Equal(budget.ProcessedLimit, 134217728L);
            Equal(budget.OutputLimit, 16777216L);
            Equal(budget.MetadataLimit, 1048576L);
            Equal(budget.Duration, TimeSpan.FromMinutes(5));
        });
        Run("budget.cumulative-processed", () =>
        {
            var budget = new ProbeBudget(new(Processed: 2L * (basic.Bytes.Length + text.Length) - 1));
            using var first = new HashSink();
            first.Complete(BoundaryProbe.Read(basic.Bytes, budget, true, first.Append));
            using var second = new HashSink();
            Expect(Failure.Budget, () => BoundaryProbe.Read(basic.Bytes, budget, true, second.Append));
            Check(first.CommittedHash != null && second.CommittedHash == null);
        });
        Run("budget.cumulative-output", () =>
        {
            var budget = new ProbeBudget(new(Output: text.Length));
            BoundaryProbe.Read(basic.Bytes, budget, true, _ => { });
            Expect(Failure.Budget, () => BoundaryProbe.Read(basic.Bytes, budget, true, _ => { }));
        });
        Run("budget.cumulative-metadata", () =>
        {
            var budget = new ProbeBudget(new(Metadata: basic.HeaderLength));
            BoundaryProbe.Read(basic.Bytes, budget, false, _ => { });
            Expect(Failure.Budget, () => BoundaryProbe.Read(basic.Bytes, budget, false, _ => { }));
        });
        Run("budget.unselected-expansion", () => Reject(Fixture.Create(large).Bytes, Failure.Budget,
            new(Processed: 10000), selected: false));
        Run("budget.deadline", () =>
        {
            var clock = new ManualClock();
            var budget = new ProbeBudget(new(Duration: TimeSpan.FromSeconds(1)), clock);
            clock.Advance(TimeSpan.FromSeconds(1));
            Expect(Failure.Budget, () => BoundaryProbe.Read(basic.Bytes, budget, false, _ => { }));
        });
        Run("budget.sink-time", () =>
        {
            var clock = new ManualClock();
            var budget = new ProbeBudget(new(Duration: TimeSpan.FromSeconds(1)), clock);
            Expect(Failure.Budget, () => BoundaryProbe.Read(basic.Bytes, budget, true,
                _ => clock.Advance(TimeSpan.FromSeconds(1))));
        });
        Run("cancel.before", () =>
        {
            using var cts = new CancellationTokenSource();
            cts.Cancel();
            ExpectCanceled(() => BoundaryProbe.Read(basic.Bytes, new(new()), true, _ => { }, cts.Token));
        });
        Run("cancel.sink", () =>
        {
            using var cts = new CancellationTokenSource();
            using var sink = new HashSink();
            ExpectCanceled(() => BoundaryProbe.Read(basic.Bytes, new(new()), true,
                bytes => { sink.Append(bytes); cts.Cancel(); }, cts.Token));
            Check(sink.Bytes > 0 && sink.CommittedHash == null);
        });
        Run("cancel.unselected-inflater", () =>
        {
            using var cts = new CancellationTokenSource();
            ExpectCanceled(() => BoundaryProbe.Read(basic.Bytes, new(new()), false, _ => { }, cts.Token,
                inflater: new CancelInflater(cts)));
        });
        Run("sink.exception", () =>
        {
            var sentinel = new InvalidOperationException("시험 수신기 실패");
            try { BoundaryProbe.Read(basic.Bytes, new(new()), true, _ => throw sentinel); }
            catch (InvalidOperationException error) when (ReferenceEquals(error, sentinel)) { return; }
            throw new Exception("수신기 예외가 보존되지 않음");
        });
        Run("inflater.no-progress", () => Expect(Failure.NoProgress, () =>
            BoundaryProbe.Read(basic.Bytes, new(new()), true, _ => { }, inflater: new StalledInflater())));
        Run("inflater.contract-violation", () => Expect(Failure.Boundary, () =>
            BoundaryProbe.Read(basic.Bytes, new(new()), true, _ => { }, inflater: new BrokenInflater())));
        Console.WriteLine($"passed={passed} failed={failed}");
        return failed == 0 ? 0 : 1;
    }

    private static BoundaryResult Verify(Fixture fixture, byte[] plain, int feed, Limits? limits = null)
    {
        using var sink = new HashSink();
        var budget = new ProbeBudget(limits ?? new());
        var result = BoundaryProbe.Read(fixture.Bytes, budget, true, sink.Append, feedSize: feed);
        Check(sink.CommittedHash == null);
        sink.Complete(result);
        Equal(sink.CommittedHash!, Convert.ToHexString(SHA256.HashData(plain)));
        Equal(result.HeaderBytes, fixture.HeaderLength);
        Equal(result.DeflateBytes, (long)fixture.RawLength);
        Equal(result.InflatedBytes, (long)plain.Length);
        Equal(result.InflaterTotalOut, (long)plain.Length);
        Equal(result.MemberBytes, fixture.Bytes.Length);
        Equal(result.OfferedBytes - result.ReadAheadBytes, result.DeflateBytes);
        Equal(budget.Processed, (long)fixture.Bytes.Length + plain.Length);
        Equal(budget.Metadata, (long)fixture.HeaderLength);
        Equal(budget.Output, (long)plain.Length);
        Check(sink.MaxChunk <= ProbeBudget.ChunkSize);
        return result;
    }

    private static void RawContract()
    {
        var data = Enumerable.Repeat((byte)65, 16384).ToArray();
        var fixture = Fixture.Create(data);
        var raw = fixture.Bytes.AsSpan(fixture.HeaderLength, fixture.RawLength).ToArray();
        byte[] input = [.. raw, 11, 22, 33, 44, 55, 66, 77, 88];
        var inflater = new Inflater(true);
        inflater.SetInput(input);
        var output = new byte[1];
        var count = 0;
        var pendingObserved = false;
        while (!inflater.IsFinished)
        {
            Check(count <= data.Length);
            var before = inflater.TotalIn;
            var n = inflater.Inflate(output);
            Check(n == 1 || inflater.IsFinished || inflater.TotalIn > before);
            if (n == 1) { Equal(output[0], (byte)65); count++; }
            Equal(inflater.TotalIn + inflater.RemainingInput, (long)input.Length);
            if (count < data.Length && inflater.RemainingInput >= 8) { pendingObserved |= !inflater.IsFinished; }
        }
        Equal(count, data.Length);
        Equal(inflater.TotalIn, (long)raw.Length);
        Equal(inflater.RemainingInput, 8);
        Equal(inflater.TotalOut, (long)data.Length);
        Check(pendingObserved);
        Check(input.AsSpan(input.Length - inflater.RemainingInput).SequenceEqual(new byte[] { 11, 22, 33, 44, 55, 66, 77, 88 }));
    }

    private static void Reject(byte[] input, Failure failure, Limits? limits = null, bool selected = true)
    {
        using var sink = new HashSink();
        Expect(failure, () => BoundaryProbe.Read(input, new(limits ?? new()), selected, sink.Append));
        Check(sink.CommittedHash == null);
    }

    private static void RejectAny(byte[] input)
    {
        using var sink = new HashSink();
        try { BoundaryProbe.Read(input, new(new()), true, sink.Append, feedSize: 1); }
        catch (ProbeException error) when (error.Failure is Failure.Header or Failure.Truncated or Failure.Deflate or Failure.Crc or Failure.Size)
        {
            Check(sink.CommittedHash == null);
            return;
        }
        throw new Exception("잘린 입력이 거부되지 않음");
    }

    private static byte[] Mutate(byte[] input, int offset, byte mask = 1)
    {
        var copy = (byte[])input.Clone();
        copy[offset] ^= mask;
        return copy;
    }

    private static void Expect(Failure failure, Action action)
    {
        try { action(); }
        catch (ProbeException error) when (error.Failure == failure) { return; }
        throw new Exception($"예상 실패 없음: {failure}");
    }

    private static void ExpectCanceled(Action action)
    {
        try { action(); }
        catch (OperationCanceledException) { return; }
        throw new Exception("취소가 전파되지 않음");
    }

    private static void Run(string name, Action action)
    {
        try { action(); passed++; Console.WriteLine($"PASS {name}"); }
        catch (Exception error) { failed++; Console.WriteLine($"FAIL {name}: {error.GetType().Name}: {error.Message}"); }
    }

    private static void Check(bool value)
    {
        if (!value) { throw new Exception("검사 실패"); }
    }

    private static void Equal<T>(T actual, T expected) where T : notnull => Check(EqualityComparer<T>.Default.Equals(actual, expected));
}

internal sealed class HashSink : IDisposable
{
    private readonly IncrementalHash hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
    public long Bytes { get; private set; }
    public int MaxChunk { get; private set; }
    public string? CommittedHash { get; private set; }

    public void Append(ReadOnlyMemory<byte> bytes)
    {
        if (CommittedHash != null || bytes.Length > ProbeBudget.ChunkSize) { throw new InvalidOperationException(); }
        hash.AppendData(bytes.Span);
        Bytes += bytes.Length;
        MaxChunk = Math.Max(MaxChunk, bytes.Length);
    }

    public void Complete(BoundaryResult result)
    {
        if (Bytes != result.InflatedBytes || CommittedHash != null) { throw new InvalidOperationException(); }
        CommittedHash = Convert.ToHexString(hash.GetHashAndReset());
    }

    public void Dispose() => hash.Dispose();
}

internal sealed record Fixture(byte[] Bytes, int HeaderLength, int RawLength)
{
    public static Fixture Create(byte[] plain, byte flags = 0, string name = "sample.txt",
        CompressionLevel level = CompressionLevel.Optimal)
    {
        using var raw = new MemoryStream();
        using (var compressor = new DeflateStream(raw, level, true)) { compressor.Write(plain); }
        return Wrap(raw.ToArray(), plain, flags, name);
    }

    public static Fixture Wrap(byte[] raw, byte[] plain, byte flags, string name = "sample.txt")
    {
        using var output = new MemoryStream();
        output.Write(new byte[] { 31, 139, 8, flags, 0, 0, 0, 0, 0, 255 });
        if ((flags & 4) != 0) { output.Write(new byte[] { 4, 0, 11, 22, 33, 44 }); }
        if ((flags & 8) != 0) { output.Write(Encoding.Latin1.GetBytes(name)); output.WriteByte(0); }
        if ((flags & 16) != 0) { output.Write("comment"u8); output.WriteByte(0); }
        if ((flags & 2) != 0)
        {
            var crc = Crc(output.ToArray());
            output.WriteByte((byte)crc);
            output.WriteByte((byte)(crc >> 8));
        }
        var header = (int)output.Length;
        output.Write(raw);
        Span<byte> trailer = stackalloc byte[8];
        BinaryPrimitives.WriteUInt32LittleEndian(trailer, Crc(plain));
        BinaryPrimitives.WriteUInt32LittleEndian(trailer[4..], (uint)plain.Length);
        output.Write(trailer);
        return new(output.ToArray(), header, raw.Length);
    }

    // 시험 기대값은 라이브러리 CRC 구현과 독립적으로 계산함.
    public static uint Crc(ReadOnlySpan<byte> bytes)
    {
        var crc = uint.MaxValue;
        foreach (var value in bytes)
        {
            crc ^= value;
            for (var bit = 0; bit < 8; bit++) { crc = (crc >> 1) ^ ((crc & 1) == 0 ? 0U : 0xedb88320U); }
        }
        return ~crc;
    }
}

internal sealed class ManualClock : TimeProvider
{
    private long ticks;
    public override long TimestampFrequency => TimeSpan.TicksPerSecond;
    public override long GetTimestamp() => ticks;
    public void Advance(TimeSpan duration) => ticks += duration.Ticks;
}

internal class StalledInflater : IRawInflater
{
    protected int supplied;
    public bool IsFinished => false;
    public bool IsNeedingInput => supplied == 0;
    public bool IsNeedingDictionary => false;
    public virtual long TotalIn => 0;
    public long TotalOut => 0;
    public int RemainingInput => supplied;
    public void SetInput(byte[] bytes, int count) => supplied += count;
    public virtual int Inflate(byte[] output) => 0;
}

internal sealed class BrokenInflater : StalledInflater
{
    public override long TotalIn => supplied == 0 ? 0 : supplied + 1L;
}

internal sealed class CancelInflater(CancellationTokenSource cts) : StalledInflater
{
    public override int Inflate(byte[] output) { cts.Cancel(); return 0; }
}

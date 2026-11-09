using System.Buffers.Binary;
using ICSharpCode.SharpZipLib;
using ICSharpCode.SharpZipLib.Checksum;
using ICSharpCode.SharpZipLib.Zip.Compression;

namespace Zipelo.GzipBoundaryProbe;

internal enum Failure
{
    Header, Truncated, Deflate, Boundary, Crc, Size, TrailingData, Budget, NoProgress
}

internal sealed class ProbeException(Failure failure) : Exception(failure.ToString())
{
    public Failure Failure { get; } = failure;
}

internal sealed record Limits(
    long Input = 16L * 1024 * 1024,
    long Processed = 128L * 1024 * 1024,
    long Output = 16L * 1024 * 1024,
    long Metadata = 1024 * 1024,
    TimeSpan? Duration = null);

// 여러 순회의 처리량과 선택 출력, 메타데이터 및 시간을 누적함.
internal sealed class ProbeBudget
{
    public const int ChunkSize = 65536;
    public long InputLimit { get; }
    public long ProcessedLimit { get; }
    public long OutputLimit { get; }
    public long MetadataLimit { get; }
    public TimeSpan Duration { get; }
    public long Processed { get; private set; }
    public long Output { get; private set; }
    public long Metadata { get; private set; }
    private readonly TimeProvider clock;
    private readonly long started;

    public ProbeBudget(Limits limits, TimeProvider? clock = null)
    {
        if (limits.Input < 0 || limits.Processed < 0 || limits.Output < 0 || limits.Metadata < 0 ||
            limits.Duration is { } duration && duration <= TimeSpan.Zero)
        {
            throw new ArgumentOutOfRangeException(nameof(limits));
        }
        InputLimit = Math.Min(limits.Input, 16L * 1024 * 1024);
        ProcessedLimit = Math.Min(limits.Processed, 128L * 1024 * 1024);
        OutputLimit = Math.Min(limits.Output, 16L * 1024 * 1024);
        MetadataLimit = Math.Min(limits.Metadata, 1024 * 1024);
        Duration = limits.Duration is { } requested && requested < TimeSpan.FromMinutes(5)
            ? requested : TimeSpan.FromMinutes(5);
        this.clock = clock ?? TimeProvider.System;
        started = this.clock.GetTimestamp();
    }

    public void Check(CancellationToken token)
    {
        token.ThrowIfCancellationRequested();
        if (Processed > ProcessedLimit || clock.GetElapsedTime(started) >= Duration) { throw new ProbeException(Failure.Budget); }
    }

    public void ChargeProcessed(int count)
    {
        if (count < 0 || count > ProcessedLimit - Processed) { throw new ProbeException(Failure.Budget); }
        Processed += count;
    }

    // 이미 생성된 출력은 한도 초과 시에도 기록하며 이후 호출을 차단함.
    public void ChargeDecoded(int count)
    {
        if (count < 0 || count > ChunkSize) { throw new ProbeException(Failure.Boundary); }
        if (Processed > ProcessedLimit) { throw new ProbeException(Failure.Budget); }
        Processed += count;
        if (Processed > ProcessedLimit) { throw new ProbeException(Failure.Budget); }
    }

    public void ChargeOutput(int count)
    {
        if (count < 0 || count > OutputLimit - Output) { throw new ProbeException(Failure.Budget); }
        Output += count;
    }

    public void ChargeMetadata()
    {
        if (Metadata >= MetadataLimit) { throw new ProbeException(Failure.Budget); }
        Metadata++;
    }
}

// 시험용 어댑터이며 생산 코드에서 참조하지 않음.
internal interface IRawInflater
{
    bool IsFinished { get; }
    bool IsNeedingInput { get; }
    bool IsNeedingDictionary { get; }
    long TotalIn { get; }
    long TotalOut { get; }
    int RemainingInput { get; }
    void SetInput(byte[] bytes, int count);
    int Inflate(byte[] output);
}

internal sealed class SharpInflater : IRawInflater
{
    private readonly Inflater inflater = new(true);
    public bool IsFinished => inflater.IsFinished;
    public bool IsNeedingInput => inflater.IsNeedingInput;
    public bool IsNeedingDictionary => inflater.IsNeedingDictionary;
    public long TotalIn => inflater.TotalIn;
    public long TotalOut => inflater.TotalOut;
    public int RemainingInput => inflater.RemainingInput;
    public void SetInput(byte[] bytes, int count) => inflater.SetInput(bytes, 0, count);
    public int Inflate(byte[] output) => inflater.Inflate(output, 0, output.Length);
}

internal sealed record BoundaryResult(
    int HeaderBytes, long DeflateBytes, int ReadAheadBytes, long InflatedBytes,
    uint Crc32, int MemberBytes, long OfferedBytes, long InflaterTotalOut);

internal static class BoundaryProbe
{
    // 입력 메모리는 호출 동안 불변이어야 함. 수신 청크는 성공 반환 전까지 임시 결과임.
    public static BoundaryResult Read(ReadOnlyMemory<byte> input, ProbeBudget budget,
        bool selected, Action<ReadOnlyMemory<byte>> sink, CancellationToken token = default,
        int feedSize = ProbeBudget.ChunkSize, IRawInflater? inflater = null)
    {
        ArgumentNullException.ThrowIfNull(budget);
        ArgumentNullException.ThrowIfNull(sink);
        if (feedSize is < 1 or > ProbeBudget.ChunkSize) { throw new ArgumentOutOfRangeException(nameof(feedSize)); }
        budget.Check(token);
        if (input.Length > budget.InputLimit) { throw new ProbeException(Failure.Budget); }
        var cursor = new Cursor(input, budget, token);
        var headerCrc = new Crc32();
        byte HeaderByte()
        {
            budget.Check(token);
            budget.ChargeMetadata();
            var value = cursor.Byte();
            headerCrc.Update(value);
            return value;
        }
        if (HeaderByte() != 0x1f || HeaderByte() != 0x8b || HeaderByte() != 8)
        {
            throw new ProbeException(Failure.Header);
        }
        var flags = HeaderByte();
        if ((flags & 0xe0) != 0) { throw new ProbeException(Failure.Header); }
        for (var i = 0; i < 6; i++) { HeaderByte(); }
        if ((flags & 4) != 0)
        {
            var length = HeaderByte() | (HeaderByte() << 8);
            for (var i = 0; i < length; i++) { HeaderByte(); }
        }
        if ((flags & 8) != 0) { while (HeaderByte() != 0) { } }
        if ((flags & 16) != 0) { while (HeaderByte() != 0) { } }
        if ((flags & 2) != 0)
        {
            var expected = (ushort)headerCrc.Value;
            budget.ChargeMetadata();
            var low = cursor.Byte();
            budget.ChargeMetadata();
            var high = cursor.Byte();
            if ((low | (high << 8)) != expected) { throw new ProbeException(Failure.Header); }
        }
        var headerBytes = cursor.Position;
        inflater ??= new SharpInflater();
        if (inflater.TotalIn != 0 || inflater.TotalOut != 0 || inflater.RemainingInput != 0 || inflater.IsFinished)
        {
            throw new ProbeException(Failure.Boundary);
        }
        var compressed = new byte[ProbeBudget.ChunkSize];
        var output = new byte[ProbeBudget.ChunkSize];
        var crc = new Crc32();
        long offered = 0;
        long decoded = 0;
        while (!inflater.IsFinished)
        {
            budget.Check(token);
            if (inflater.IsNeedingDictionary) { throw new ProbeException(Failure.Deflate); }
            if (inflater.IsNeedingInput)
            {
                var count = cursor.Copy(compressed, feedSize);
                if (count == 0) { throw new ProbeException(Failure.Truncated); }
                inflater.SetInput(compressed, count);
                offered += count;
            }
            var before = inflater.TotalIn;
            var beforeOut = inflater.TotalOut;
            int written;
            try { written = inflater.Inflate(output); }
            catch (SharpZipBaseException) { throw new ProbeException(Failure.Deflate); }
            finally
            {
                // 예외와 반환 직후 취소보다 먼저 실제 출력 증가분을 누적함.
                var produced = inflater.TotalOut - beforeOut;
                if (produced < 0 || produced > output.Length) { throw new ProbeException(Failure.Boundary); }
                budget.ChargeDecoded((int)produced);
            }
            budget.Check(token);
            if (written < 0 || written > output.Length || inflater.TotalIn < before ||
                inflater.TotalIn > offered || inflater.RemainingInput < 0 ||
                inflater.TotalIn != offered - inflater.RemainingInput ||
                inflater.TotalOut != decoded + written)
            {
                throw new ProbeException(Failure.Boundary);
            }
            if (written > 0)
            {
                decoded += written;
                crc.Update(new ArraySegment<byte>(output, 0, written));
                if (selected)
                {
                    budget.ChargeOutput(written);
                    sink(output.AsMemory(0, written));
                    budget.Check(token);
                }
            }
            if (written == 0 && inflater.TotalIn == before && !inflater.IsFinished && !inflater.IsNeedingInput)
            {
                throw new ProbeException(Failure.NoProgress);
            }
        }
        budget.Check(token);
        if (inflater.IsNeedingDictionary || inflater.TotalIn != offered - inflater.RemainingInput ||
            inflater.TotalOut != decoded)
        {
            throw new ProbeException(Failure.Boundary);
        }
        var readAhead = inflater.RemainingInput;
        var boundary = checked(headerBytes + (int)inflater.TotalIn);
        if (cursor.Position - boundary != readAhead) { throw new ProbeException(Failure.Boundary); }
        // 원본 메모리의 미소비 꼬리를 다시 읽음. 선행 읽기 바이트를 중복 과금하지 않음.
        cursor.Rewind(boundary);
        Span<byte> trailer = stackalloc byte[8];
        for (var i = 0; i < trailer.Length; i++) { trailer[i] = cursor.Byte(); }
        if (BinaryPrimitives.ReadUInt32LittleEndian(trailer) != (uint)crc.Value)
        {
            throw new ProbeException(Failure.Crc);
        }
        if (BinaryPrimitives.ReadUInt32LittleEndian(trailer[4..]) != unchecked((uint)decoded))
        {
            throw new ProbeException(Failure.Size);
        }
        // 연결 member는 GZip 규격에서 유효하지만 이 실험의 단일 member 범위에서는 거부함.
        if (cursor.Position != input.Length) { throw new ProbeException(Failure.TrailingData); }
        budget.Check(token);
        return new(headerBytes, inflater.TotalIn, readAhead, decoded, (uint)crc.Value,
            cursor.Position, offered, inflater.TotalOut);
    }

    private sealed class Cursor(ReadOnlyMemory<byte> input, ProbeBudget budget, CancellationToken token)
    {
        public int Position { get; private set; }
        private int highWater;

        public byte Byte()
        {
            budget.Check(token);
            if (Position == input.Length) { throw new ProbeException(Failure.Truncated); }
            Account(Position + 1);
            return input.Span[Position++];
        }

        public int Copy(byte[] destination, int max)
        {
            budget.Check(token);
            var count = Math.Min(max, input.Length - Position);
            Account(Position + count);
            input.Span.Slice(Position, count).CopyTo(destination);
            Position += count;
            return count;
        }

        public void Rewind(int position)
        {
            if (position < 0 || position > Position) { throw new ProbeException(Failure.Boundary); }
            Position = position;
        }

        private void Account(int end)
        {
            if (end > highWater)
            {
                budget.ChargeProcessed(end - highWater);
                highWater = end;
            }
        }
    }
}

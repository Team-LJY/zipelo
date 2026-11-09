using System.Buffers.Binary;
using System.Diagnostics;
using System.Security.Cryptography;

internal enum Failure { Decode, DictionaryRequired, DictionaryMismatch, Truncated, NoProgress, Limit, Mismatch }
internal sealed class ProbeFailure(Failure kind, string detail) : Exception(detail)
{
    public Failure Kind { get; } = kind;
}
internal sealed record CompletedResult(long Length, string Sha256, long Consumed);

internal sealed class StateDriver(Inflater inflater)
{
    public CompletedResult? Completed { get; private set; }
    public int Calls { get; private set; }
    public int DictionaryRequests { get; private set; }

    public void Run(Sample sample, Envelope envelope, int feedSize, int outputSize,
        byte[]? suppliedDictionary = null, bool stopAtDictionary = false,
        byte[]? wireOverride = null, bool addSentinel = true)
    {
        Require(Completed == null && Calls == 0, "driver 재사용 금지");
        Require(inflater.TotalIn == 0 && inflater.TotalOut == 0 && inflater.RemainingInput == 0,
            "새 스트림 카운터");
        Require(feedSize > 0 && outputSize > 0 && outputSize <= 65536, "분할 크기");
        var wire = wireOverride ?? sample.Wire(envelope);
        var offset = envelope == Envelope.GzipBody ? 10 : 0;
        if (envelope == Envelope.GzipBody)
        {
            Require(wire.AsSpan(0, 10).SequenceEqual(new byte[] { 31, 139, 8, 0, 0, 0, 0, 0, 0, 255 }),
                "고정 시험 헤더");
        }
        var input = new byte[wire.Length - offset + (addSentinel ? Fixtures.Sentinel.Length : 0)];
        wire.AsSpan(offset).CopyTo(input);
        if (addSentinel) { Fixtures.Sentinel.CopyTo(input, wire.Length - offset); }
        var inputHash = SHA256.HashData(input);
        var dictionary = suppliedDictionary ?? sample.Dictionary;
        var dictionaryHash = dictionary == null ? null : SHA256.HashData(dictionary);
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        var output = new byte[outputSize + 2];
        var watch = Stopwatch.StartNew();
        var offered = 0;
        long delivered = 0;
        CompletedResult? pending = null;
        try
        {
            while (!inflater.IsFinished)
            {
                if (Calls >= 1000000 || watch.Elapsed >= TimeSpan.FromSeconds(5))
                {
                    throw new ProbeFailure(Failure.Limit, "호출/시간 한도");
                }
                if (inflater.IsNeedingInput && offered < input.Length)
                {
                    var count = Math.Min(feedSize, input.Length - offered);
                    inflater.SetInput(input, offered, count);
                    offered += count;
                    Counters(offered);
                }
                Array.Fill(output, (byte)0xa6);
                var beforeIn = inflater.TotalIn;
                var beforeOut = inflater.TotalOut;
                int written;
                try
                {
                    Calls++;
                    written = inflater.Inflate(output, 1, outputSize);
                }
                catch (SharpZipBaseException)
                {
                    throw new ProbeFailure(Failure.Decode, "Inflate 거부: 문법/이력/체크섬 세부 원인은 fixture로 구분함");
                }
                Require(written >= 0 && written <= outputSize, "반환 출력 길이");
                Require(output[0] == 0xa6 && output[^1] == 0xa6, "출력 경계 보존");
                Require(inflater.TotalIn >= beforeIn && inflater.TotalOut - beforeOut == written,
                    "호출별 카운터");
                Counters(offered);
                for (var i = 0; i < written; i++)
                {
                    Require(delivered + i < sample.Output.Length &&
                        output[i + 1] == sample.Output.At((int)delivered + i), "독립 출력 패턴");
                }
                hash.AppendData(output, 1, written);
                delivered += written;
                Require(delivered == inflater.TotalOut && delivered <= 131072, "누적 출력");
                if (inflater.IsNeedingDictionary)
                {
                    DictionaryRequests++;
                    Require(envelope == Envelope.Zlib && sample.Dictionary != null && DictionaryRequests == 1,
                        "zlib 사전 요청");
                    Require(inflater.TotalIn == 6 && inflater.TotalOut == 0 && delivered == 0,
                        "FDICT/DICTID 소비량과 사전 출력 제외");
                    Require(unchecked((uint)inflater.Adler) == Fixtures.Adler(sample.Dictionary!), "DICTID");
                    if (stopAtDictionary || dictionary == null)
                    {
                        throw new ProbeFailure(Failure.DictionaryRequired, "사전 대기 상태");
                    }
                    var remaining = inflater.RemainingInput;
                    try { inflater.SetDictionary(dictionary); }
                    catch (SharpZipBaseException)
                    {
                        throw new ProbeFailure(Failure.DictionaryMismatch, "다른 Adler 사전 거부");
                    }
                    Require(!inflater.IsNeedingDictionary && inflater.TotalIn == 6 && inflater.TotalOut == 0 &&
                        inflater.RemainingInput == remaining && inflater.Adler == 1, "사전 설치 카운터/Adler 초기화");
                    Counters(offered);
                    continue;
                }
                if (written == 0 && !inflater.IsFinished)
                {
                    if (inflater.IsNeedingInput && offered == input.Length)
                    {
                        throw new ProbeFailure(Failure.Truncated, "완료 전 입력 소진");
                    }
                    if (inflater.TotalIn == beforeIn && !inflater.IsNeedingInput)
                    {
                        throw new ProbeFailure(Failure.NoProgress, "소비/출력/요청 없는 호출");
                    }
                }
            }
            var expectedConsumed = envelope == Envelope.Zlib
                ? sample.Raw.Length + (sample.Dictionary == null ? 2 : 6) + 4
                : sample.Raw.Length;
            Require(inflater.TotalIn == expectedConsumed && delivered == sample.Output.Length,
                "알려진 종료 위치/길이");
            var tailLength = (envelope == Envelope.GzipBody ? 8 : 0) + (addSentinel ? Fixtures.Sentinel.Length : 0);
            Require(input.Length - expectedConsumed == tailLength, "선행 읽기 및 미공급 꼬리");
            Require(inflater.RemainingInput == offered - expectedConsumed, "미소비 꼬리 길이");
            if (addSentinel)
            {
                Require(input.AsSpan(input.Length - Fixtures.Sentinel.Length).SequenceEqual(Fixtures.Sentinel),
                    "sentinel 보존");
            }
            if (envelope == Envelope.Zlib)
            {
                Require(unchecked((uint)inflater.Adler) == Fixtures.Adler(sample.Output), "출력 Adler");
                Require(DictionaryRequests == (sample.Dictionary == null ? 0 : 1), "사전 요청 횟수");
            }
            if (envelope == Envelope.GzipBody)
            {
                Require(BinaryPrimitives.ReadUInt32LittleEndian(input.AsSpan(expectedConsumed)) == Fixtures.Crc(sample.Output),
                    "고정 trailer CRC");
                Require(BinaryPrimitives.ReadUInt32LittleEndian(input.AsSpan(expectedConsumed + 4)) == sample.Output.Length,
                    "고정 trailer ISIZE");
            }
            var beforeFinishedIn = inflater.TotalIn;
            var beforeFinishedOut = inflater.TotalOut;
            var beforeRemaining = inflater.RemainingInput;
            Require(inflater.Inflate(output, 1, outputSize) == 0 && inflater.IsFinished &&
                inflater.TotalIn == beforeFinishedIn && inflater.TotalOut == beforeFinishedOut &&
                inflater.RemainingInput == beforeRemaining, "완료 후 추가 출력 없음");
            var actualHash = Convert.ToHexString(hash.GetHashAndReset());
            Require(actualHash == sample.Output.Sha256(), "독립 패턴 SHA256");
            pending = new(delivered, actualHash, inflater.TotalIn);
        }
        finally
        {
            Require(SHA256.HashData(input).AsSpan().SequenceEqual(inputHash), "입력 불변");
            if (dictionary != null)
            {
                Require(SHA256.HashData(dictionary).AsSpan().SequenceEqual(dictionaryHash!), "사전 불변");
            }
        }
        Completed = pending;
    }

    private void Counters(int offered) => Require(inflater.TotalIn >= 0 && inflater.RemainingInput >= 0 &&
        inflater.TotalIn + inflater.RemainingInput == offered, "소비/미소비/공급 관계");

    public static void Require(bool condition, string detail)
    {
        if (!condition) { throw new ProbeFailure(Failure.Mismatch, detail); }
    }
}

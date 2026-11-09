using System.Diagnostics;
using System.Security.Cryptography;
using System.Text.Json;
using ICSharpCode.SharpZipLib;
using ICSharpCode.SharpZipLib.Zip.Compression;

namespace Zipelo.DynamicDeflateCorpus;

internal static class Program
{
    private static readonly int[] Feeds = [1, 2, 3, 7, 65536];
    private static readonly int[] Outputs = [1, 258, 65536];
    private static readonly byte[] Sentinel = [0xde, 0xad, 0xbe, 0xef, 0x5a, 0xa5, 0xc3, 0x3c];
    private static int passed;
    private static int failed;

    public static int Main(string[] args)
    {
        if (args.Length == 1 && args[0] == "--emit-extra")
        {
            foreach (var fixture in ExtraFixtures.Create())
            {
                Console.WriteLine(JsonSerializer.Serialize(new
                {
                    fixture.Id, RawHex = Convert.ToHexString(fixture.Raw),
                    RawSha256 = Convert.ToHexString(SHA256.HashData(fixture.Raw)),
                    RawLength = fixture.Raw.Length, fixture.EndBitExclusive, fixture.Tokens, fixture.Trees,
                    OutputLength = fixture.Output?.Length, OutputSha256 = fixture.Output?.Hash(),
                    fixture.ExpectedFailure, fixture.Rationale
                }));
            }
            return 0;
        }
        if (args.Length != 0) { return 2; }

        Test("binding", () => Require(typeof(Inflater).Assembly == typeof(StreamDecodingException).Assembly &&
            typeof(Inflater).Assembly.GetName().Name == "Zipelo.Experimental.SharpZipLibTreeCandidate", "후보 assembly 연결 불일치"));
        Test("tree.single", () => { _ = new InflaterHuffmanTree(new byte[] { 1 }); });
        Test("tree.empty", () => { _ = new InflaterHuffmanTree(new byte[] { 0 }); });
        Test("tree.complete", () => { _ = new InflaterHuffmanTree(new byte[] { 1, 1 }); });
        Test("tree.length16", () => RejectTree(new byte[] { 16 }));
        Test("tree.overfull", () => RejectTree(new byte[] { 1, 1, 1 }));
        var extra = ExtraFixtures.Create().ToArray();
        Test("fixture.extra-layout", () =>
        {
            Require(extra.Length == 5, "추가 표본 개수 불일치");
            var bits = new[] { 391, 330, 371, 372, 336 };
            for (var index = 0; index < extra.Length; index++)
            {
                Require(extra[index].EndBitExclusive == bits[index], "독립 비트 길이 불일치");
                Require(extra[index].Raw.Length == (bits[index] + 7) / 8, "독립 raw 길이 불일치");
            }
        });
        foreach (var fixture in Corpus.Create().Concat(extra))
        {
            foreach (var feed in Feeds)
            {
                foreach (var output in Outputs)
                {
                    Test($"{fixture.Id}.{feed}.{output}", () => Run(fixture, feed, output));
                }
            }
        }
        Console.WriteLine(JsonSerializer.Serialize(new { Passed = passed, Failed = failed, Total = passed + failed }));
        return failed == 0 ? 0 : 1;
    }

    private static void Run(Fixture fixture, int feed, int outputSize)
    {
        var source = fixture.Raw.Concat(Sentinel).ToArray();
        var original = (byte[])source.Clone();
        var inflater = new Inflater(true);
        var output = new byte[outputSize + 2];
        var offered = 0;
        long returned = 0;
        var watch = Stopwatch.StartNew();
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        try
        {
            for (var calls = 0; calls < 1_000_000; calls++)
            {
                Require(watch.Elapsed < TimeSpan.FromSeconds(5), "시험 기한 초과");
                Array.Fill(output, (byte)0xa6);
                var beforeIn = inflater.TotalIn;
                var beforeOut = inflater.TotalOut;
                var written = 0;
                SharpZipBaseException? error = null;
                try { written = inflater.Inflate(output, 1, outputSize); }
                catch (SharpZipBaseException caught) { error = caught; }
                finally { Require(output[0] == 0xa6 && output[^1] == 0xa6, "출력 guard 손상"); }
                Counters(inflater, offered);
                Require(inflater.TotalIn >= beforeIn, "입력 카운터 감소");
                if (error is not null)
                {
                    Require(!fixture.Valid && !inflater.IsFinished, "정상 표본 거부 또는 실패 완료 상태");
                    Require(inflater.TotalOut >= beforeOut && inflater.TotalOut <= beforeOut + outputSize,
                        "예외 경로 출력 카운터 불일치");
                    if (fixture.ExpectedFailure?.Contains("OverSubscribed", StringComparison.Ordinal) == true ||
                        fixture.Id.StartsWith("extra.dynamic-reserved-", StringComparison.Ordinal) ||
                        fixture.Id.StartsWith("fixed.reserved-distance", StringComparison.Ordinal))
                    {
                        Require(error is StreamDecodingException, "명시적 형식 예외가 아님");
                    }
                    if (fixture.ExpectedFailure?.Contains("OverSubscribed", StringComparison.Ordinal) == true)
                    {
                        Require(inflater.TotalOut == 0, "과다 할당 트리에서 출력 발생");
                    }
                    if (fixture.Id.Contains("reserved", StringComparison.Ordinal))
                    {
                        Require(inflater.TotalOut <= 1, "예약 거리에서 추가 출력 발생");
                    }
                    return;
                }
                Require(written >= 0 && written <= outputSize && inflater.TotalOut == beforeOut + written,
                    "반환 길이와 TotalOut 불일치");
                if (fixture.Output is { } pattern)
                {
                    for (var index = 0; index < written; index++)
                    {
                        Require(returned + index < pattern.Length &&
                            output[index + 1] == (byte)pattern.Unit[(int)((returned + index) % pattern.Unit.Length)],
                            "독립 출력 패턴 불일치");
                    }
                }
                returned += written;
                Require(returned <= 4096 && inflater.TotalOut == returned, "출력 상한 또는 누적 불일치");
                hash.AppendData(output, 1, written);
                if (inflater.IsFinished)
                {
                    Require(fixture.Valid, "무효 표본 수용");
                    var expected = fixture.Output ?? throw new InvalidOperationException("정상 기대값 없음");
                    Require(returned == expected.Length && inflater.TotalIn == fixture.Raw.Length,
                        "완료 길이 또는 소비량 불일치");
                    Require(Convert.ToHexString(hash.GetHashAndReset()) == expected.Hash(), "출력 해시 불일치");
                    var remaining = inflater.RemainingInput;
                    Array.Fill(output, (byte)0xa6);
                    try
                    {
                        Require(inflater.Inflate(output, 1, outputSize) == 0 && inflater.IsFinished &&
                            inflater.TotalOut == returned && inflater.TotalIn == fixture.Raw.Length &&
                            inflater.RemainingInput == remaining, "완료 후 상태 변화");
                    }
                    finally { Require(output.All(value => value == 0xa6), "완료 후 출력 버퍼 변화"); }
                    return;
                }
                Require(!inflater.IsNeedingDictionary, "raw 시험의 사전 요청");
                if (written != 0) { continue; }
                if (inflater.IsNeedingInput)
                {
                    Require(offered < source.Length, "완료 전 실제 EOF");
                    var count = Math.Min(feed, source.Length - offered);
                    inflater.SetInput(source, offered, count);
                    offered += count;
                    Counters(inflater, offered);
                }
                else { Require(inflater.TotalIn != beforeIn, "입력 요청 없는 무진행"); }
            }
            throw new InvalidOperationException("호출 상한 초과");
        }
        finally { Require(source.AsSpan().SequenceEqual(original), "입력 배열 변화"); }
    }

    private static void Counters(Inflater inflater, int offered)
    {
        Require(inflater.TotalIn >= 0 && inflater.RemainingInput >= 0 &&
            inflater.TotalIn + inflater.RemainingInput == offered, "입력 보존 불일치");
    }

    private static void RejectTree(byte[] lengths)
    {
        try { _ = new InflaterHuffmanTree(lengths); }
        catch (StreamDecodingException) { return; }
        throw new InvalidOperationException("무효 트리 수용");
    }

    private static void Test(string id, Action action)
    {
        try { action(); passed++; }
        catch (Exception error)
        {
            failed++;
            Console.WriteLine(JsonSerializer.Serialize(new { Id = id, Error = error.GetType().FullName, error.Message }));
        }
    }

    private static void Require(bool condition, string message)
    {
        if (!condition) { throw new InvalidOperationException(message); }
    }
}

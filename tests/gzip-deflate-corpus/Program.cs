using System.Diagnostics;
using System.IO.Compression;
using System.Security.Cryptography;
using System.Text.Json;
using Zipelo.DeflateCorpus;

return Runner.MainEntry(args);

internal sealed record SpecRow(
    string Id,
    bool Valid,
    int RawLength,
    int EndBitExclusive,
    string RawSha256,
    int? OutputLength,
    string? OutputSha256,
    string? OutputCrc32,
    string? ExpectedFailure,
    int? FaultBitOffset,
    int? HistoryBeforeFault,
    int? MatchDistance);

internal static class Runner
{
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        PropertyNameCaseInsensitive = true
    };

    public static int MainEntry(string[] args)
    {
        if (args.Length > 1 || args.Length == 1 && args[0] is not "--emit" and not "--verify-bcl")
        {
            Console.Error.WriteLine("사용법: Zipelo.DeflateCorpus [--emit|--verify-bcl]");
            return 2;
        }

        try
        {
            var specPath = Path.Combine(AppContext.BaseDirectory, "expectations.json");
            var expected = JsonSerializer.Deserialize<SpecRow[]>(File.ReadAllText(specPath), JsonOptions)
                ?? throw new InvalidOperationException("기대값 목록이 없음.");
            ValidateCorpus(expected);

            if (args is ["--emit"])
            {
                foreach (var fixture in Corpus.Create())
                {
                    Console.WriteLine(JsonSerializer.Serialize(new
                    {
                        Spec = Describe(fixture),
                        RawHex = Convert.ToHexString(fixture.Raw),
                        fixture.Rationale,
                        fixture.Tokens,
                        FinalPaddingBits = fixture.Raw.Length * 8 - fixture.EndBitExclusive,
                        OutputPattern = fixture.Output,
                        InvalidWrapperTrailerOnly = fixture.InvalidWrapperPattern?.Hash()
                    }, JsonOptions));
                }
            }
            else if (args is ["--verify-bcl"])
            {
                return VerifyBclSuite();
            }
            else
            {
                Console.WriteLine(JsonSerializer.Serialize(new
                {
                    Mode = "spec",
                    Cases = expected.Length,
                    Valid = expected.Count(row => row.Valid),
                    Invalid = expected.Count(row => !row.Valid),
                    DecoderExecuted = false
                }, JsonOptions));
            }

            return 0;
        }
        catch (Exception exception)
        {
            Console.Error.WriteLine($"FAIL {exception.GetType().Name}: {exception.Message}");
            return 1;
        }
    }

    private static SpecRow Describe(Fixture fixture)
    {
        var digest = fixture.Output?.Hash();
        return new SpecRow(
            fixture.Id, fixture.Valid, fixture.Raw.Length, fixture.EndBitExclusive,
            Convert.ToHexString(SHA256.HashData(fixture.Raw)),
            digest?.Length, digest?.Sha256, digest?.Crc32,
            fixture.Valid ? null : "Deflate.InvalidBackreferenceHistory",
            fixture.FaultBitOffset, fixture.HistoryBeforeFault, fixture.MatchDistance);
    }

    private static void ValidateCorpus(SpecRow[] expected)
    {
        Require(expected.Length == 22, "고정 corpus는 22개여야 함.");
        Require(expected.Select(row => row.Id).Distinct().Count() == expected.Length, "기대값 ID가 중복됨.");
        var found = new HashSet<string>();
        foreach (var fixture in Corpus.Create())
        {
            Require(found.Add(fixture.Id), "표본 ID가 중복됨.");
            var pin = expected.Single(row => row.Id == fixture.Id);
            Require(Describe(fixture) == pin, $"고정 기대값 불일치: {fixture.Id}");
            Require(fixture.Raw.Length <= Corpus.MaxRawBytes, "raw 크기 상한 초과임.");
            Require(fixture.Output is null || fixture.Output.Length <= Corpus.MaxOutputBytes, "출력 상한 초과임.");
            Require(fixture.EndBitExclusive > 0 &&
                fixture.Raw.Length == (fixture.EndBitExclusive + 7) / 8, "raw 종료 위치가 맞지 않음.");

            var cursor = 0;
            foreach (var token in fixture.Tokens)
            {
                Require(token.StartBit == cursor && token.BitLength >= 0, "토큰 설명에 간격이나 중복이 있음.");
                cursor += token.BitLength;
            }
            Require(cursor == fixture.EndBitExclusive, "토큰 설명이 끝까지 도달하지 않음.");
            for (var bit = cursor; bit < fixture.Raw.Length * 8; bit++)
            {
                Require(((fixture.Raw[bit / 8] >> (bit % 8)) & 1) == 0, "표본 패딩은 0이어야 함.");
            }

            if (fixture.MatchDistance is int distance && fixture.HistoryBeforeFault is int history)
            {
                var validDistance = distance >= 1 && distance <= Math.Min(history, 32768);
                Require(validDistance == fixture.Valid, "명세상 거리 유효성이 기대값과 다름.");
            }
            Require(fixture.Valid || fixture.InvalidWrapperPattern is not null, "무효 표본의 trailer 대조 패턴이 없음.");

            if (fixture.Id == "history.before-first.invalid")
            {
                Require(Convert.ToHexString(fixture.Raw) == "030200", "기존 최소 재현 바이트가 달라짐.");
            }
        }
        Require(found.SetEquals(expected.Select(row => row.Id)), "기대값과 표본의 집합이 다름.");
    }

    private static int VerifyBclSuite()
    {
        var clock = Stopwatch.StartNew();
        var passed = 0;
        var failed = 0;
        foreach (var inputChunk in new[] { 1, 7, 65536 })
        {
            foreach (var outputChunk in new[] { 1, 258, 65536 })
            {
                foreach (var fixture in Corpus.Create())
                {
                    Check(fixture, inputChunk, outputChunk);
                }

                // 앞선 해제 성공 뒤 새 스트림의 이력이 비어 있음을 확인함.
                foreach (var id in new[]
                {
                    "window.prefix-65537",
                    "history.before-first.invalid",
                    "empty.fixed",
                    "overlap.xy.length5.distance2"
                })
                {
                    Check(Corpus.Create().Single(item => item.Id == id), inputChunk, outputChunk);
                }
            }
        }

        Console.WriteLine(JsonSerializer.Serialize(new
        {
            Mode = "bcl-observation",
            Runtime = Environment.Version.ToString(),
            Passed = passed,
            Failed = failed,
            BoundaryOrCompletionProved = false
        }, JsonOptions));
        return failed == 0 ? 0 : 1;

        void Check(Fixture fixture, int inputChunk, int outputChunk)
        {
            if (clock.Elapsed > TimeSpan.FromSeconds(30))
            {
                throw new TimeoutException("전체 관찰 예산 30초를 초과함.");
            }

            try
            {
                VerifyBcl(fixture, inputChunk, outputChunk, clock);
                passed++;
            }
            catch (Exception exception)
            {
                failed++;
                Console.Error.WriteLine(
                    $"FAIL {fixture.Id} input={inputChunk} output={outputChunk}: {exception.GetType().Name}: {exception.Message}");
            }
        }
    }

    private static void VerifyBcl(Fixture fixture, int inputChunk, int outputChunk, Stopwatch clock)
    {
        using var input = new FragmentedInput(fixture.Raw, inputChunk);
        using var decoder = new DeflateStream(input, CompressionMode.Decompress, leaveOpen: true);
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        var buffer = new byte[outputChunk];
        var produced = 0;
        var rejected = false;
        while (true)
        {
            if (clock.Elapsed > TimeSpan.FromSeconds(30))
            {
                throw new TimeoutException("전체 관찰 예산 30초를 초과함.");
            }

            int count;
            try
            {
                count = decoder.Read(buffer, 0, buffer.Length);
            }
            catch (InvalidDataException)
            {
                rejected = true;
                break;
            }

            if (count == 0)
            {
                break;
            }

            Require(count > 0 && count <= buffer.Length, "잘못된 Read 반환값임.");
            Require(produced + count <= Corpus.MaxOutputBytes, "관찰 출력 상한 초과임.");
            if (fixture.Output is Pattern pattern)
            {
                Require(produced + count <= pattern.Length, "정상 표본의 출력이 너무 긺.");
                for (var index = 0; index < count; index++)
                {
                    Require(buffer[index] == pattern.At(produced + index), "알려진 출력 패턴과 다름.");
                }
                hash.AppendData(buffer, 0, count);
            }
            produced += count;
        }

        if (!fixture.Valid)
        {
            Require(rejected, "잘못된 역참조를 InvalidDataException으로 거부하지 않음.");
            return;
        }

        Require(!rejected, "명세상 유효한 표본을 거부함.");
        var digest = fixture.Output!.Hash();
        Require(produced == digest.Length, "정상 출력 길이가 다름.");
        Require(Convert.ToHexString(hash.GetHashAndReset()) == digest.Sha256, "정상 출력 해시가 다름.");
    }

    private static void Require(bool condition, string message)
    {
        if (!condition)
        {
            throw new InvalidOperationException(message);
        }
    }
}

internal sealed class FragmentedInput(byte[] bytes, int maximumRead) : MemoryStream(bytes, writable: false)
{
    public override int Read(byte[] buffer, int offset, int count)
    {
        return base.Read(buffer, offset, Math.Min(count, maximumRead));
    }

    public override int Read(Span<byte> buffer)
    {
        return base.Read(buffer[..Math.Min(buffer.Length, maximumRead)]);
    }
}

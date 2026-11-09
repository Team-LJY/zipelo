using System.Security.Cryptography;

namespace Zipelo.DynamicDeflateCorpus;

internal sealed record Pattern(string Unit, int Length)
{
    public string Hash()
    {
        if (Unit.Length == 0 || Unit.Any(value => value > 127) || Length is < 0 or > 4096)
        {
            throw new InvalidOperationException("출력 패턴 범위를 벗어남.");
        }
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        var buffer = new byte[64];
        for (var offset = 0; offset < Length;)
        {
            var count = Math.Min(buffer.Length, Length - offset);
            for (var index = 0; index < count; index++)
            {
                buffer[index] = (byte)Unit[(offset + index) % Unit.Length];
            }
            hash.AppendData(buffer, 0, count);
            offset += count;
        }
        return Convert.ToHexString(hash.GetHashAndReset());
    }
}

internal sealed record Fixture(string Id, byte[] Raw, int EndBitExclusive, Token[] Tokens,
    TreeDescription[] Trees, Pattern? Output, string? ExpectedFailure, int? FaultBitOffset,
    string? Control, string Rationale)
{
    public bool Valid => Output is not null;
}

internal static class Corpus
{
    public const int Count = 18;
    public const int MaxRawBytes = 4096;
    public const int MaxOutputBytes = 4096;
    public const string LegacyLiteralRawSha256 = "309A625CB0AC4192F71A76BEA1291E944E5516F44102C4A7AB18830788C6AAFF";

    public static IEnumerable<Fixture> Create()
    {
        yield return Overlap(false);
        yield return StoredDynamic();
        yield return Literal(1);
        yield return Overlap(true);
        foreach (var distances in new[] { 30, 31, 32 }) { yield return Literal(distances); }
        yield return Alphabet("valid");
        yield return Alphabet("missing-eob");
        yield return Repeat16(false);
        yield return Repeat16(true);
        yield return Repeat18(false);
        yield return Repeat18(true);
        yield return Alphabet("overfull-literal");
        yield return Alphabet("overfull-meta");
        foreach (var distance in new[] { 0, 30, 31 }) { yield return FixedDistance(distance); }
    }

    private static Fixture Overlap(bool beforeFirst)
    {
        var writer = new FixtureWriter();
        var ll = Lengths(286, (65, 1), (256, 2), (285, 2));
        var dd = Lengths(1, (0, 1));
        var layout = writer.Dynamic(ll, dd, Meta012());
        var literal = new Codebook(ll);
        if (!beforeFirst) { writer.Huffman(literal, 65, "literal", "A"); }
        var matchStart = writer.Position;
        writer.Huffman(literal, 285, "length", "258; no extra bits");
        writer.Huffman(new Codebook(dd), 0, "distance", "1; no extra bits");
        writer.Huffman(literal, 256, "EOB", "end");
        return Finish(writer, beforeFirst ? "dynamic.before-first.invalid" : "dynamic.overlap.a258",
            beforeFirst ? null : new Pattern("A", 259),
            beforeFirst ? "Deflate.InvalidBackreferenceHistory" : null, beforeFirst ? matchStart : null,
            beforeFirst ? "dynamic.overlap.a258" : null,
            $"HLIT=29, HDIST=0, data starts at {layout.DataStart}. LL A=0/EOB=10/length258=11; distance1=0. " +
            (beforeFirst ? "P=0이므로 거리1 참조가 무효임. 트리는 정상 대조군과 동일함."
                : "P=1 뒤 길이258/거리1의 겹침 복사를 허용함. 단일 거리 심벌은 길이1임."));
    }

    private static Fixture StoredDynamic()
    {
        var writer = new FixtureWriter();
        writer.StoredXY();
        var ll = Lengths(260, (256, 1), (259, 1));
        var dd = Lengths(2, (1, 1));
        writer.Dynamic(ll, dd, Meta01());
        writer.Huffman(new Codebook(ll), 259, "length", "5; no extra bits");
        writer.Huffman(new Codebook(dd), 1, "distance", "2; no extra bits");
        writer.Huffman(new Codebook(ll), 256, "EOB", "end");
        return Finish(writer, "stored-dynamic.xy", new Pattern("XY", 7), null, null, null,
            "56비트의 non-final stored XY 뒤 dynamic 길이5/거리2를 사용함. EOB=0/length5=1, distance2=0. 블록 전환은 이력을 지우지 않음.");
    }

    private static Fixture Literal(int distanceCount)
    {
        var writer = new FixtureWriter();
        var ll = Lengths(257, (65, 1), (256, 1));
        var dd = distanceCount == 1 ? new int[1] : Lengths(distanceCount, (0, 1), (1, 1));
        writer.Dynamic(ll, dd, Meta01());
        writer.Huffman(new Codebook(ll), 65, "literal", "A");
        writer.Huffman(new Codebook(ll), 256, "EOB", "end");
        return Finish(writer, distanceCount == 1 ? "dynamic.literal-only.a" : $"dynamic.hdist-{distanceCount}.valid",
            new Pattern("A", 1), null, null, distanceCount > 30 ? "dynamic.hdist-30.valid" : null,
            distanceCount == 1 ? "PR16의 literal-only 표본을 동일 비트 구성으로 재작성함. 단일 거리 길이0은 참조가 없음을 뜻함."
                : "LL A=0/EOB=1; distance0=0/distance1=1, 나머지 길이0. 예약 심벌30/31은 존재하는 경우 길이0이며 사용하지 않음. RFC 기대는 성공임.");
    }

    private static Fixture Alphabet(string mode)
    {
        var writer = new FixtureWriter();
        var ll = mode switch
        {
            "missing-eob" => Lengths(257, (65, 1), (66, 2), (67, 2)),
            "overfull-literal" => Lengths(257, (65, 1), (66, 1), (256, 1)),
            _ => Lengths(257, (65, 1), (66, 2), (256, 2))
        };
        var declaredMeta = mode == "overfull-meta" ? Lengths(19, (0, 1), (1, 1), (2, 1)) : Meta012();
        var layout = writer.Dynamic(ll, new int[1], declaredMeta,
            suffixEncodingMeta: mode == "overfull-meta" ? Meta012() : null);
        if (mode is "overfull-literal" or "overfull-meta")
        {
            writer.OpaqueSuffix();
        }
        else
        {
            var book = new Codebook(ll);
            writer.Huffman(book, 65, "literal", "A");
            writer.Huffman(book, 66, "literal", "B");
            writer.Huffman(book, mode == "missing-eob" ? 67 : 256,
                mode == "missing-eob" ? "literal" : "EOB", mode == "missing-eob" ? "C; no EOB" : "end");
        }
        var failure = mode switch
        {
            "missing-eob" => "Deflate.MissingEndOfBlock",
            "overfull-literal" => "Deflate.OverSubscribedLiteralTree",
            "overfull-meta" => "Deflate.OverSubscribedCodeLengthTree",
            _ => null
        };
        int? fault = mode switch
        {
            "missing-eob" or "overfull-literal" => layout.LengthStarts[256],
            "overfull-meta" => layout.MetaStarts[17],
            _ => null
        };
        return Finish(writer, $"dynamic.alphabet.{mode}", mode == "valid" ? new Pattern("AB", 2) : null,
            failure, fault, mode == "valid" ? null : "dynamic.alphabet.valid",
            mode switch
            {
                "missing-eob" => "LL A=0/B=10/C=11은 완전하지만 EOB 길이가0임. 끝 비트는 명목상 작성 끝이며 정상 EOB가 아님.",
                "overfull-literal" => "LL 심벌65/66/256이 모두 길이1이므로 Kraft 합3/2로 과다 할당됨. canonical 코드는 정의하지 않음. 뒤5비트는 비교용 불투명 suffix임.",
                "overfull-meta" => "code-length 심벌0/1/2가 모두 길이1이므로 Kraft 합3/2로 과다 할당됨. 뒤 길이열은 정상 대조군의 meta 코드로만 작성한 불투명 suffix이며 손상 트리의 실제 해석을 주장하지 않음.",
                _ => "LL A=0/B=10/EOB=11은 완전함. 거리 참조 없이 AB를 출력함. meta0=0/1=10/2=11도 완전함."
            });
    }

    private static Fixture Repeat16(bool invalid)
    {
        var writer = new FixtureWriter();
        var ll = Lengths(257, (65, 1), (256, 1));
        var all = ll.Concat(new int[1]).ToArray();
        var sequence = new List<LengthCode>();
        if (invalid) { sequence.Add(new(16, 0)); sequence.Add(new(0)); }
        else { sequence.Add(new(0)); sequence.Add(new(16, 0)); }
        sequence.AddRange(all.Skip(4).Select(value => new LengthCode(value)));
        var layout = writer.Dynamic(ll, new int[1], Lengths(19, (0, 1), (1, 2), (16, 2)), sequence.ToArray());
        writer.Huffman(new Codebook(ll), 65, "literal", "A");
        writer.Huffman(new Codebook(ll), 256, "EOB", "end");
        return Finish(writer, invalid ? "dynamic.repeat16-first.invalid" : "dynamic.repeat16.valid",
            invalid ? null : new Pattern("A", 1), invalid ? "Deflate.RepeatWithoutPreviousLength" : null,
            invalid ? layout.LengthStarts[0] : null, invalid ? "dynamic.repeat16.valid" : null,
            invalid ? "첫 길이 토큰16에는 반복할 이전 길이가 없음. 선언 배열은 정상 대조군의 의도만 나타내며 이 잘못된 열의 복원 결과가 아님. 16과0의 순서만 교환함."
                : "길이0을 먼저 쓴 뒤 심벌16 extra=0으로0을3회 반복함. 반복16으로0을 복사하는 것은 유효함. 나머지 길이는 직접 작성함.");
    }

    private static Fixture Repeat18(bool invalid)
    {
        var writer = new FixtureWriter();
        var ll = Lengths(257, (65, 1), (256, 1));
        LengthCode[] sequence = [new(18, 54), new(1), new(18, 127), new(18, 41), new(1), invalid ? new(18, 0) : new(0)];
        var layout = writer.Dynamic(ll, new int[1], Lengths(19, (0, 1), (1, 2), (18, 2)), sequence);
        writer.Huffman(new Codebook(ll), 65, "literal", "A");
        writer.Huffman(new Codebook(ll), 256, "EOB", "end");
        return Finish(writer, invalid ? "dynamic.repeat18-overflow.invalid" : "dynamic.repeat18.valid",
            invalid ? null : new Pattern("A", 1), invalid ? "Deflate.CodeLengthRepeatOverflow" : null,
            invalid ? layout.LengthStarts[5] : null, invalid ? "dynamic.repeat18.valid" : null,
            invalid ? "65zero,1,138zero,52zero,1로257개를 채운 뒤 남은 거리 길이1칸에11zero를 요구하여10칸 초과함. 선언 배열은 정상 대조군의 의도이며 잘못된 열의 복원 결과가 아님."
                : "65zero,1,138zero,52zero,1,0은 정확히258개 길이이며 LL65/256만1임. 반복18의 최댓값138도 유효함.");
    }

    private static Fixture FixedDistance(int distanceSymbol)
    {
        var writer = new FixtureWriter();
        var (ll, dd) = writer.Fixed();
        writer.Huffman(ll, 65, "literal", "A");
        writer.Huffman(ll, 257, "length", "3; no extra bits");
        var fault = writer.Position;
        writer.Huffman(dd, distanceSymbol, "distance",
            distanceSymbol == 0 ? "1; no extra bits" : "reserved; no distance value defined");
        writer.Huffman(ll, 256, "EOB", "nominal end");
        return Finish(writer, distanceSymbol == 0 ? "fixed.distance0.valid" : $"fixed.reserved-distance{distanceSymbol}.invalid",
            distanceSymbol == 0 ? new Pattern("A", 4) : null,
            distanceSymbol == 0 ? null : "Deflate.ReservedDistanceSymbol", distanceSymbol == 0 ? null : fault,
            distanceSymbol == 0 ? null : "fixed.distance0.valid",
            "동일 fixed 트리에서 거리 심벌만 교체함. 정상 거리1은 AAAA를 출력하지만 예약 심벌30/31은 실제 사용 금지임. HDIST 선언 검사와 무관한 오류군임.");
    }

    private static int[] Meta01() => Lengths(19, (0, 1), (1, 1));
    private static int[] Meta012() => Lengths(19, (0, 1), (1, 2), (2, 2));
    private static int[] Lengths(int count, params (int Symbol, int Length)[] entries)
    {
        var result = new int[count];
        foreach (var (symbol, length) in entries) { result[symbol] = length; }
        return result;
    }

    private static Fixture Finish(FixtureWriter writer, string id, Pattern? output, string? failure,
        int? fault, string? control, string rationale) => new(id, writer.Bytes(), writer.Position,
            writer.Tokens.ToArray(), writer.Trees.ToArray(), output, failure, fault, control, rationale);
}

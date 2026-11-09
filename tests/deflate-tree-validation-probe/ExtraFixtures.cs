namespace Zipelo.DynamicDeflateCorpus;

internal static class ExtraFixtures
{
    public static IEnumerable<Fixture> Create()
    {
        yield return Literal(true);
        yield return Literal(false);
        yield return Reserved(30);
        yield return Reserved(31);
        yield return OverfullDistance();
    }

    private static Fixture Literal(bool maximum)
    {
        var writer = new FixtureWriter();
        var ll = Lengths(maximum ? 286 : 257, (256, 1));
        if (maximum) { ll[65] = 1; }
        var dd = maximum ? Lengths(32, (0, 1), (1, 1)) : new int[1];
        writer.Dynamic(ll, dd, Lengths(19, (0, 1), (1, 1)));
        var book = new Codebook(ll);
        if (maximum) { writer.Huffman(book, 65, "literal", "A"); }
        writer.Huffman(book, 256, "EOB", "end");
        return Finish(writer, maximum ? "extra.maximum-318.valid" : "extra.eob-only.valid",
            new Pattern("A", maximum ? 1 : 0), null,
            maximum ? "HLIT=29, HDIST=31. 318개 길이를 모두 읽으며 마지막 예약 꼬리 길이는 0임. 출력 A임."
                : "LL은 EOB만 길이1이며 거리 길이는0임. EOB 코드0 하나로 빈 출력을 완료해야 함.");
    }

    private static Fixture Reserved(int reserved)
    {
        var writer = new FixtureWriter();
        var ll = Lengths(258, (65, 1), (256, 2), (257, 2));
        var dd = Lengths(reserved + 1, (0, 1), (reserved, 1));
        writer.Dynamic(ll, dd, Lengths(19, (0, 1), (1, 2), (2, 2)));
        var literal = new Codebook(ll);
        writer.Huffman(literal, 65, "literal", "A");
        writer.Huffman(literal, 257, "length", "3; no extra bits");
        writer.Huffman(new Codebook(dd), reserved, "distance", "reserved; no distance value");
        writer.Huffman(literal, 256, "EOB", "unreachable suffix");
        return Finish(writer, $"extra.dynamic-reserved-{reserved}.invalid", null,
            "Deflate.ReservedDistance", "선언 자체를 거부하지 않고 본문의 예약 거리 코드1 사용을 거부해야 함. 선행 A 전달량은 0 또는1임.");
    }

    private static Fixture OverfullDistance()
    {
        var writer = new FixtureWriter();
        writer.Dynamic(Lengths(257, (65, 1), (256, 1)), Lengths(3, (0, 1), (1, 1), (2, 1)),
            Lengths(19, (0, 1), (1, 1)));
        writer.OpaqueSuffix();
        return Finish(writer, "extra.overfull-distance.invalid", null,
            "Deflate.OverSubscribedDistanceTree", "거리 심벌0/1/2의 길이가 모두1이므로 Kraft 합3/2임. 본문 전에 거부하며 suffix에는 의미를 부여하지 않음.");
    }

    private static Fixture Finish(FixtureWriter writer, string id, Pattern? output, string? failure, string reason)
        => new(id, writer.Bytes(), writer.Position, writer.Tokens.ToArray(), writer.Trees.ToArray(),
            output, failure, null, null, reason);

    private static int[] Lengths(int count, params (int Symbol, int Bits)[] values)
    {
        var result = new int[count];
        foreach (var (symbol, bits) in values) { result[symbol] = bits; }
        return result;
    }
}

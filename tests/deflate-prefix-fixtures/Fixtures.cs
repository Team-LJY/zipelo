namespace Zipelo.PrefixFixtures;

public static class Fixtures
{
    public static EmittedFixture[] Build()
    {
        var specs = Specification.Load();
        Specification.Require(specs.Length == 7 && specs.Count(spec => spec.Valid) == 4, "표본 수가 다름.");
        return specs.Select(BuildOne).ToArray();
    }

    private static EmittedFixture BuildOne(FixtureSpec spec)
    {
        var writer = new FixtureWriter();
        var literal = new int[spec.Id.Contains("distance") ? 258 : 257];
        var distance = new int[1];
        var meta = new int[19];
        if (spec.Id.Contains("eob-only"))
        {
            literal[256] = 1;
            meta[0] = meta[1] = 1;
        }
        else if (spec.Id.Contains("subtree"))
        {
            for (var symbol = 0; symbol <= 7; symbol++) { literal[symbol] = symbol + 2; }
            literal[65] = 1;
            literal[66] = 10;
            literal[67] = literal[256] = 11;
            for (var symbol = 0; symbol <= 11; symbol++) { meta[symbol] = symbol <= 3 ? 3 : 4; }
        }
        else
        {
            literal[65] = 1;
            literal[256] = literal[257] = 2;
            meta[0] = 1;
            meta[1] = meta[2] = 2;
            if (spec.Id.Contains("single-distance")) { distance[0] = 1; }
        }
        writer.Header(literal, distance, meta);
        Specification.Require(writer.Position == spec.DataStartBit, "독립 헤더 길이와 다름.");
        switch (spec.Id)
        {
            case "valid.eob-only":
                writer.Code(literal, 256, "EOB");
                break;
            case "invalid.eob-only-unused":
                writer.Raw("1", "impossible-prefix", "EOB-only의 미사용 분기");
                break;
            case "valid.single-distance":
            case "invalid.single-distance-unused":
                for (var count = 0; count < 8; count++) { writer.Code(literal, 65, "literal A"); }
                writer.Code(literal, 257, "length 3");
                writer.Raw(spec.Valid ? "0" : "1", spec.Valid ? "distance-code" : "impossible-prefix",
                    spec.Valid ? "distance 1" : "단일 거리 코드의 미사용 분기");
                writer.Code(literal, 256, spec.Valid ? "EOB" : "오류 뒤 명목상 EOB");
                break;
            case "valid.empty-distance-unused":
                writer.Code(literal, 65, "literal A");
                writer.Code(literal, 256, "EOB");
                break;
            case "invalid.empty-distance-reference":
                writer.Code(literal, 65, "literal A");
                writer.Code(literal, 257, "length 3 with no distance alphabet");
                writer.Raw("10", "opaque-suffix", "거리 코드 없이 놓은 명목상 EOB 비트");
                break;
            case "valid.subtree-split":
                writer.Code(literal, 66, "literal B, 10 bits");
                for (var count = 0; count < 7; count++) { writer.Code(literal, 65, "literal A"); }
                writer.Code(literal, 67, "literal C, 11 bits");
                writer.Code(literal, 256, "EOB, 11 bits");
                break;
            default:
                throw new InvalidDataException("알 수 없는 표본임.");
        }
        Specification.Require(writer.Position == spec.EndBitExclusive && writer.Bytes.Length == spec.RawLength,
            "독립 끝 비트 또는 raw 길이와 다름.");
        Specification.Require(writer.Tokens.Sum(token => token.BitLength) == spec.EndBitExclusive,
            "토큰 길이가 맞지 않음.");
        if (!spec.Valid)
        {
            var fault = writer.Tokens.Single(token => token.Kind is "impossible-prefix" or "opaque-suffix");
            Specification.Require(fault.StartBit == spec.FaultStartBit
                && spec.DecisiveEndBit == fault.StartBit + (fault.Kind == "impossible-prefix" ? 1 : 0),
                "독립 오류 위치와 다름.");
        }
        return new EmittedFixture(spec, Convert.ToHexString(writer.Bytes), Specification.Hash(writer.Bytes),
            spec.Valid ? Specification.Hash(Specification.Bytes(spec.OutputText)) : null, writer.Trees.ToArray(), writer.Tokens.ToArray());
    }
}

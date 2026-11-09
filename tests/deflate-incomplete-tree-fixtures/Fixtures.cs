using System.Text.Json;

namespace Zipelo.IncompleteFixtures;

public static class Fixtures
{
    public static EmittedFixture[] Build()
    {
        var specs = Specification.Load();
        Specification.Require(specs.Length == 14 && specs.Count(spec => spec.Valid) == 6, "표본 수가 다름.");
        var inherited = Zipelo.PrefixFixtures.Fixtures.Build().ToDictionary(item => item.Spec.Id);
        var fixtures = new List<EmittedFixture>();
        foreach (var spec in specs)
        {
            if (inherited.TryGetValue(spec.Id, out var original))
            {
                // 고정 작성기 결과를 그대로 옮겨 raw와 token 및 원래 기대값을 보존함.
                var json = JsonSerializer.Serialize(original, Zipelo.PrefixFixtures.Specification.Json);
                var copy = JsonSerializer.Deserialize<EmittedFixture>(json, Specification.Json)
                    ?? throw new InvalidDataException("기존 표본을 읽지 못함.");
                Specification.Require(JsonSerializer.Serialize(spec, Specification.Json)
                    == JsonSerializer.Serialize(copy.Spec, Specification.Json), "기존 기대값이 바뀜.");
                fixtures.Add(copy);
            }
            else
            {
                fixtures.Add(BuildNew(spec));
            }
        }
        Specification.Require(fixtures.Count(item => inherited.ContainsKey(item.Spec.Id)) == 5,
            "기존 표본 재사용 수가 다름.");
        return fixtures.ToArray();
    }

    private static EmittedFixture BuildNew(FixtureSpec spec)
    {
        var pair = spec.Id.StartsWith("invalid.dists-pair-", StringComparison.Ordinal);
        var literal = new int[pair ? 258 : 257];
        var distance = new int[spec.Id switch
        {
            "valid.hdist31" => 31,
            "valid.hdist32" => 32,
            _ => pair ? 2 : 1
        }];
        var meta = new int[19];
        literal[256] = 1;
        switch (spec.Id)
        {
            case "invalid.lens-eob-2":
            case "invalid.lens-eob-10":
                var length = spec.Id.EndsWith("-10", StringComparison.Ordinal) ? 10 : 2;
                literal[256] = length;
                meta[0] = meta[length] = 1;
                break;
            case "invalid.dists-pair-used":
            case "invalid.dists-pair-unused":
                literal[65] = 1;
                literal[256] = literal[257] = 2;
                distance[0] = distance[1] = 2;
                meta[0] = 1;
                meta[1] = meta[2] = 2;
                break;
            case "invalid.dists-singleton-2-unused":
                distance[0] = 2;
                meta[0] = 1;
                meta[1] = meta[2] = 2;
                break;
            case "invalid.codes-incomplete":
                meta[0] = meta[1] = 2;
                break;
            case "invalid.codes-empty":
                break;
            case "valid.hdist31":
            case "valid.hdist32":
                distance[0] = 1;
                meta[0] = meta[1] = 1;
                break;
            default:
                throw new InvalidDataException("알 수 없는 표본임.");
        }

        var writer = new FixtureWriter();
        var emptyCodes = spec.Id == "invalid.codes-empty";
        writer.Header(literal, distance, meta, !emptyCodes);
        Specification.Require(writer.Position == spec.DataStartBit, "독립 선언 길이와 다름.");
        if (!emptyCodes)
        {
            if (spec.Id == "invalid.dists-pair-used")
            {
                writer.Code(literal, 65, "literal A");
                writer.Code(literal, 257, "length 3");
                writer.Code(distance, 0, "명목상 distance 1");
            }
            writer.Code(literal, 256, spec.Valid ? "EOB" : "무효 선언 뒤 명목상 EOB");
        }
        Specification.Require(writer.Position == spec.EndBitExclusive && writer.Bytes.Length == spec.RawLength,
            "독립 끝 비트 또는 raw 길이와 다름.");
        var expectedSuffix = spec.Id switch
        {
            "invalid.lens-eob-2" => new string('0', 256) + "10" + "00",
            "invalid.lens-eob-10" => new string('0', 256) + "10" + new string('0', 10),
            "invalid.dists-pair-used" => new string('0', 65) + "10" + new string('0', 190) + "11111111" + "0110010",
            "invalid.dists-pair-unused" => new string('0', 65) + "10" + new string('0', 190) + "11111111" + "10",
            "invalid.dists-singleton-2-unused" => new string('0', 256) + "10110",
            "invalid.codes-incomplete" => new string('0', 512) + "01000",
            "invalid.codes-empty" => "",
            "valid.hdist31" => new string('0', 256) + "11" + new string('0', 31),
            "valid.hdist32" => new string('0', 256) + "11" + new string('0', 32),
            _ => throw new InvalidDataException("독립 비트 기대가 없음.")
        };
        Specification.Require(string.Concat(writer.Tokens.Select(token => token.Bits))[71..] == expectedSuffix,
            "독립 코드 길이열 또는 본문 비트와 다름.");
        Specification.Require(writer.Tokens.Sum(token => token.BitLength) == spec.EndBitExclusive,
            "토큰 길이가 맞지 않음.");
        return new EmittedFixture(spec, Convert.ToHexString(writer.Bytes), Specification.Hash(writer.Bytes),
            spec.Valid ? Specification.Hash(Specification.Bytes(spec.OutputText)) : null,
            writer.Trees.ToArray(), writer.Tokens.ToArray());
    }
}

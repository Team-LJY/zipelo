using System.Text.Json;
using Zipelo.PrefixFixtures;

if (args.Length != 6 || args[0] != "--assembly" || args[2] != "--sha256" || args[4] != "--corpus"
    || args[3].Length != 64 || args[3].Any(value => !Uri.IsHexDigit(value)))
{
    Console.Error.WriteLine("사용법: Zipelo.PrefixProbe --assembly <DLL> --sha256 <SHA256> --corpus <JSONL>");
    return 2;
}
try
{
    var expected = Specification.Load().ToDictionary(spec => spec.Id);
    var lines = File.ReadAllLines(args[5]);
    Specification.Require(lines.Length == 7, "emit은 정확히 7행이어야 함.");
    var fixtures = lines.Select(line => JsonSerializer.Deserialize<EmittedFixture>(line, Specification.Json)
        ?? throw new InvalidDataException("emit을 읽지 못함.")).ToArray();
    Specification.Require(fixtures.Select(item => item.Spec.Id).Distinct().Count() == 7, "표본 ID가 중복됨.");
    foreach (var fixture in fixtures)
    {
        Specification.Require(expected.TryGetValue(fixture.Spec.Id, out var spec)
            && JsonSerializer.Serialize(spec, Specification.Json) == JsonSerializer.Serialize(fixture.Spec, Specification.Json),
            "독립 고정 기대값과 emit 명세가 다름.");
        var raw = Convert.FromHexString(fixture.RawHex);
        Specification.Require(raw.Length == fixture.Spec.RawLength && Specification.Hash(raw) == fixture.RawSha256,
            "emit raw 길이 또는 해시가 다름.");
        Specification.Require(!fixture.Spec.Valid || fixture.ExpectedOutputSha256
            == Specification.Hash(Specification.Bytes(fixture.Spec.OutputText)), "독립 출력 해시가 다름.");
    }
    var factory = new DecoderFactory(args[1], args[3]);
    Console.WriteLine(JsonSerializer.Serialize(new { kind = "decoder", factory.Identity, factory.Sha256 }, Specification.Json));
    var passed = 0;
    var total = 0;
    foreach (var fixture in fixtures)
    {
        var trials = new List<Trial>();
        foreach (var input in new[] { 1, 7, int.MaxValue })
        foreach (var output in new[] { 1, 257 })
        foreach (var tail in new[] { 0, 1 })
        {
            trials.Add(new Trial($"matrix.in{input}.out{output}.tail{tail}", input, output, tail, false));
        }
        if (fixture.Spec.Checkpoints.Length > 0)
        {
            foreach (var output in new[] { 1, 257 })
            foreach (var tail in new[] { 0, 1 })
            {
                trials.Add(new Trial($"gates.out{output}.tail{tail}", int.MaxValue, output, tail, true));
            }
        }
        var cut = fixture.Spec.Id switch
        {
            "valid.eob-only" => 41,
            "valid.single-distance" => 43,
            "valid.subtree-split" => 110,
            _ => 0
        };
        if (cut > 0)
        {
            foreach (var output in new[] { 1, 257 })
            {
                trials.Add(new Trial($"true-eof.{cut}.out{output}", int.MaxValue, output, 0, false, cut));
            }
        }
        foreach (var trial in trials)
        {
            var observation = new Driver(factory).Run(fixture, trial);
            total++;
            if (observation.Passed) { passed++; }
            Console.WriteLine(JsonSerializer.Serialize(observation, Specification.Json));
        }
    }
    Console.WriteLine(JsonSerializer.Serialize(new { kind = "summary", total, passed, failed = total - passed }, Specification.Json));
    return passed == total ? 0 : 1;
}
catch (Exception error)
{
    Console.Error.WriteLine(error.GetType().Name + ": " + error.Message);
    return 1;
}

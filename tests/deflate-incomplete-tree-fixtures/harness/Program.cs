using System.Text.Json;
using Zipelo.IncompleteFixtures;

if (args.Length == 1 && args[0] == "--self-test")
{
    return DriverRegression.Run();
}

if ((args.Length != 6 && args.Length != 7) || args[0] != "--assembly" || args[2] != "--sha256"
    || args[4] != "--corpus" || !Path.IsPathFullyQualified(args[1])
    || args[3].Length != 64 || args[3].Any(value => !Uri.IsHexDigit(value))
    || (args.Length == 7 && args[6] != "--observe-empty-codes"))
{
    Console.Error.WriteLine("사용법: Zipelo.IncompleteProbe --assembly <절대 DLL 경로> --sha256 <SHA256> --corpus <JSONL> [--observe-empty-codes] 또는 --self-test");
    return 2;
}
try
{
    var diagnostic = args.Length == 7;
    var expected = Specification.Load().ToDictionary(spec => spec.Id);
    Specification.Require(expected.Count == 14, "고정 명세 수가 다름.");
    Specification.Require(new FileInfo(args[5]).Length <= 2 * 1024 * 1024, "emit 크기 상한을 넘음.");
    var lines = File.ReadAllLines(args[5]);
    Specification.Require(lines.Length == 14, "emit은 정확히 14행이어야 함.");
    var fixtures = lines.Select(line => JsonSerializer.Deserialize<EmittedFixture>(line, Specification.Json)
        ?? throw new InvalidDataException("emit을 읽지 못함.")).ToArray();
    Specification.Require(fixtures.Select(item => item.Spec.Id).Distinct().Count() == 14, "표본 ID가 중복됨.");
    foreach (var fixture in fixtures)
    {
        Specification.Require(expected.TryGetValue(fixture.Spec.Id, out var spec)
            && JsonSerializer.Serialize(spec, Specification.Json) == JsonSerializer.Serialize(fixture.Spec, Specification.Json),
            "독립 고정 기대값과 emit 명세가 다름.");
        var raw = Convert.FromHexString(fixture.RawHex);
        Specification.Require(raw.Length == fixture.Spec.RawLength && Specification.Hash(raw) == fixture.RawSha256,
            "emit raw 길이 또는 해시가 다름.");
        Specification.Require(fixture.Spec.Valid
            ? fixture.ExpectedOutputSha256 == Specification.Hash(Specification.Bytes(fixture.Spec.OutputText))
            : fixture.ExpectedOutputSha256 is null, "독립 출력 해시가 다름.");
        var position = 0;
        foreach (var token in fixture.Tokens)
        {
            Specification.Require(token.StartBit == position && token.BitLength == token.Bits.Length,
                "token 위치 또는 길이가 다름.");
            foreach (var bit in token.Bits)
            {
                Specification.Require(bit is '0' or '1' && position < fixture.Spec.EndBitExclusive,
                    "token 비트 범위를 벗어남.");
                Specification.Require(((raw[position / 8] >> (position % 8)) & 1) == bit - '0',
                    "token과 raw 비트가 다름.");
                position++;
            }
        }
        Specification.Require(position == fixture.Spec.EndBitExclusive, "token 끝 위치가 다름.");
        for (; position < raw.Length * 8; position++)
        {
            Specification.Require(((raw[position / 8] >> (position % 8)) & 1) == 0, "패딩이0이 아님.");
        }
    }
    var factory = new DecoderFactory(args[1], args[3]);
    Console.WriteLine(JsonSerializer.Serialize(new { kind = "decoder", factory.Identity, factory.Sha256 }, Specification.Json));
    if (diagnostic)
    {
        // 꼬리 관찰은 선언 경계 수용 시험에 합산하지 않음.
        var fixture = fixtures.Single(item => item.Spec.Id == "invalid.codes-empty");
        var observations = 0;
        var unhealthy = 0;
        foreach (var input in new[] { 1, 7, int.MaxValue })
        foreach (var output in new[] { 1, 257 })
        {
            var trial = new Trial($"delayed.in{input}.out{output}.tail33", input, output, 33, false);
            var observation = new Driver(factory).Run(fixture, trial);
            observations++;
            if (!observation.GuardOk || !observation.InputOk || observation.Violations.Length > 0
                || observation.Outcome is "Budget" or "Timeout" or "NoProgress" or "NoProgressLimit"
                    or "ArrayError" or "HarnessInvariant" or "HarnessException" or "UnexpectedException"
                    or "UnexpectedDictionary")
            {
                unhealthy++;
            }
            Console.WriteLine(JsonSerializer.Serialize(new
            {
                kind = "delayed-observation", acceptanceTrial = false,
                strictContractSatisfied = observation.Passed, observation.Outcome, observation.ExceptionType,
                observation.Output, observation.ReturnedOutput, observation.OutputSha256, observation.Consumed,
                observation.Remaining, observation.Offered, observation.Finished, observation.FirstMismatch,
                observation.GuardOk, observation.InputOk, observation.Violations, observation.Calls, trial.Name
            }, Specification.Json));
        }
        Console.WriteLine(JsonSerializer.Serialize(new
        {
            kind = "diagnostic-summary", observations, acceptanceTrials = 0, unhealthy,
            meaning = "exit0은 관찰 수집만 뜻하며 EOF 또는 거짓 완료를 형식화된 거부로 세지 않음."
        }, Specification.Json));
        return observations == 6 && unhealthy == 0 ? 0 : 1;
    }

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
    Specification.Require(total == 226, "decoder 조건 수가 다름.");
    Console.WriteLine(JsonSerializer.Serialize(new { kind = "summary", total, passed, failed = total - passed }, Specification.Json));
    return passed == total ? 0 : 1;
}
catch (Exception error)
{
    Console.Error.WriteLine(error.GetType().Name + ": " + error.Message);
    return 1;
}

using System.Text.Json;

namespace Zipelo.PrefixFixtures;

internal static class DriverRegression
{
    private sealed class FakeDeflateException : Exception { }

    private sealed class ScriptDecoder : IDecoder
    {
        public long TotalIn { get; set; }
        public long TotalOut { get; set; }
        public int Remaining { get; private set; }
        public bool Finished { get; set; }
        public bool NeedInput => Remaining == 0;
        public bool NeedDictionary => false;
        public int Offered { get; private set; }
        public List<int> FeedEnds { get; } = new();
        public Func<byte[], int, int, int> Inflate { get; set; } = (_, _, _) => 0;
        public Action<byte[], int, int> SetInput => (_, _, count) =>
        {
            Specification.Require(Remaining == 0, "미소비 입력 위에 공급함.");
            Remaining += count;
            Offered += count;
            FeedEnds.Add(Offered);
        };

        public void Consume()
        {
            TotalIn += Remaining;
            Remaining = 0;
        }

        public int Write(byte[] buffer, int offset, int count)
        {
            buffer.AsSpan(offset, count).Fill((byte)'A');
            TotalOut += count;
            return count;
        }
    }

    public static int Run()
    {
        var total = 0;
        var passed = 0;
        void Check(string name, Action test)
        {
            total++;
            try
            {
                test();
                passed++;
                Console.WriteLine(JsonSerializer.Serialize(new { kind = "driver-regression", name, passed = true }, Specification.Json));
            }
            catch (Exception error)
            {
                Console.WriteLine(JsonSerializer.Serialize(new
                {
                    kind = "driver-regression", name, passed = false,
                    error = error.GetType().Name, error.Message
                }, Specification.Json));
            }
        }

        foreach (var mode in new[]
        {
            "timely-complete", "timely-reject", "late-complete", "late-reject",
            "late-stability-return", "late-stability-throw", "late-guard-throw", "expired-before-call"
        })
        {
            Check(mode, () => Timing(mode));
        }
        foreach (var id in new[] { "invalid.eob-only-unused", "invalid.single-distance-unused" })
        foreach (var output in new[] { 1, 257 })
        foreach (var reject in new[] { true, false })
        {
            Check($"gate.{id}.out{output}.reject{reject}", () => Gate(id, output, reject));
        }
        Console.WriteLine(JsonSerializer.Serialize(new
        {
            kind = "driver-regression-summary", total, passed, failed = total - passed
        }, Specification.Json));
        return total == 16 && passed == total ? 0 : 1;
    }

    private static void Timing(string mode)
    {
        var preExpired = mode == "expired-before-call";
        var reject = mode is "timely-reject" or "late-reject" or "late-guard-throw";
        var badGuard = mode == "late-guard-throw";
        var elapsed = preExpired ? TimeSpan.FromSeconds(6) : TimeSpan.Zero;
        var decoder = new ScriptDecoder();
        decoder.Inflate = (buffer, offset, count) =>
        {
            if (decoder.Finished)
            {
                if (mode.StartsWith("late-stability", StringComparison.Ordinal))
                {
                    elapsed = TimeSpan.FromSeconds(6);
                }
                if (mode == "late-stability-throw") { throw new FakeDeflateException(); }
                return 0;
            }
            if (decoder.Remaining == 0) { return 0; }
            decoder.Consume();
            var written = decoder.Write(buffer, offset, Math.Min(1, count));
            if (mode is "late-complete" or "late-reject" or "late-guard-throw")
            {
                elapsed = TimeSpan.FromSeconds(6);
            }
            if (badGuard) { buffer[offset - 1] = 0; }
            if (reject) { throw new FakeDeflateException(); }
            decoder.Finished = true;
            return written;
        };
        var spec = new FixtureSpec("driver-control." + mode, !reject, 0, 8, 1,
            reject ? "" : "A", null, null, 1, 1, "A", "실행기 제어 흐름 대조임.", Array.Empty<Checkpoint>());
        var observation = new Driver(() => decoder, error => error is FakeDeflateException, () => elapsed)
            .Run(Control(spec), new Trial(mode, 1, 1, 0, false));
        var timely = mode.StartsWith("timely-", StringComparison.Ordinal);
        var expectedOutcome = timely ? reject ? "Deflate" : "Complete" : "Timeout";
        Specification.Require(observation.Outcome == expectedOutcome && observation.Passed == timely,
            "시간 제한 뒤 성공 또는 거부가 통과함.");
        Specification.Require(observation.Output == (preExpired ? 0 : 1)
            && observation.OutputSha256 == Specification.Hash(Specification.Bytes(preExpired ? "" : "A")),
            "finally 출력 수집이 누락됨.");
        Specification.Require(observation.ReturnedOutput == (preExpired || reject ? 0 : 1),
            "예외 전 출력량과 반환량을 혼동함.");
        Specification.Require(observation.GuardOk == !badGuard && observation.InputOk
            && observation.Violations.SequenceEqual(badGuard ? new[] { "Guard" } : Array.Empty<string>()),
            "시간 초과 시 guard 또는 입력 검사가 누락됨.");
        var expectedCalls = preExpired ? 0
            : reject || mode == "late-complete" ? 2 : 3;
        Specification.Require(observation.Calls == expectedCalls && decoder.Offered == (preExpired ? 0 : 1),
            "만료된 호출 또는 stability 호출 횟수가 다름.");
        if (reject || mode == "late-stability-throw")
        {
            Specification.Require(observation.ExceptionType == typeof(FakeDeflateException).FullName,
                "시간 초과 시 원래 예외 타입을 잃음.");
        }
    }

    private static void Gate(string id, int output, bool reject)
    {
        var spec = Specification.Load().Single(item => item.Id == id);
        var boundary = id == "invalid.eob-only-unused" ? 42 : 44;
        var expectedOutput = boundary == 42 ? 0 : 8;
        Specification.Require(spec.Checkpoints.Length == 2
            && spec.Checkpoints[0] == new Checkpoint(boundary - 1, "Wait", expectedOutput, boundary - 1, 0)
            && spec.Checkpoints[1] == new Checkpoint(boundary, "Reject", expectedOutput, boundary, 0),
            "새 Reject 경계 또는 기존 Wait 기대가 다름.");
        var decoder = new ScriptDecoder();
        decoder.Inflate = (buffer, offset, count) =>
        {
            decoder.Consume();
            if (decoder.TotalIn >= boundary && reject) { throw new FakeDeflateException(); }
            var pending = decoder.TotalIn >= boundary - 1 ? expectedOutput - (int)decoder.TotalOut : 0;
            return decoder.Write(buffer, offset, Math.Min(count, pending));
        };
        var observation = new Driver(() => decoder, error => error is FakeDeflateException, () => TimeSpan.Zero)
            .Run(Control(spec), new Trial("gate-control", int.MaxValue, output, 1, true));
        Specification.Require(observation.Passed == reject
            && observation.Outcome == (reject ? "Deflate" : "MissingImmediateRejection"),
            "Reject 경계의 오류 또는 추가 입력 대기를 잘못 판정함.");
        Specification.Require(decoder.FeedEnds.SequenceEqual(new[] { boundary - 1, boundary })
            && observation.Offered == boundary && observation.Consumed == boundary && observation.Remaining == 0,
            "Reject 판정 전에 tail 바이트를 공급함.");
        Specification.Require(observation.Checkpoints.SequenceEqual(new[]
        {
            $"wait:{boundary - 1}", $"{(reject ? "reject" : "reject-missed")}:{boundary}"
        }), "경계 방문 결과가 다름.");
        Specification.Require(observation.Output == expectedOutput && observation.GuardOk && observation.InputOk
            && observation.FirstMismatch is null && observation.Violations.Length == 0 && !observation.Finished,
            "경계 전 출력 또는 guard 계약이 다름.");
    }

    private static EmittedFixture Control(FixtureSpec spec)
    {
        // 문법 검증용 raw 표본이 아닌 실행기 제어 입력임.
        var raw = new byte[spec.RawLength];
        return new EmittedFixture(spec, Convert.ToHexString(raw), Specification.Hash(raw),
            spec.Valid ? Specification.Hash(Specification.Bytes(spec.OutputText)) : null,
            Array.Empty<TreeSpec>(), Array.Empty<BitToken>());
    }
}

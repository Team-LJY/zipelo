using System.Buffers.Binary;
using System.Security.Cryptography;

internal static class Program
{
    private static int passed;
    private static int failed;

    public static int Main(string[] args)
    {
        if (args.Length != 0) { return 2; }
        Run("binding", () =>
        {
            Check(typeof(Inflater).Assembly.GetName().Name == "Zipelo.Experimental.SharpZipLibHistoryProbe");
            Check(typeof(Inflater).Assembly == typeof(SharpZipBaseException).Assembly);
        });
        Run("fixture.contract", VerifyFixtures);
        var xy = Fixtures.DictionaryXy();
        var window = Fixtures.DictionaryWindow();
        (Sample Sample, Envelope Envelope)[] valid =
        [
            (Fixtures.EmptyFixed(), Envelope.Raw),
            (Fixtures.Stored(0), Envelope.Raw),
            (Fixtures.Overlap(), Envelope.Raw),
            (Fixtures.Stored(257), Envelope.Raw),
            (Fixtures.Stored(258), Envelope.Raw),
            (Fixtures.Stored(259), Envelope.Raw),
            (Fixtures.StoredFixed(), Envelope.Raw),
            (Fixtures.Overlap(), Envelope.Zlib),
            (xy, Envelope.Zlib),
            (window, Envelope.Zlib),
            (Fixtures.Overlap(), Envelope.GzipBody)
        ];
        foreach (var item in valid)
        {
            foreach (var feed in new[] { 1, 7, 65536 })
            {
                foreach (var size in new[] { 1, 257, 258, 259 })
                {
                    Run($"valid.{item.Sample.Name}.{item.Envelope}.{feed}.{size}", () =>
                    {
                        var driver = new StateDriver(new Inflater(item.Envelope != Envelope.Zlib));
                        driver.Run(item.Sample, item.Envelope, feed, size);
                        Check(driver.Completed != null);
                    });
                }
            }
        }
        foreach (var sample in new[] { xy, window })
        {
            foreach (var feed in new[] { 1, 7, 65536 })
            {
                foreach (var size in new[] { 1, 257, 258, 259 })
                {
                    foreach (var envelope in new[] { Envelope.Raw, Envelope.GzipBody })
                    {
                        Run($"no-dictionary.{sample.Name}.{envelope}.{feed}.{size}", () =>
                            Reject(sample, envelope, feed, size, Failure.Decode));
                    }
                }
                Run($"dictionary.wait.{sample.Name}.{feed}", () =>
                {
                    var inflater = new Inflater();
                    var driver = new StateDriver(inflater);
                    Expect(Failure.DictionaryRequired, () => driver.Run(sample, Envelope.Zlib, feed, 1,
                        stopAtDictionary: true));
                    Check(inflater.IsNeedingDictionary && inflater.TotalIn == 6 && inflater.TotalOut == 0);
                    Check(driver.DictionaryRequests == 1 && driver.Completed == null);
                });
                Run($"dictionary.wrong.{sample.Name}.{feed}", () =>
                {
                    var wrong = (byte[])sample.Dictionary!.Clone();
                    wrong[0] ^= 1;
                    Check(Fixtures.Adler(wrong) != Fixtures.Adler(sample.Dictionary!));
                    var driver = new StateDriver(new Inflater());
                    Expect(Failure.DictionaryMismatch, () => driver.Run(sample, Envelope.Zlib, feed, 1,
                        suppliedDictionary: wrong));
                    Check(driver.Completed == null && driver.DictionaryRequests == 1);
                });
                Run($"dictionary.output-adler.{sample.Name}.{feed}", () =>
                {
                    var wire = sample.Wire(Envelope.Zlib);
                    var dictAdler = Fixtures.Adler(sample.Dictionary!);
                    Check(dictAdler != Fixtures.Adler(sample.Output));
                    BinaryPrimitives.WriteUInt32BigEndian(wire.AsSpan(wire.Length - 4), dictAdler);
                    Reject(sample, Envelope.Zlib, feed, 1, Failure.Decode, wire);
                });
            }
        }
        foreach (var feed in new[] { 1, 7, 65536 })
        {
            Run($"dictionary.full-id.{feed}", () =>
            {
                var tail = window.Dictionary![1..];
                Check(Fixtures.Adler(tail) != Fixtures.Adler(window.Dictionary!));
                var driver = new StateDriver(new Inflater());
                Expect(Failure.DictionaryMismatch, () => driver.Run(window, Envelope.Zlib, feed, 1,
                    suppliedDictionary: tail));
                Check(driver.Completed == null);
            });
        }
        foreach (var raw in new[] { false, true })
        {
            Run($"api.dictionary-without-request.{raw}", () =>
            {
                var inflater = new Inflater(raw);
                try { inflater.SetDictionary([88, 89]); }
                catch (InvalidOperationException) { return; }
                throw new Exception("사전 요청 전 API 호출이 수용됨");
            });
            Run($"reset.completed.{raw}", () => ResetCompleted(raw, false));
            Run($"reset.partial.{raw}", () => ResetPartial(raw));
        }
        Run("reset.dictionary", () => ResetCompleted(false, true));
        foreach (var sample in new[] { xy, window })
        {
            var wire = sample.Wire(Envelope.Zlib);
            foreach (var prefix in new[] { 1, 3, 5, wire.Length - 1 })
            {
                Run($"truncated.zlib.{sample.Name}.{prefix}", () =>
                    Reject(sample, Envelope.Zlib, 1, 1, Failure.Truncated, wire[..prefix], false));
            }
        }
        foreach (var prefix in new[] { 0, 1 })
        {
            Run($"truncated.raw.empty.{prefix}", () =>
                Reject(Fixtures.EmptyFixed(), Envelope.Raw, 1, 1, Failure.Truncated,
                    Fixtures.EmptyFixed().Raw[..prefix], false));
        }
        Console.WriteLine($"stateful passed={passed} failed={failed}");
        return failed == 0 ? 0 : 1;
    }

    private static void ResetCompleted(bool raw, bool dictionary)
    {
        var envelope = raw ? Envelope.Raw : Envelope.Zlib;
        var inflater = new Inflater(raw);
        var first = dictionary ? Fixtures.DictionaryWindow() : Fixtures.Overlap();
        new StateDriver(inflater).Run(first, envelope, 65536, 1);
        Check(inflater.IsFinished && inflater.RemainingInput == Fixtures.Sentinel.Length);
        inflater.Reset();
        ResetState(inflater, raw);
        new StateDriver(inflater).Run(Fixtures.Stored(3), envelope, 7, 1);
        Check(inflater.IsFinished && inflater.TotalOut == 3);
        inflater.Reset();
        ResetState(inflater, raw);
        var rejected = new StateDriver(inflater);
        Expect(Failure.Decode, () => rejected.Run(Fixtures.InvalidFirstReference(), envelope, 65536, 1));
        Check(rejected.Completed == null && inflater.TotalOut == 0);
    }

    private static void ResetPartial(bool raw)
    {
        var envelope = raw ? Envelope.Raw : Envelope.Zlib;
        var sample = Fixtures.Stored(32769);
        var wire = sample.Wire(envelope);
        var input = wire.Concat(Fixtures.Sentinel).ToArray();
        var snapshot = SHA256.HashData(input);
        var inflater = new Inflater(raw);
        inflater.SetInput(input);
        var one = new byte[1];
        Check(inflater.Inflate(one) == 1 && one[0] == sample.Output.At(0));
        Check(!inflater.IsFinished && inflater.TotalOut == 1);
        Check(inflater.RemainingInput > Fixtures.Sentinel.Length + (raw ? 0 : 4));
        Check(inflater.TotalIn + inflater.RemainingInput == input.Length);
        var consumed = inflater.TotalIn;
        Check(inflater.Inflate(one) == 1 && one[0] == sample.Output.At(1));
        Check(inflater.TotalIn == consumed && inflater.TotalOut == 2 && !inflater.IsFinished);
        inflater.Reset();
        ResetState(inflater, raw);
        var next = new StateDriver(inflater);
        next.Run(Fixtures.Stored(3), envelope, 1, 1);
        Check(next.Completed is { Length: 3 });
        Check(SHA256.HashData(input).AsSpan().SequenceEqual(snapshot));
    }

    private static void ResetState(Inflater inflater, bool raw)
    {
        Check(inflater.TotalIn == 0 && inflater.TotalOut == 0 && inflater.RemainingInput == 0);
        Check(inflater.IsNeedingInput && !inflater.IsFinished && !inflater.IsNeedingDictionary);
        Check(inflater.Adler == (raw ? 0 : 1));
    }

    private static void VerifyFixtures()
    {
        foreach (var sample in new[] { Fixtures.EmptyFixed(), Fixtures.Stored(0), Fixtures.Overlap(),
            Fixtures.StoredFixed(), Fixtures.DictionaryXy(), Fixtures.DictionaryWindow() })
        {
            Check(sample.Raw.Length == (sample.EndBits + 7) / 8);
            if (sample.EndBits % 8 != 0)
            {
                Check((sample.Raw[^1] >> (sample.EndBits % 8)) == 0);
            }
        }
        Check(Fixtures.Stored(0).Raw.AsSpan().SequenceEqual(new byte[] { 1, 0, 0, 255, 255 }));
        Check(Fixtures.Overlap().Raw.AsSpan().SequenceEqual(new byte[] { 0x73, 0x1c, 0x05, 0x00 }));
        Check(Fixtures.DictionaryXy().Raw.AsSpan().SequenceEqual(new byte[] { 0x03, 0x43, 0x00 }));
        Check(Fixtures.DictionaryWindow().Raw.AsSpan().SequenceEqual(new byte[] { 0x03, 0xde, 0xff, 0x0f, 0x00 }));
        Check(Fixtures.StoredFixed().Raw.AsSpan().SequenceEqual(new byte[]
            { 0x00, 0x02, 0x00, 0xfd, 0xff, 0x58, 0x59, 0x03, 0x43, 0x00 }));
        Check(Fixtures.Adler([88, 89]) == 0x010b00b2);
        Check(Fixtures.Adler(new Pattern(5, i => (byte)(88 + i % 2))) == 0x053301bb);
        Check(Fixtures.Adler([]) == 1);
        Check(Fixtures.Crc(new Pattern(9, i => (byte)(49 + i))) == 0xcbf43926);
    }

    private static void Reject(Sample sample, Envelope envelope, int feed, int size, Failure expected,
        byte[]? wire = null, bool sentinel = true)
    {
        var driver = new StateDriver(new Inflater(envelope != Envelope.Zlib));
        Expect(expected, () => driver.Run(sample, envelope, feed, size,
            wireOverride: wire, addSentinel: sentinel));
        Check(driver.Completed == null);
    }

    private static void Expect(Failure expected, Action action)
    {
        try { action(); }
        catch (ProbeFailure failure) when (failure.Kind == expected) { return; }
        throw new Exception($"예상 거부 없음: {expected}");
    }

    private static void Run(string name, Action action)
    {
        try { action(); passed++; }
        catch (Exception error)
        {
            failed++;
            Console.WriteLine($"FAIL {name}: {error.GetType().Name} {error.Message}");
        }
    }

    private static void Check(bool condition) => StateDriver.Require(condition, "상태 API 회귀");
}

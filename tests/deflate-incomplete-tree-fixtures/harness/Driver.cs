using System.Diagnostics;
using System.Security.Cryptography;

namespace Zipelo.IncompleteFixtures;

internal sealed record Trial(string Name, int InputChunk, int OutputChunk, int TailBytes, bool Gates, int? Cut = null);
internal sealed record Observation(
    string Fixture, string Trial, bool Passed, string Outcome, string? ExceptionType,
    long Output, long ReturnedOutput, string OutputSha256, long Consumed, int Remaining,
    int Offered, bool Finished, long? FirstMismatch, bool GuardOk, bool InputOk,
    string[] Violations, string[] Checkpoints, int Calls);

internal sealed class Driver
{
    private const int GuardSize = 16;
    private const int OutputLimit = 65536;
    private const int CallLimit = 100000;
    private const byte GuardByte = 0xa5;
    private readonly Func<IDecoder> createDecoder;
    private readonly Func<Exception, bool> isDeflateError;
    private readonly Func<TimeSpan>? elapsedOverride;

    public Driver(DecoderFactory factory)
        : this(() => factory.Create(), factory.IsDeflateError, null) { }

    internal Driver(Func<IDecoder> createDecoder, Func<Exception, bool> isDeflateError,
        Func<TimeSpan>? elapsedOverride)
    {
        this.createDecoder = createDecoder;
        this.isDeflateError = isDeflateError;
        this.elapsedOverride = elapsedOverride;
    }

    public Observation Run(EmittedFixture fixture, Trial trial)
    {
        var spec = fixture.Spec;
        var raw = Convert.FromHexString(fixture.RawHex);
        var source = trial.Cut is int cut ? raw[..cut] : raw.Concat(new byte[trial.TailBytes]).ToArray();
        var original = (byte[])source.Clone();
        var expected = Specification.Bytes(spec.Valid ? spec.OutputText : spec.ErrorPrefix);
        var decoder = createDecoder();
        var buffer = new byte[GuardSize + trial.OutputChunk + GuardSize];
        var watch = Stopwatch.StartNew();
        bool TimedOut() => (elapsedOverride?.Invoke() ?? watch.Elapsed) > TimeSpan.FromSeconds(5);
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        var violations = new List<string>();
        var checkpoints = new List<string>();
        var offered = 0;
        var gateIndex = 0;
        var calls = 0;
        long observed = 0;
        long returned = 0;
        long? mismatch = null;
        var guardOk = true;
        var inputOk = true;
        string outcome = "NoProgress";
        string? exceptionType = null;
        var gates = trial.Gates ? spec.Checkpoints : Array.Empty<Checkpoint>();

        void Violation(string value)
        {
            if (!violations.Contains(value)) { violations.Add(value); }
        }

        bool Counters()
        {
            var ok = decoder.TotalIn >= 0 && decoder.Remaining >= 0
                && decoder.TotalIn + decoder.Remaining == offered && decoder.TotalOut == observed;
            if (!ok) { Violation("Counter"); }
            return ok;
        }

        (int Count, Exception? Error) Call()
        {
            Array.Fill(buffer, GuardByte);
            var capacity = Math.Min(trial.OutputChunk, OutputLimit - (int)observed);
            var beforeOut = decoder.TotalOut;
            var beforeIn = decoder.TotalIn;
            var count = 0;
            Exception? error = null;
            calls++;
            try { count = decoder.Inflate(buffer, GuardSize, capacity); }
            catch (Exception caught) { error = caught; }
            finally
            {
                var delta = decoder.TotalOut - beforeOut;
                guardOk &= buffer.AsSpan(0, GuardSize).IndexOfAnyExcept(GuardByte) < 0
                    && buffer.AsSpan(GuardSize + capacity).IndexOfAnyExcept(GuardByte) < 0;
                inputOk &= source.AsSpan().SequenceEqual(original);
                if (!guardOk) { Violation("Guard"); }
                if (!inputOk) { Violation("InputMutation"); }
                if (delta < 0 || delta > capacity || decoder.TotalIn < beforeIn)
                {
                    Violation("Counter");
                }
                else
                {
                    var bytes = (int)delta;
                    for (var index = 0; index < bytes; index++)
                    {
                        var offset = observed + index;
                        if (mismatch is null && (offset >= expected.Length
                            || buffer[GuardSize + index] != expected[(int)offset]))
                        {
                            mismatch = offset;
                        }
                    }
                    hash.AppendData(buffer, GuardSize, bytes);
                    observed += bytes;
                    if (buffer.AsSpan(GuardSize + bytes, capacity - bytes).IndexOfAnyExcept(GuardByte) >= 0)
                    {
                        Violation("UnreportedOutput");
                    }
                }
                if (error is null)
                {
                    returned += count;
                    if (count < 0 || count > capacity || count != delta) { Violation("ReturnedCount"); }
                }
                Counters();
            }
            return (count, error);
        }

        try
        {
            while (true)
            {
                if (TimedOut()) { outcome = "Timeout"; break; }
                if (calls >= CallLimit) { outcome = "NoProgressLimit"; break; }
                var call = Call();
                exceptionType = call.Error?.GetType().FullName;
                if (TimedOut()) { outcome = "Timeout"; break; }
                if (observed >= OutputLimit) { outcome = "Budget"; break; }
                if (violations.Count > 0) { outcome = "HarnessInvariant"; break; }
                if (call.Error is not null)
                {
                    exceptionType = call.Error.GetType().FullName;
                    outcome = call.Error is IndexOutOfRangeException ? "ArrayError"
                        : isDeflateError(call.Error) ? "Deflate" : "UnexpectedException";
                    break;
                }
                if (decoder.Finished)
                {
                    outcome = "Complete";
                    var beforeIn = decoder.TotalIn;
                    var beforeRemaining = decoder.Remaining;
                    if (TimedOut()) { outcome = "Timeout"; break; }
                    var afterFinish = Call();
                    exceptionType = afterFinish.Error?.GetType().FullName;
                    if (TimedOut()) { outcome = "Timeout"; break; }
                    if (afterFinish.Error is not null || afterFinish.Count != 0 || !decoder.Finished
                        || beforeIn != decoder.TotalIn || beforeRemaining != decoder.Remaining)
                    {
                        Violation("AfterFinish");
                    }
                    break;
                }
                if (call.Count > 0) { continue; }
                if (decoder.NeedDictionary) { outcome = "UnexpectedDictionary"; break; }
                if (!decoder.NeedInput) { outcome = "NoProgress"; break; }

                if (gateIndex < gates.Length && offered == gates[gateIndex].Offered)
                {
                    var gate = gates[gateIndex++];
                    if (gate.Expected == "Reject")
                    {
                        checkpoints.Add($"reject-missed:{gate.Offered}");
                        outcome = "MissingImmediateRejection";
                        break;
                    }
                    var ok = observed == gate.OutputLength && decoder.TotalIn == gate.Consumed
                        && decoder.Remaining == gate.Remaining && !decoder.Finished && mismatch is null;
                    checkpoints.Add($"{(ok ? "wait" : "wait-mismatch")}:{gate.Offered}");
                    if (!ok) { Violation("Checkpoint"); break; }
                }
                if (offered == source.Length) { outcome = "Eof"; break; }
                var limit = gateIndex < gates.Length ? gates[gateIndex].Offered : source.Length;
                var count = Math.Min(trial.InputChunk, Math.Min(source.Length, limit) - offered);
                if (count <= 0) { outcome = "NoProgress"; break; }
                decoder.SetInput(source, offered, count);
                offered += count;
                if (!Counters()) { outcome = "HarnessInvariant"; break; }
            }
        }
        catch (Exception error)
        {
            outcome = "HarnessException";
            exceptionType = error.GetType().FullName;
        }
        finally
        {
            inputOk &= source.AsSpan().SequenceEqual(original);
            if (!inputOk) { Violation("InputMutation"); }
        }

        if (outcome == "Deflate" && trial.Gates && gateIndex < gates.Length
            && gates[gateIndex].Expected == "Reject" && offered == gates[gateIndex].Offered)
        {
            checkpoints.Add($"reject:{offered}");
            gateIndex++;
        }
        if (trial.Gates && gateIndex != gates.Length) { Violation("UnvisitedCheckpoint"); }
        var digest = Convert.ToHexString(hash.GetHashAndReset()).ToLowerInvariant();
        var passed = violations.Count == 0 && guardOk && inputOk && mismatch is null;
        if (trial.Cut is int prefixLength)
        {
            var gate = spec.Checkpoints.Single(item => item.Offered == prefixLength && item.Expected == "Wait");
            passed &= outcome == "Eof" && !decoder.Finished && observed == gate.OutputLength
                && decoder.TotalIn == gate.Consumed && decoder.Remaining == gate.Remaining;
        }
        else if (spec.Valid)
        {
            passed &= outcome == "Complete" && observed == expected.Length && decoder.TotalIn == spec.RawLength
                && digest == fixture.ExpectedOutputSha256;
        }
        else
        {
            passed &= outcome == "Deflate" && !decoder.Finished && observed <= expected.Length
                && decoder.TotalIn >= spec.ErrorConsumedMin && decoder.TotalIn <= spec.ErrorConsumedMax;
        }
        return new Observation(spec.Id, trial.Name, passed, outcome, exceptionType, observed, returned,
            digest, decoder.TotalIn, decoder.Remaining, offered, decoder.Finished, mismatch,
            guardOk, inputOk, violations.ToArray(), checkpoints.ToArray(), calls);
    }
}

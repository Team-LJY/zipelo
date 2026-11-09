#nullable enable
using System;
using System.Diagnostics;
using System.Security.Cryptography;
using System.Threading;
using ICSharpCode.SharpZipLib;
using ICSharpCode.SharpZipLib.Zip.Compression;

namespace Zipelo.Experimental;

public sealed record RawResult(long Consumed, int Remaining, long Offered, long Decoded, string Sha256);

public sealed class ProbeFailure(string code) : Exception(code)
{
    public string Code { get; } = code;
}

// 입력은 호출 동안 독점 보유하며 결과는 종료와 최종 취소 확인 후 반환함.
public static class RawProbe
{
    public const int ChunkLimit = 65536;
    public const int InputLimit = 16 * 1024 * 1024;
    public const long ProcessedLimit = 128L * 1024 * 1024;
    public const long DecodedLimit = 16L * 1024 * 1024;

    public static RawResult Run(byte[] input, int feedSize = ChunkLimit, int outputSize = ChunkLimit,
        long maxProcessed = ProcessedLimit, long maxDecoded = DecodedLimit,
        CancellationToken token = default)
    {
        ArgumentNullException.ThrowIfNull(input);
        if (feedSize is < 1 or > ChunkLimit || outputSize is < 1 or > ChunkLimit ||
            maxProcessed < 0 || maxProcessed > ProcessedLimit ||
            maxDecoded < 0 || maxDecoded > DecodedLimit)
        {
            throw new ArgumentOutOfRangeException();
        }
        if (input.Length > InputLimit) { throw new ProbeFailure("Budget"); }
        var watch = Stopwatch.StartNew();
        var inflater = new Inflater(true);
        var output = new byte[outputSize];
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        var offered = 0;
        long decoded = 0;
        long processed = 0;
        while (!inflater.IsFinished)
        {
            Check();
            if (inflater.IsNeedingDictionary) { throw new ProbeFailure("Dictionary"); }
            if (inflater.IsNeedingInput)
            {
                var count = Math.Min(feedSize, input.Length - offered);
                if (count > 0)
                {
                    Charge(count);
                    inflater.SetInput(input, offered, count);
                    offered += count;
                }
            }
            var before = inflater.TotalIn;
            int written;
            try { written = inflater.Inflate(output); }
            catch (SharpZipBaseException) { throw new ProbeFailure("Deflate"); }
            Check();
            if (written < 0 || written > output.Length ||
                inflater.TotalIn < before || inflater.TotalIn > offered ||
                inflater.RemainingInput < 0 ||
                inflater.TotalIn != offered - inflater.RemainingInput ||
                inflater.TotalOut != decoded + written)
            {
                throw new ProbeFailure("Boundary");
            }
            Charge(written);
            if (written > maxDecoded - decoded) { throw new ProbeFailure("Budget"); }
            decoded += written;
            hash.AppendData(output.AsSpan(0, written));
            if (written == 0 && inflater.TotalIn == before && !inflater.IsFinished)
            {
                if (inflater.IsNeedingInput && offered == input.Length)
                {
                    throw new ProbeFailure("Truncated");
                }
                if (!inflater.IsNeedingInput) { throw new ProbeFailure("NoProgress"); }
            }
        }
        Check();
        if (inflater.TotalOut != decoded || inflater.TotalIn != offered - inflater.RemainingInput)
        {
            throw new ProbeFailure("Boundary");
        }
        var digest = Convert.ToHexString(hash.GetHashAndReset());
        Check();
        return new RawResult(inflater.TotalIn, inflater.RemainingInput, offered, decoded, digest);

        void Charge(int count)
        {
            if (count < 0 || count > maxProcessed - processed) { throw new ProbeFailure("Budget"); }
            processed += count;
        }

        void Check()
        {
            token.ThrowIfCancellationRequested();
            if (watch.Elapsed >= TimeSpan.FromSeconds(15)) { throw new ProbeFailure("Budget"); }
        }
    }
}

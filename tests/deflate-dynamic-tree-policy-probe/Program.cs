using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq;
using ICSharpCode.SharpZipLib;
using ICSharpCode.SharpZipLib.Zip.Compression;

internal static class Program
{
    private const string CandidateAssembly = "Zipelo.Experimental.SharpZipLibDynamicTreePolicyCandidate";
    private static readonly int[] Order = { 16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15 };

    private sealed record Fixture(string Name, byte[] Raw, int DeclarationBits, string? Error);

    private static int Main(string[] args)
    {
        if (args.Length != 0)
        {
            Console.Error.WriteLine("Usage: Zipelo.Experimental.DeflateDynamicTreePolicyValidation");
            return 2;
        }

        int passed = 0;
        int failed = 0;
        try
        {
            Require(typeof(Inflater).Assembly.GetName().Name == CandidateAssembly, "Wrong candidate assembly");
            Fixture[] fixtures = CreateFixtures();
            Require(fixtures.Length == 17, "Unexpected fixture count");
            foreach (Fixture fixture in fixtures)
            {
                foreach (int chunk in new[] { 1, fixture.Raw.Length })
                {
                    try
                    {
                        Check(fixture, chunk);
                        passed++;
                    }
                    catch (Exception error)
                    {
                        failed++;
                        Console.Error.WriteLine(fixture.Name + "/" + chunk + ": " + error.GetType().Name + ": " + error.Message);
                    }
                }
            }
        }
        catch (Exception error)
        {
            Console.Error.WriteLine(error.GetType().Name + ": " + error.Message);
            return 1;
        }

        Console.WriteLine("passed=" + passed + " failed=" + failed + " expected=34");
        return failed == 0 && passed == 34 ? 0 : 1;
    }

    private static Fixture[] CreateFixtures()
    {
        byte[] eob = new byte[257];
        eob[256] = 1;
        byte[] longTree = new byte[257];
        for (int i = 0; i < 14; i++)
        {
            longTree[i] = (byte)(i + 1);
        }
        longTree[14] = 15;
        longTree[256] = 15;
        byte[] unusedLength = new byte[258];
        unusedLength[65] = 1;
        unusedLength[256] = 2;
        unusedLength[257] = 2;
        byte[] hdist31 = new byte[31];
        byte[] hdist32 = new byte[32];
        hdist31[0] = hdist32[0] = 1;
        byte[] eob2 = (byte[])eob.Clone();
        byte[] eob10 = (byte[])eob.Clone();
        eob2[256] = 2;
        eob10[256] = 10;
        byte[] missingEob = new byte[257];
        missingEob[0] = missingEob[1] = 1;
        byte[] overLens = (byte[])eob.Clone();
        overLens[0] = overLens[1] = 1;
        byte[] emptyCodes = new byte[19];
        byte[] singleCodes = new byte[19];
        singleCodes[0] = 1;
        byte[] incompleteCodes = new byte[19];
        incompleteCodes[0] = incompleteCodes[1] = 2;
        byte[] overCodes = new byte[19];
        overCodes[0] = overCodes[1] = overCodes[2] = 1;

        return new[]
        {
            Build("valid-eob-empty-distance", eob, new byte[] { 0 }),
            Build("valid-single-distance", eob, new byte[] { 1 }),
            Build("valid-hdist31", eob, hdist31),
            Build("valid-hdist32", eob, hdist32),
            Build("valid-complete-length15", longTree, new byte[] { 0 }),
            Build("valid-unused-length-empty-distance", unusedLength, new byte[] { 0 }),
            Build("invalid-eob-length2", eob2, new byte[] { 0 }, "Incomplete Lens Huffman tree"),
            Build("invalid-eob-length10", eob10, new byte[] { 0 }, "Incomplete Lens Huffman tree"),
            Build("invalid-unused-single-distance2", eob, new byte[] { 2 }, "Incomplete Dists Huffman tree"),
            Build("invalid-unused-distance22", eob, new byte[] { 2, 2 }, "Incomplete Dists Huffman tree"),
            Build("invalid-empty-codes", eob, new byte[] { 0 }, "Incomplete Codes Huffman tree", emptyCodes),
            Build("invalid-single-codes", eob, new byte[] { 0 }, "Incomplete Codes Huffman tree", singleCodes),
            Build("invalid-incomplete-codes", eob, new byte[] { 0 }, "Incomplete Codes Huffman tree", incompleteCodes),
            Build("invalid-oversubscribed-codes", eob, new byte[] { 0 }, "Oversubscribed Codes Huffman tree", overCodes),
            Build("invalid-missing-eob", missingEob, new byte[] { 0 }, "Inflater dynamic header end-of-block code missing"),
            Build("invalid-oversubscribed-lens", overLens, new byte[] { 0 }, "Oversubscribed Lens Huffman tree"),
            Build("invalid-oversubscribed-distance", eob, new byte[] { 1, 1, 1 }, "Oversubscribed Dists Huffman tree")
        };
    }

    private static Fixture Build(string name, byte[] lens, byte[] dists, string? error = null, byte[]? invalidCodes = null)
    {
        var bits = new BitWriter();
        bits.Integer(1, 1);
        bits.Integer(2, 2);
        bits.Integer(lens.Length - 257, 5);
        bits.Integer(dists.Length - 1, 5);
        bits.Integer(15, 4);
        byte[] codes = invalidCodes ?? Enumerable.Range(0, 19).Select(i => (byte)(i < 16 ? 4 : 0)).ToArray();
        foreach (int symbol in Order)
        {
            bits.Integer(codes[symbol], 3);
        }
        Require(bits.Count == 74, "Header recipe mismatch");
        if (invalidCodes != null)
        {
            return new Fixture(name, bits.Bytes(), bits.Count, error);
        }

        foreach (byte length in lens.Concat(dists))
        {
            bits.Code(length, 4);
        }
        int boundary = bits.Count;
        if (error == null)
        {
            int[] counts = new int[16];
            foreach (byte length in lens)
            {
                if (length != 0)
                {
                    counts[length]++;
                }
            }
            int code = 0;
            int[] next = new int[16];
            for (int width = 1; width <= 15; width++)
            {
                code = (code + counts[width - 1]) << 1;
                next[width] = code;
            }
            for (int symbol = 0; symbol < 256; symbol++)
            {
                if (lens[symbol] != 0)
                {
                    next[lens[symbol]]++;
                }
            }
            bits.Code(next[lens[256]], lens[256]);
        }
        Require(bits.Count <= 2048, "Fixture bound exceeded");
        return new Fixture(name, bits.Bytes(), boundary, error);
    }

    private static void Check(Fixture fixture, int chunk)
    {
        byte[] original = (byte[])fixture.Raw.Clone();
        byte[] input = (byte[])fixture.Raw.Clone();
        byte[] output = { 0xA5, 0xA5, 0xA5 };
        var decoder = new Inflater(true);
        var clock = Stopwatch.StartNew();
        int offered = 0;
        try
        {
            for (int calls = 0; calls < 4096; calls++)
            {
                Require(clock.Elapsed < TimeSpan.FromSeconds(5), "Cooperative timeout");
                int written;
                try
                {
                    written = decoder.Inflate(output, 1, 1);
                }
                catch (StreamDecodingException error)
                {
                    Require(clock.Elapsed < TimeSpan.FromSeconds(5), "Cooperative timeout after throw");
                    Require(fixture.Error != null && error.Message == fixture.Error, "Unexpected typed rejection: " + error.Message);
                    Require(!decoder.IsFinished, "Rejected stream reported complete");
                    Require(decoder.TotalIn >= (fixture.DeclarationBits + 7) / 8, "Rejected before complete declaration");
                    return;
                }
                Require(clock.Elapsed < TimeSpan.FromSeconds(5), "Cooperative timeout after return");
                Require(written == 0, "Unexpected output");
                if (decoder.IsFinished)
                {
                    Require(fixture.Error == null, "Invalid stream accepted");
                    Require(decoder.TotalIn == input.Length && decoder.RemainingInput == 0, "Completion consumption mismatch");
                    Require(decoder.Inflate(output, 1, 1) == 0 && decoder.IsFinished, "Unstable completion");
                    Require(clock.Elapsed < TimeSpan.FromSeconds(5), "Cooperative timeout after stability probe");
                    return;
                }
                Require(decoder.IsNeedingInput, "No-progress state");
                Require(offered < input.Length, "Unexpected EOF instead of completion or typed rejection");
                int count = Math.Min(chunk, input.Length - offered);
                decoder.SetInput(input, offered, count);
                offered += count;
            }
            throw new InvalidOperationException("Call budget exhausted");
        }
        finally
        {
            Require(output.All(value => value == 0xA5), "Output or guard changed");
            Require(input.SequenceEqual(original), "Input changed");
            Require(decoder.TotalOut == 0, "Unexpected cumulative output");
            Require(decoder.TotalIn >= 0 && decoder.TotalIn <= offered, "Invalid input accounting");
            Require(decoder.RemainingInput >= 0 && decoder.TotalIn + decoder.RemainingInput == offered, "Input conservation failed");
        }
    }

    private static void Require(bool condition, string message)
    {
        if (!condition)
        {
            throw new InvalidOperationException(message);
        }
    }

    private sealed class BitWriter
    {
        private readonly List<byte> bytes = new List<byte>();
        public int Count { get; private set; }

        private void Bit(int value)
        {
            if ((Count & 7) == 0)
            {
                bytes.Add(0);
            }
            bytes[Count >> 3] |= (byte)((value & 1) << (Count & 7));
            Count++;
        }

        public void Integer(int value, int width)
        {
            for (int i = 0; i < width; i++)
            {
                Bit(value >> i);
            }
        }

        public void Code(int value, int width)
        {
            for (int i = width - 1; i >= 0; i--)
            {
                Bit(value >> i);
            }
        }

        public byte[] Bytes() => bytes.ToArray();
    }
}

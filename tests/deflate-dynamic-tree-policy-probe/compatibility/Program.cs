using System;
using System.Collections.Generic;
using System.Diagnostics;
using ICSharpCode.SharpZipLib;
using ICSharpCode.SharpZipLib.Zip.Compression;
using ICSharpCode.SharpZipLib.Zip.Compression.Streams;

internal static class Program
{
    private const string InvalidLength = "Encountered invalid codelength 0";
    private const string CandidateAssembly = "Zipelo.Experimental.SharpZipLibDynamicTreePolicyCandidate";

    private static int Main(string[] args)
    {
        if (args.Length != 0)
        {
            Console.Error.WriteLine("Usage: Zipelo.Experimental.DeflateZeroSlotValidation");
            return 2;
        }

        var cases = new List<(string Name, Action Run)>
        {
            ("candidate-identity", CheckIdentity),
            ("empty-tree-zero-bits-reject", () => CheckRejection(new byte[] { 0 }, 0)),
            ("single-zero-bits-wait-then-symbol-zero", CheckSingle),
            ("complete-tree-symbol-zero", CheckSymbolZero),
            ("single-unused-partial-root-reject", () => CheckRejection(new byte[] { 1 }, 1)),
            ("single-unused-full-root-reject", () => CheckRejection(new byte[] { 1 }, 9)),
            ("partial-subtree-complete-ten-bit-leaf", () => CheckLongLeaf(10, 511, 9)),
            ("full-subtree-complete-eleven-bit-leaf", () => CheckLongLeaf(11, 1023, 10))
        };

        for (int bits = 0; bits <= 10; bits++)
        {
            int prefixBits = bits;
            cases.Add(($"complete-tree-wait-{bits}-then-eleven-bit-leaf",
                () => CheckWaitThenComplete(prefixBits)));
        }

        int failed = 0;
        foreach (var test in cases)
        {
            var timer = Stopwatch.StartNew();
            Exception? failure = null;
            try
            {
                test.Run();
            }
            catch (Exception error)
            {
                failure = error;
            }
            finally
            {
                if (timer.Elapsed > TimeSpan.FromSeconds(5))
                {
                    failure = new TimeoutException("Cooperative case budget exceeded.", failure);
                }
            }

            if (failure is null)
            {
                Console.WriteLine($"PASS {test.Name}");
            }
            else
            {
                failed++;
                Console.WriteLine($"FAIL {test.Name}: {failure.GetType().Name}: {failure.Message}");
            }
        }

        Console.WriteLine($"cases={cases.Count}; passed={cases.Count - failed}; failed={failed}");
        return failed == 0 ? 0 : 1;
    }

    private static void CheckIdentity()
    {
        Require(typeof(InflaterHuffmanTree).Assembly.GetName().Name == CandidateAssembly, "Wrong tree assembly.");
        Require(typeof(Inflater).Assembly == typeof(InflaterHuffmanTree).Assembly, "Wrong inflater assembly.");
        Require(typeof(StreamManipulator).Assembly == typeof(InflaterHuffmanTree).Assembly, "Wrong input assembly.");
    }

    private static InflaterHuffmanTree CompleteTree()
    {
        return new InflaterHuffmanTree(new byte[] { 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 11 });
    }

    private static void CheckRejection(byte[] lengths, int bits)
    {
        var tree = new InflaterHuffmanTree(lengths);
        var input = new InputState(bits, (1 << bits) - 1);
        bool rejected = false;
        try
        {
            tree.GetSymbol(input.Reader);
        }
        catch (SharpZipBaseException error) when (
            error.GetType() == typeof(SharpZipBaseException) && error.Message == InvalidLength)
        {
            rejected = true;
        }
        finally
        {
            input.CheckUnchanged();
            Require(input.Reader.AvailableBits == bits, "Rejected leaf consumed bits.");
            Require(input.Reader.PeekBits(bits) == (1 << bits) - 1, "Rejected prefix changed.");
        }

        Require(rejected, "Missing typed zero-leaf rejection.");
    }

    private static void CheckSingle()
    {
        var tree = new InflaterHuffmanTree(new byte[] { 1 });
        var input = new InputState(0, 0);
        try
        {
            Wait(tree, input, 0);
            input.Append(new byte[] { 0 });
            Require(tree.GetSymbol(input.Reader) == 0, "Single symbol zero did not decode.");
            Require(input.Reader.AvailableBits == 7, "Single leaf consumed wrong bit count.");
            Require(input.Reader.PeekBits(7) == 0, "Single leaf remainder changed.");
        }
        finally
        {
            input.CheckUnchanged();
        }
    }

    private static void CheckSymbolZero()
    {
        var tree = new InflaterHuffmanTree(new byte[] { 1, 1 });
        var input = new InputState(1, 0);
        try
        {
            Require(tree.GetSymbol(input.Reader) == 0, "Valid symbol zero was rejected.");
            Require(input.Reader.AvailableBits == 0, "Symbol zero consumed wrong bit count.");
        }
        finally
        {
            input.CheckUnchanged();
        }
    }

    private static void CheckLongLeaf(int bits, int wireValue, int expectedSymbol)
    {
        var input = new InputState(bits, wireValue);
        try
        {
            Require(CompleteTree().GetSymbol(input.Reader) == expectedSymbol, "Wrong long leaf.");
            Require(input.Reader.AvailableBits == 0, "Long leaf consumed wrong bit count.");
        }
        finally
        {
            input.CheckUnchanged();
        }
    }

    private static void CheckWaitThenComplete(int prefixBits)
    {
        var tree = CompleteTree();
        var input = new InputState(prefixBits, (1 << prefixBits) - 1);
        try
        {
            Wait(tree, input, prefixBits);
            int missing = 11 - prefixBits;
            int tailValue = (1 << missing) - 1;
            var tail = new byte[(missing + 7) / 8];
            for (int i = 0; i < tail.Length; i++)
            {
                tail[i] = (byte)(tailValue >> (8 * i));
            }

            input.Append(tail);
            Require(tree.GetSymbol(input.Reader) == 11, "Continuation decoded wrong symbol.");
            int remaining = prefixBits + tail.Length * 8 - 11;
            Require(input.Reader.AvailableBits == remaining, "Continuation consumed wrong bit count.");
            Require(input.Reader.PeekBits(remaining) == 0, "Continuation remainder changed.");
        }
        finally
        {
            input.CheckUnchanged();
        }
    }

    private static void Wait(InflaterHuffmanTree tree, InputState input, int bits)
    {
        int prefix = input.Reader.PeekBits(bits);
        for (int attempt = 0; attempt < 2; attempt++)
        {
            Require(tree.GetSymbol(input.Reader) == -1, "Incomplete prefix did not wait.");
            Require(input.Reader.AvailableBits == bits, "Waiting consumed bits.");
            Require(input.Reader.PeekBits(bits) == prefix, "Waiting changed prefix.");
        }
    }

    private static void Require(bool condition, string message)
    {
        if (!condition)
        {
            throw new InvalidOperationException(message);
        }
    }

    private sealed class InputState
    {
        private readonly List<(byte[] Buffer, byte[] Snapshot)> buffers = new();
        public StreamManipulator Reader { get; } = new();

        public InputState(int bits, int wireValue)
        {
            Require(bits >= 0 && bits <= 11, "Unsupported direct prefix width.");
            if (bits == 0)
            {
                return;
            }

            int byteCount = (bits + 7) / 8;
            int padding = byteCount * 8 - bits;
            int shifted = wireValue << padding;
            var data = new byte[byteCount];
            for (int i = 0; i < data.Length; i++)
            {
                data[i] = (byte)(shifted >> (8 * i));
            }

            Append(data);
            Require(Reader.PeekBits(byteCount * 8) == shifted, "Prefix setup failed.");
            Reader.DropBits(padding);
            Require(Reader.AvailableBits == bits && Reader.PeekBits(bits) == wireValue,
                "Exact prefix setup failed.");
        }

        public void Append(byte[] bytes)
        {
            buffers.Add((bytes, (byte[])bytes.Clone()));
            Reader.SetInput(bytes, 0, bytes.Length);
        }

        public void CheckUnchanged()
        {
            foreach (var pair in buffers)
            {
                Require(pair.Buffer.AsSpan().SequenceEqual(pair.Snapshot), "Input bytes changed.");
            }
        }
    }
}

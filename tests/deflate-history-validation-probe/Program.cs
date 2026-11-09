#nullable enable
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Security.Cryptography;
using System.Text.Json;
using System.Threading;
using ICSharpCode.SharpZipLib;
using ICSharpCode.SharpZipLib.Zip.Compression;
using ICSharpCode.SharpZipLib.Zip.Compression.Streams;
using Zipelo.Experimental;

if (args.Length != 0)
{
    if (args.Length != 4 || args[0] != "--raw" ||
        !int.TryParse(args[2], NumberStyles.None, CultureInfo.InvariantCulture, out var feed) ||
        !int.TryParse(args[3], NumberStyles.None, CultureInfo.InvariantCulture, out var output) ||
        feed is < 1 or > RawProbe.ChunkLimit || output is < 1 or > RawProbe.ChunkLimit)
    {
        Console.Error.WriteLine("Usage: --raw synthetic.raw feedBytes outputBytes");
        return 2;
    }
    try
    {
        using var source = new FileStream(args[1], FileMode.Open, FileAccess.Read, FileShare.Read);
        var length = source.Length;
        if (length > RawProbe.InputLimit) { throw new ProbeFailure("Budget"); }
        var bytes = new byte[checked((int)length)];
        source.ReadExactly(bytes);
        if (source.ReadByte() != -1 || source.Length != length) { throw new ProbeFailure("InputChanged"); }
        var result = RawProbe.Run(bytes, feed, output);
        Console.WriteLine(JsonSerializer.Serialize(result));
        return 0;
    }
    catch (ProbeFailure error) { Console.WriteLine(JsonSerializer.Serialize(new { Failure = error.Code })); return 1; }
    catch (Exception error) { Console.WriteLine(JsonSerializer.Serialize(new { Failure = error.GetType().Name })); return 1; }
}

var tests = new List<(string Name, Action Run)>();
void Add(string name, Action action) => tests.Add((name, action));

foreach (var distance in new[] { -1, 0, 1, 32768, 32769 })
{
    Add("empty.distance." + distance, () =>
    {
        var window = new OutputWindow();
        Unchanged<SharpZipBaseException>(window, () => window.Repeat(3, distance));
    });
}
Add("partial.history.reject", () =>
{
    var window = new OutputWindow();
    window.Write(65);
    Unchanged<SharpZipBaseException>(window, () => window.Repeat(3, 2));
    Equal(Drain(window), [65]);
});
Add("repeat.invalid.length", () =>
{
    var window = new OutputWindow();
    window.Write(65);
    Unchanged<ArgumentOutOfRangeException>(window, () => window.Repeat(-1, 1));
    Unchanged<InvalidOperationException>(window, () => window.Repeat(int.MaxValue, 1));
});
Add("capacity.precheck", () =>
{
    var window = new OutputWindow();
    for (var i = 0; i < 32768; i++) { window.Write(i); }
    Unchanged<InvalidOperationException>(window, () => window.Write(7));
    Unchanged<InvalidOperationException>(window, () => window.Repeat(1, 1));
    var input = new StreamManipulator();
    input.SetInput([9], 0, 1);
    var before = WindowState.Capture(window);
    Check(window.CopyStored(input, 3) == 0);
    before.RequireSame(window);
    Check(input.AvailableBytes == 1);
    var actual = Drain(window);
    for (var i = 0; i < actual.Length; i++) { Check(actual[i] == (byte)i); }
});
Add("overlap.original.length", () =>
{
    var window = new OutputWindow();
    window.Write(65);
    window.Repeat(258, 1);
    Check(WindowState.Capture(window).History == 259);
    Check(Drain(window).All(value => value == 65));
    window.Repeat(3, 259);
    Equal(Drain(window), [65, 65, 65]);
});
Add("overlap.xy", () =>
{
    var window = new OutputWindow();
    window.Write(88);
    window.Write(89);
    window.Repeat(5, 2);
    Equal(Drain(window), "XYXYXYX"u8.ToArray());
    Check(WindowState.Capture(window).History == 7);
});
Add("drain.preserves.history", () =>
{
    var window = new OutputWindow();
    window.Write(17);
    Equal(Drain(window), [17]);
    Check(window.GetAvailable() == 0 && WindowState.Capture(window).History == 1);
    window.Repeat(3, 1);
    Equal(Drain(window), [17, 17, 17]);
});
Add("stored.actual.count", () =>
{
    var window = new OutputWindow();
    var input = new StreamManipulator();
    input.SetInput([7, 8], 0, 2);
    Check(window.CopyStored(input, 100) == 2);
    Check(WindowState.Capture(window).History == 2);
    Unchanged<SharpZipBaseException>(window, () => window.Repeat(3, 3));
    Equal(Drain(window), [7, 8]);
    window.Repeat(3, 2);
    Equal(Drain(window), [7, 8, 7]);
});
Add("stored.wrap", () =>
{
    var window = new OutputWindow();
    for (var i = 0; i < 32767; i++) { window.Write(i); }
    Drain(window);
    var input = new StreamManipulator();
    input.SetInput([71, 72, 73, 74], 0, 4);
    Check(window.CopyStored(input, 4) == 4);
    Equal(Drain(window), [71, 72, 73, 74]);
    Check(WindowState.Capture(window).History == 32768);
    window.Repeat(4, 4);
    Equal(Drain(window), [71, 72, 73, 74]);
});
Add("repeat.wrap.and.saturation", () =>
{
    var window = new OutputWindow();
    for (var i = 0; i < 32767; i++) { window.Write(i); }
    Drain(window);
    window.Repeat(258, 1);
    Check(Drain(window).All(value => value == 254));
    Check(WindowState.Capture(window).History == 32768);
    window.Repeat(3, 32768);
    Equal(Drain(window), [1, 2, 3]);
    Unchanged<SharpZipBaseException>(window, () => window.Repeat(3, 32769));
});
Add("history.exact.window", () =>
{
    var window = new OutputWindow();
    for (var i = 0; i < 32767; i++) { window.Write(i); }
    Unchanged<SharpZipBaseException>(window, () => window.Repeat(1, 32768));
    window.Write(255);
    Drain(window);
    window.Repeat(258, 32768);
    var actual = Drain(window);
    for (var i = 0; i < actual.Length; i++) { Check(actual[i] == (byte)i); }
});
Add("reset.no.stale.history", () =>
{
    var window = new OutputWindow();
    window.CopyDict([11, 12, 13], 0, 3);
    window.Repeat(3, 3);
    window.Reset();
    var state = WindowState.Capture(window);
    Check(state.History == 0 && state.Filled == 0 && state.End == 0);
    Unchanged<SharpZipBaseException>(window, () => window.Repeat(3, 1));
    window.Write(9);
    window.Repeat(3, 1);
    Equal(Drain(window), [9, 9, 9, 9]);
});
foreach (var length in new[] { 0, 1, 32767, 32768, 32769 })
{
    Add("dictionary.boundary." + length, () =>
    {
        var dictionary = Enumerable.Range(0, length).Select(i => (byte)(i * 17 + 3)).ToArray();
        var window = new OutputWindow();
        window.CopyDict(dictionary, 0, dictionary.Length);
        var kept = Math.Min(length, 32768);
        Check(WindowState.Capture(window).History == kept && window.GetAvailable() == 0);
        Unchanged<SharpZipBaseException>(window, () => window.Repeat(3, kept + 1));
        if (length == 0) { return; }
        window.Repeat(3, kept);
        var start = length - kept;
        Equal(Drain(window), Enumerable.Range(0, 3).Select(i => dictionary[start + i % kept]).ToArray());
    });
}
Add("dictionary.replacement", () =>
{
    var window = new OutputWindow();
    window.CopyDict(new byte[32768], 0, 32768);
    window.CopyDict([91], 0, 1);
    Check(WindowState.Capture(window).History == 1);
    Unchanged<SharpZipBaseException>(window, () => window.Repeat(3, 2));
    window.Repeat(3, 1);
    Equal(Drain(window), [91, 91, 91]);
    window.CopyDict([], 0, 0);
    Check(WindowState.Capture(window).History == 0);
    Unchanged<SharpZipBaseException>(window, () => window.Repeat(3, 1));
});
Add("dictionary.pending.output.reject", () =>
{
    var window = new OutputWindow();
    window.Write(5);
    Unchanged<InvalidOperationException>(window, () => window.CopyDict([7], 0, 1));
});

foreach (var feed in new[] { 1, 2, 7, 65536 })
foreach (var output in new[] { 1, 258, 65536 })
{
    Add($"raw.invalid.history.{feed}.{output}", () =>
    {
        ProbeError("Deflate", () => RawProbe.Run([0x03, 0x02, 0x00], feed, output));
    });
    Add($"raw.empty.fixed.{feed}.{output}", () => Raw([0x03, 0x00], [], feed, output));
    Add($"raw.empty.stored.{feed}.{output}", () => Raw([0x01, 0, 0, 255, 255], [], feed, output));
    Add($"raw.zero.control.{feed}.{output}", () => Raw([1, 3, 0, 252, 255, 0, 0, 0], [0, 0, 0], feed, output));
}
Add("raw.trailing.report", () =>
{
    var result = RawProbe.Run([3, 0, 222, 173, 190, 239]);
    Check(result.Consumed == 2 && result.Remaining == 4 && result.Offered == 6 && result.Decoded == 0);
});
Add("raw.empty.input", () => ProbeError("Truncated", () => RawProbe.Run([])));
Add("raw.truncated.stored", () => ProbeError("Truncated", () => RawProbe.Run([1, 3, 0, 252, 255, 0, 0])));
Add("raw.budget", () => ProbeError("Budget", () => RawProbe.Run([3, 0], maxProcessed: 1)));
Add("raw.decoded.budget", () => ProbeError("Budget",
    () => RawProbe.Run([1, 3, 0, 252, 255, 0, 0, 0], maxDecoded: 2)));
Add("raw.cancelled", () =>
{
    using var cancel = new CancellationTokenSource();
    cancel.Cancel();
    Expect<OperationCanceledException>(() => RawProbe.Run([3, 0], token: cancel.Token));
});
Add("raw.no.dictionary.injection", () =>
{
    var inflater = new Inflater(true);
    Expect<InvalidOperationException>(() => inflater.SetDictionary([1, 2, 3]));
});

var failed = 0;
foreach (var test in tests)
{
    try { test.Run(); Console.WriteLine("PASS " + test.Name); }
    catch (Exception error) { failed++; Console.WriteLine("FAIL " + test.Name + " " + error.GetType().Name + " " + error.Message); }
}
Console.WriteLine($"total={tests.Count} failed={failed}");
return failed == 0 ? 0 : 1;

static void Raw(byte[] input, byte[] expected, int feed, int output)
{
    var result = RawProbe.Run(input, feed, output);
    Check(result.Consumed == input.Length && result.Remaining == 0 && result.Offered == input.Length);
    Check(result.Decoded == expected.Length && result.Sha256 == Convert.ToHexString(SHA256.HashData(expected)));
}

static void ProbeError(string code, Action action)
{
    try { action(); }
    catch (ProbeFailure error) when (error.Code == code) { return; }
    throw new Exception("Expected " + code);
}

static void Expect<T>(Action action) where T : Exception
{
    try { action(); }
    catch (T error) when (error.GetType() == typeof(T)) { return; }
    throw new Exception("Expected " + typeof(T).Name);
}

static void Unchanged<T>(OutputWindow window, Action action) where T : Exception
{
    var before = WindowState.Capture(window);
    Expect<T>(action);
    before.RequireSame(window);
}

static byte[] Drain(OutputWindow window)
{
    var bytes = new byte[window.GetAvailable()];
    Check(window.CopyOutput(bytes, 0, bytes.Length) == bytes.Length);
    return bytes;
}

static void Equal(byte[] actual, byte[] expected) => Check(actual.AsSpan().SequenceEqual(expected));
static void Check(bool condition) { if (!condition) { throw new Exception("Assertion failed"); } }

// 수정본 윈도 상태만 관찰함. inflater 입력과 전체 상태의 롤백은 주장하지 않음.
internal sealed record WindowState(int End, int Filled, int History, byte[] Bytes)
{
    internal static WindowState Capture(OutputWindow window)
    {
        object Read(string name) => typeof(OutputWindow).GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)!
            .GetValue(window)!;
        return new((int)Read("windowEnd"), (int)Read("windowFilled"), (int)Read("historyLength"),
            (byte[])((byte[])Read("window")).Clone());
    }

    internal void RequireSame(OutputWindow window)
    {
        var next = Capture(window);
        if (End != next.End || Filled != next.Filled || History != next.History ||
            !Bytes.AsSpan().SequenceEqual(next.Bytes))
        {
            throw new Exception("OutputWindow changed after rejection");
        }
    }
}

using System.Formats.Tar;
using System.Security.Cryptography;
using System.Text;
using Zipelo.Core;
using Zipelo.Worker.Tar;

var tests = new List<(string Name, Func<Task> Run)>();
var normal = Fixtures.Archive(new Fixture("folder/", [], '5'), new Fixture("folder/a.txt", "alpha"u8.ToArray()),
    new Fixture("empty", []), new Fixture("b.bin", Enumerable.Range(0, 140000).Select(i => (byte)i).ToArray()));
var defaults = new UstarLimits(new JobLimits(1024, 128L << 20, 16L << 20, TimeSpan.FromSeconds(15), 1), 16L << 20, 1L << 20);

tests.Add(("list", async () =>
{
    using var source = new MemoryStream(normal, false);
    using var sink = new HashSink();
    var result = await new UstarReader(defaults, FileNamePolicy.Linux).ReadAsync(source, _ => [], sink.Write, default);
    Check(result.Entries.Count == 4 && result.Entries[0].Name == "folder" && result.Usage.Entries == 4);
    Check(result.Usage.ProcessedBytes == normal.Length && result.Usage.OutputBytes == 0 && sink.Calls == 0);
    Check(result.ContentVerification == VerificationState.Unavailable);
    Check(result.InputSha256 == Convert.ToHexString(SHA256.HashData(normal)) && source.CanRead);
}));
tests.Add(("selected.hash.chunks", async () =>
{
    using var source = new FragmentedStream(normal);
    using var sink = new HashSink();
    var result = await new UstarReader(defaults, FileNamePolicy.Linux).ReadAsync(source, _ => ["2", "3"], sink.Write, default);
    Check(sink.Counts["2"] == 0 && sink.Counts["3"] == 140000 && !sink.Counts.ContainsKey("1"));
    Check(sink.Digest("3") == Convert.ToHexString(SHA256.HashData(Enumerable.Range(0, 140000).Select(i => (byte)i).ToArray())));
    Check(sink.MaxChunk == UstarReader.ChunkSize && result.Usage.ProcessedBytes == normal.Length * 2L);
    Check(result.Usage.OutputBytes == 140000 && result.Usage.Entries == 4);
}));
tests.Add(("bcl.ustar.prefix", async () =>
{
    using var archive = new MemoryStream();
    var prefix = new string('p', 80) + "/" + new string('q', 60);
    using (var writer = new TarWriter(archive, TarEntryFormat.Ustar, leaveOpen: true))
    {
        var entry = new UstarTarEntry(TarEntryType.RegularFile, prefix + "/a") { DataStream = new MemoryStream("ok"u8.ToArray()) };
        writer.WriteEntry(entry);
    }
    using var sink = new HashSink();
    var result = await new UstarReader(defaults, FileNamePolicy.Linux).ReadAsync(archive, _ => ["0"], sink.Write, default);
    Check(result.Entries[0].Name == prefix + "/a" && sink.Counts["0"] == 2);
}));
tests.Add(("empty.archive", async () => { await Run(new byte[1024]); }));
tests.Add(("zero.trailing.blocks", async () => { await Run([.. normal, .. new byte[1024]]); }));
tests.Add(("directory.after.child", async () => { await Run(Fixtures.Archive(new Fixture("a/b", []), new Fixture("a/", [], '5'))); }));
tests.Add(("linux.case.distinct", async () => { await Run(Fixtures.Archive(new Fixture("A", []), new Fixture("a", []))); }));
tests.Add(("header.checksum.not.content", async () =>
{
    var bytes = Fixtures.Archive(new Fixture("a", "one"u8.ToArray()));
    bytes[512] ^= 1;
    var result = await Run(bytes);
    Check(result.ContentVerification == VerificationState.Unavailable);
}));

Reject("checksum", Mutate(normal, 0, 1), ArchiveError.IntegrityFailure, checksum: false);
Reject("truncated.header", normal[..511], ArchiveError.IntegrityFailure, checksum: false);
Reject("truncated.body", Fixtures.Archive(new Fixture("a", new byte[1024]))[..1024], ArchiveError.IntegrityFailure, checksum: false);
Reject("end.missing", Fixtures.Archive(new Fixture("a", []))[..512], ArchiveError.IntegrityFailure, checksum: false);
Reject("end.single", Fixtures.Archive(new Fixture("a", []))[..1024], ArchiveError.IntegrityFailure, checksum: false);
Reject("end.second.nonzero", Mutate(new byte[1024], 512, 1), ArchiveError.IntegrityFailure, checksum: false);
Reject("trailing.nonzero", [.. normal, .. Enumerable.Repeat((byte)1, 512)], ArchiveError.IntegrityFailure, checksum: false);
Reject("trailing.partial", [.. normal, (byte)0], ArchiveError.IntegrityFailure, checksum: false);
Reject("padding.nonzero", Mutate(Fixtures.Archive(new Fixture("a", [1])), 513, 1), ArchiveError.IntegrityFailure, checksum: false);
Reject("size.huge.octal", Field(Fixtures.Archive(new Fixture("a", [])), 124, "77777777777\0"), ArchiveError.IntegrityFailure);
Reject("size.base256", Mutate(normal, 124, 128), ArchiveError.IntegrityFailure);
Reject("size.negative", Field(normal, 124, "-0000000001\0"), ArchiveError.IntegrityFailure);
Reject("size.invalid.digit", Mutate(normal, 124, (byte)'8'), ArchiveError.IntegrityFailure);
Reject("size.internal.terminator", Field(normal, 124, "0\00000000001"), ArchiveError.IntegrityFailure);
Reject("directory.nonzero", Field(normal, 124, "00000000001\0"), ArchiveError.IntegrityFailure);
Reject("version", Mutate(normal, 264, (byte)'1'), ArchiveError.UnsupportedEntry);
Reject("gnu.magic", Mutate(normal, 262, (byte)' '), ArchiveError.UnsupportedEntry);
Reject("linkname.on.file", Mutate(Fixtures.Archive(new Fixture("a", [])), 157, (byte)'x'), ArchiveError.IntegrityFailure);
Reject("reserved.bytes", Mutate(normal, 500, 1), ArchiveError.IntegrityFailure);
Reject("name.hidden.after.nul", Mutate(Fixtures.Archive(new Fixture("a", [])), 2, (byte)'b'), ArchiveError.IntegrityFailure);
Reject("invalid.utf8", Mutate(Fixtures.Archive(new Fixture("a", [])), 0, 255), ArchiveError.IntegrityFailure);
foreach (var type in new[] { 'x', 'g', 'L', 'K', 'S', '1', '2', '3', '4', '6', '7', 'V', '?' })
{
    Reject("type." + type, Fixtures.Archive(new Fixture("a", [], type)), ArchiveError.UnsupportedEntry);
}
foreach (var name in new[] { "../a", "/root", "a//b", "a/./b", "a/../b", "a\\b", "" })
{
    Reject("path." + name, Fixtures.Archive(new Fixture(name, [])), name.Length == 0 ? ArchiveError.IntegrityFailure : ArchiveError.UnsafePath);
}
foreach (var entries in new[]
{
    new[] { new Fixture("a", []), new Fixture("a", []) },
    new[] { new Fixture("a/", [], '5'), new Fixture("a", []) },
    new[] { new Fixture("a", []), new Fixture("a/b", []) },
    new[] { new Fixture("a/b", []), new Fixture("a", []) }
})
{
    Reject("collision." + tests.Count, Fixtures.Archive(entries), ArchiveError.OutputConflict);
}
Reject("windows.case.collision", Fixtures.Archive(new Fixture("A", []), new Fixture("a", [])), ArchiveError.OutputConflict, policy: FileNamePolicy.Windows);
Reject("windows.device", Fixtures.Archive(new Fixture("CON.txt", [])), ArchiveError.UnsafePath, policy: FileNamePolicy.Windows);
Reject("windows.drive", Fixtures.Archive(new Fixture("C:/a", [])), ArchiveError.UnsafePath, policy: FileNamePolicy.Windows);
Reject("unselected.unsafe", Fixtures.Archive(new Fixture("ok", [1]), new Fixture("../bad", [])), ArchiveError.UnsafePath, selection: _ => ["0"]);
Reject("input.limit", normal, ArchiveError.BudgetExceeded, limits: defaults with { MaxInputBytes = normal.Length - 1 });
Reject("entries.limit", normal, ArchiveError.BudgetExceeded, limits: defaults with { Job = defaults.Job with { MaxEntries = 3 } });
Reject("metadata.limit", normal, ArchiveError.BudgetExceeded, limits: defaults with { MaxMetadataBytes = 2047 });
Reject("processed.first.pass", normal, ArchiveError.BudgetExceeded, limits: defaults with { Job = defaults.Job with { MaxProcessedBytes = normal.Length - 1 } });
Reject("processed.second.pass.cumulative", normal, ArchiveError.BudgetExceeded,
    limits: defaults with { Job = defaults.Job with { MaxProcessedBytes = normal.Length * 2L - 1 } }, expectNoOutput: false);
Reject("output.limit", normal, ArchiveError.BudgetExceeded, limits: defaults with { Job = defaults.Job with { MaxOutputBytes = 4 } });
Reject("entries.hard.cap", Fixtures.Archive(Enumerable.Range(0, 1025).Select(i => new Fixture("f" + i, [])).ToArray()),
    ArchiveError.BudgetExceeded, limits: defaults with { Job = defaults.Job with { MaxEntries = long.MaxValue } });
Reject("input.hard.cap", new byte[(16 << 20) + 512], ArchiveError.BudgetExceeded,
    limits: defaults with { MaxInputBytes = long.MaxValue });
Reject("selection.duplicate", normal, ArchiveError.UnsupportedEntry, selection: _ => ["1", "1"]);
Reject("selection.unknown", normal, ArchiveError.UnsupportedEntry, selection: _ => ["99"]);

tests.Add(("cancel.before.read", async () =>
{
    using var cts = new CancellationTokenSource();
    cts.Cancel();
    await Cancelled(() => Run(normal, token: cts.Token));
}));
tests.Add(("cancel.during.read", async () =>
{
    using var cts = new CancellationTokenSource();
    using var source = new FragmentedStream(normal, () => cts.Cancel());
    using var sink = new HashSink();
    await Cancelled(() => new UstarReader(defaults, FileNamePolicy.Linux).ReadAsync(source, _ => [], sink.Write, cts.Token));
    Check(sink.Calls == 0);
}));
tests.Add(("cancel.sink", async () =>
{
    using var cts = new CancellationTokenSource();
    using var source = new MemoryStream(normal, false);
    await Cancelled(() => new UstarReader(defaults, FileNamePolicy.Linux).ReadAsync(source, _ => ["3"], (_, _, _) =>
    {
        cts.Cancel();
        return ValueTask.CompletedTask;
    }, cts.Token));
}));
tests.Add(("duration.cumulative", async () =>
{
    var time = new TestTime();
    using var source = new MemoryStream(normal, false);
    using var sink = new HashSink();
    await Error(ArchiveError.BudgetExceeded, () => new UstarReader(defaults, FileNamePolicy.Linux, time).ReadAsync(source, _ =>
    {
        time.Ticks = TimeSpan.FromSeconds(15).Ticks;
        return ["1"];
    }, sink.Write, default));
    Check(sink.Calls == 0);
}));
tests.Add(("input.changed.between.passes", async () =>
{
    var bytes = Fixtures.Archive(new Fixture("a", "old"u8.ToArray()));
    using var source = new MemoryStream(bytes, false);
    using var sink = new HashSink();
    await Error(ArchiveError.IntegrityFailure, () => new UstarReader(defaults, FileNamePolicy.Linux).ReadAsync(source, _ =>
    {
        bytes[512] ^= 1;
        return ["0"];
    }, sink.Write, default));
    Check(sink.Calls > 0);
}));
tests.Add(("input.unselected.changed", async () =>
{
    var bytes = Fixtures.Archive(new Fixture("a", [1]), new Fixture("b", [2]));
    using var source = new MemoryStream(bytes, false);
    using var sink = new HashSink();
    await Error(ArchiveError.IntegrityFailure, () => new UstarReader(defaults, FileNamePolicy.Linux).ReadAsync(source, _ =>
    {
        bytes[1536] ^= 1;
        return ["0"];
    }, sink.Write, default));
}));
tests.Add(("single.use", async () =>
{
    var reader = new UstarReader(defaults, FileNamePolicy.Linux);
    using var source = new MemoryStream(normal, false);
    using var sink = new HashSink();
    await reader.ReadAsync(source, _ => [], sink.Write, default);
    try { await reader.ReadAsync(source, _ => [], sink.Write, default); }
    catch (InvalidOperationException) { return; }
    throw new Exception("Reuse accepted.");
}));

tests.Add(("metadata.exact.boundary", async () =>
{
    using var source = new MemoryStream(normal, false);
    using var sink = new HashSink();
    var result = await new UstarReader(defaults with { MaxMetadataBytes = 2048 }, FileNamePolicy.Linux)
        .ReadAsync(source, _ => ["0"], sink.Write, default);
    Check(result.Entries.Count == 4 && sink.Calls == 0);
}));
tests.Add(("input.limit.exact", async () =>
{
    using var source = new MemoryStream(normal, false);
    using var sink = new HashSink();
    await new UstarReader(defaults with { MaxInputBytes = normal.Length }, FileNamePolicy.Linux)
        .ReadAsync(source, _ => [], sink.Write, default);
}));
tests.Add(("sink.failure.propagates", async () =>
{
    using var source = new MemoryStream(normal, false);
    var expected = new IOException("Test sink failure.");
    try
    {
        await new UstarReader(defaults, FileNamePolicy.Linux).ReadAsync(source, _ => ["1"],
            (_, _, _) => throw expected, default);
    }
    catch (IOException error) when (ReferenceEquals(error, expected)) { Check(source.CanRead); return; }
    throw new Exception("Sink failure suppressed.");
}));
tests.Add(("nonseekable.rejected", async () =>
{
    using var source = new NonSeekableStream(normal);
    using var sink = new HashSink();
    try { await new UstarReader(defaults, FileNamePolicy.Linux).ReadAsync(source, _ => [], sink.Write, default); }
    catch (ArgumentException) { Check(sink.Calls == 0); return; }
    throw new Exception("Nonseekable input accepted.");
}));
tests.Add(("invalid.limits", () =>
{
    foreach (var limits in new[]
    {
        defaults with { MaxMetadataBytes = 0 }, defaults with { MaxInputBytes = 0 },
        defaults with { Job = defaults.Job with { MaxEntries = 0 } },
        defaults with { Job = defaults.Job with { MaxDuration = TimeSpan.Zero } }
    })
    {
        try { _ = new UstarReader(limits, FileNamePolicy.Linux); }
        catch (ArgumentOutOfRangeException) { continue; }
        throw new Exception("Invalid limits accepted.");
    }
    return Task.CompletedTask;
}));

var failures = 0;
foreach (var test in tests)
{
    try { await test.Run(); Console.WriteLine("PASS " + test.Name); }
    catch (Exception error) { failures++; Console.WriteLine("FAIL " + test.Name + " " + error); }
}
Console.WriteLine($"total={tests.Count} failed={failures}");
return failures == 0 ? 0 : 1;

void Reject(string name, byte[] bytes, ArchiveError code, bool checksum = true, UstarLimits? limits = null,
    FileNamePolicy policy = FileNamePolicy.Linux, Func<IReadOnlyList<ArchiveEntry>, IReadOnlyCollection<string>>? selection = null,
    bool expectNoOutput = true)
{
    if (checksum && bytes.Length >= 512) { Fixtures.Checksum(bytes.AsSpan(0, 512)); }
    tests.Add((name, async () =>
    {
        using var source = new MemoryStream(bytes, false);
        using var sink = new HashSink();
        await Error(code, () => new UstarReader(limits ?? defaults, policy).ReadAsync(source,
            selection ?? (entries => entries.Where(e => e.Kind == EntryKind.File).Select(e => e.Id).ToArray()), sink.Write, default));
        if (expectNoOutput) { Check(sink.Calls == 0); }
    }));
}

async Task<UstarResult> Run(byte[] bytes, CancellationToken token = default)
{
    using var source = new MemoryStream(bytes, false);
    using var sink = new HashSink();
    return await new UstarReader(defaults, FileNamePolicy.Linux).ReadAsync(source, _ => [], sink.Write, token);
}

static async Task Error(ArchiveError expected, Func<Task> action)
{
    try { await action(); }
    catch (ArchiveException error) when (error.Code == expected) { return; }
    throw new Exception("Expected " + expected);
}

static async Task Cancelled(Func<Task> action)
{
    try { await action(); }
    catch (OperationCanceledException) { return; }
    throw new Exception("Expected cancellation.");
}

static void Check(bool condition) { if (!condition) { throw new Exception("Assertion failed."); } }
static byte[] Mutate(byte[] bytes, int index, byte value) { var copy = (byte[])bytes.Clone(); copy[index] = value; return copy; }
static byte[] Field(byte[] bytes, int offset, string value)
{
    var copy = (byte[])bytes.Clone();
    Encoding.ASCII.GetBytes(value).CopyTo(copy, offset);
    return copy;
}

internal sealed record Fixture(string Name, byte[] Data, char Type = '0');

internal static class Fixtures
{
    internal static byte[] Archive(params Fixture[] entries)
    {
        using var stream = new MemoryStream();
        foreach (var entry in entries)
        {
            var header = new byte[512];
            Encoding.UTF8.GetBytes(entry.Name).CopyTo(header, 0);
            Number(header, 100, 8, 420);
            Number(header, 108, 8, 0);
            Number(header, 116, 8, 0);
            Number(header, 124, 12, entry.Data.LongLength);
            Number(header, 136, 12, 0);
            header[156] = (byte)entry.Type;
            "ustar\000"u8.CopyTo(header.AsSpan(257));
            Checksum(header);
            stream.Write(header);
            stream.Write(entry.Data);
            stream.Write(new byte[(512 - entry.Data.Length % 512) % 512]);
        }
        stream.Write(new byte[1024]);
        return stream.ToArray();
    }

    internal static void Checksum(Span<byte> header)
    {
        header.Slice(148, 8).Fill(32);
        var sum = 0;
        foreach (var b in header) { sum += b; }
        Encoding.ASCII.GetBytes(Convert.ToString(sum, 8).PadLeft(6, '0')).CopyTo(header.Slice(148, 6));
        header[154] = 0;
        header[155] = 32;
    }

    private static void Number(byte[] bytes, int offset, int width, long value) =>
        Encoding.ASCII.GetBytes(Convert.ToString(value, 8).PadLeft(width - 1, '0') + "\0").CopyTo(bytes, offset);
}

internal sealed class HashSink : IDisposable
{
    private readonly Dictionary<string, IncrementalHash> hashes = new();
    internal Dictionary<string, long> Counts { get; } = new();
    internal int Calls { get; private set; }
    internal int MaxChunk { get; private set; }

    internal ValueTask Write(ArchiveEntry entry, ReadOnlyMemory<byte> bytes, CancellationToken token)
    {
        token.ThrowIfCancellationRequested();
        if (!hashes.TryGetValue(entry.Id, out var hash))
        {
            hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
            hashes.Add(entry.Id, hash);
            Counts.Add(entry.Id, 0);
        }
        hash.AppendData(bytes.Span);
        Counts[entry.Id] += bytes.Length;
        Calls++;
        MaxChunk = Math.Max(MaxChunk, bytes.Length);
        return ValueTask.CompletedTask;
    }

    internal string Digest(string id) => Convert.ToHexString(hashes[id].GetHashAndReset());
    public void Dispose() { foreach (var hash in hashes.Values) { hash.Dispose(); } }
}

internal sealed class FragmentedStream(byte[] bytes, Action? onRead = null) : MemoryStream(bytes, false)
{
    public override ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken cancellationToken = default)
    {
        onRead?.Invoke();
        return base.ReadAsync(buffer[..Math.Min(buffer.Length, 97)], cancellationToken);
    }
}

internal sealed class TestTime : TimeProvider
{
    internal long Ticks { get; set; }
    public override long TimestampFrequency => TimeSpan.TicksPerSecond;
    public override long GetTimestamp() => Ticks;
}

internal sealed class NonSeekableStream(byte[] bytes) : MemoryStream(bytes, false)
{
    public override bool CanSeek => false;
}

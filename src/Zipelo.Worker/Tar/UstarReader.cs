using System.Globalization;
using System.Security.Cryptography;
using System.Text;
using Zipelo.Core;

namespace Zipelo.Worker.Tar;

internal sealed record UstarLimits(JobLimits Job, long MaxInputBytes, long MaxMetadataBytes);
internal sealed record UstarResult(IReadOnlyList<ArchiveEntry> Entries, BudgetUsage Usage,
    string InputSha256, VerificationState ContentVerification);

// 수신기는 청크를 복사하거나 즉시 소비함. 성공 반환 전에는 게시하지 않으며 실패 시 전부 폐기함.
internal delegate ValueTask UstarSink(ArchiveEntry entry, ReadOnlyMemory<byte> bytes, CancellationToken token);

// 입력은 호출자가 독점 보유한 seek 가능 스트림임. 입력과 수신기를 닫지 않음.
internal sealed class UstarReader
{
    internal const int ChunkSize = 64 * 1024;
    internal const long InputCap = 16 * 1024 * 1024;
    internal const long MetadataCap = 1024 * 1024;
    private const int BlockSize = 512;
    private static readonly UTF8Encoding Utf8 = new(false, true);
    private readonly JobBudget budget;
    private readonly long inputLimit;
    private readonly long metadataLimit;
    private readonly FileNamePolicy policy;
    private int used;

    internal UstarReader(UstarLimits limits, FileNamePolicy policy, TimeProvider? time = null)
    {
        ArgumentNullException.ThrowIfNull(limits);
        ArgumentNullException.ThrowIfNull(limits.Job);
        if (limits.MaxInputBytes <= 0 || limits.MaxMetadataBytes <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(limits));
        }
        _ = OutputPathPolicy.Comparer(policy);
        this.policy = policy;
        inputLimit = Math.Min(limits.MaxInputBytes, InputCap);
        metadataLimit = Math.Min(limits.MaxMetadataBytes, MetadataCap);
        budget = new JobBudget(limits.Job with
        {
            MaxEntries = Math.Min(limits.Job.MaxEntries, 1024),
            MaxProcessedBytes = Math.Min(limits.Job.MaxProcessedBytes, 128L * 1024 * 1024),
            MaxOutputBytes = Math.Min(limits.Job.MaxOutputBytes, 16L * 1024 * 1024),
            MaxDuration = limits.Job.MaxDuration < TimeSpan.FromMinutes(5)
                ? limits.Job.MaxDuration : TimeSpan.FromMinutes(5)
        }, time);
    }

    // 선택은 검증 완료된 목록의 순번 ID로 고정함. 빈 선택은 목록만 반환함.
    internal async Task<UstarResult> ReadAsync(Stream input,
        Func<IReadOnlyList<ArchiveEntry>, IReadOnlyCollection<string>> select,
        UstarSink sink, CancellationToken token)
    {
        ArgumentNullException.ThrowIfNull(input);
        ArgumentNullException.ThrowIfNull(select);
        ArgumentNullException.ThrowIfNull(sink);
        if (Interlocked.Exchange(ref used, 1) != 0) { throw new InvalidOperationException(); }
        budget.Check(token);
        if (!input.CanRead || !input.CanSeek) { throw new ArgumentException("Seekable input required.", nameof(input)); }
        var length = input.Length;
        if (length > inputLimit) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
        if (length < 1024 || length % BlockSize != 0) { Fail(); }

        var entries = new List<ArchiveEntry>();
        var first = await ScanAsync(input, length, entries, null, sink, token).ConfigureAwait(false);
        var inventory = entries.AsReadOnly();
        budget.Check(token);
        var requested = select(inventory) ?? throw new ArgumentException("Selection required.", nameof(select));
        budget.Check(token);
        if (requested.Count > entries.Count) { throw new ArchiveException(ArchiveError.UnsupportedEntry); }
        var selected = new HashSet<string>(StringComparer.Ordinal);
        foreach (var id in requested)
        {
            budget.Check(token);
            if (selected.Count >= entries.Count || !selected.Add(id) || !entries.Any(e => e.Id == id))
            {
                throw new ArchiveException(ArchiveError.UnsupportedEntry);
            }
        }
        if (selected.Count != 0)
        {
            var second = await ScanAsync(input, length, entries, selected, sink, token).ConfigureAwait(false);
            if (!CryptographicOperations.FixedTimeEquals(first, second)) { Fail(); }
        }
        budget.Check(token);
        // SHA256은 두 순회 입력의 동일성 확인용임. TAR에는 본문 무결성 값이 없음.
        return new UstarResult(inventory, budget.Usage, Convert.ToHexString(first), VerificationState.Unavailable);
    }

    private async Task<byte[]> ScanAsync(Stream input, long length, List<ArchiveEntry> entries,
        HashSet<string>? selected, UstarSink sink, CancellationToken token)
    {
        if (input.Length != length) { Fail(); }
        input.Position = 0;
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        var buffer = new byte[ChunkSize];
        var header = new byte[BlockSize];
        var seen = new Dictionary<string, EntryKind>(OutputPathPolicy.Comparer(policy));
        long consumed = 0;
        long metadata = 0;
        var index = 0;
        while (true)
        {
            await ReadExactAsync(header).ConfigureAwait(false);
            if (IsZero(header))
            {
                await ReadExactAsync(header).ConfigureAwait(false);
                if (!IsZero(header)) { Fail(); }
                while (consumed < length)
                {
                    var count = (int)Math.Min(buffer.Length, length - consumed);
                    await ReadExactAsync(buffer.AsMemory(0, count)).ConfigureAwait(false);
                    if (!IsZero(buffer.AsSpan(0, count))) { Fail(); }
                }
                break;
            }
            if (selected is null) { budget.AddEntry(token); }
            if (metadataLimit - metadata < BlockSize) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
            metadata += BlockSize;
            var entry = ParseHeader(header, index);
            RequireUnique(entry, seen);
            if (selected is null) { entries.Add(entry); }
            else if (index >= entries.Count || entries[index] != entry) { Fail(); }
            index++;
            var size = entry.DeclaredSize!.Value;
            var padding = (BlockSize - size % BlockSize) % BlockSize;
            // 감산 비교로 거대한 선언값의 덧셈 오버플로를 피함.
            if (size > length - consumed || padding > length - consumed - size) { Fail(); }
            var emit = selected?.Contains(entry.Id) == true && entry.Kind == EntryKind.File;
            if (emit) { budget.ReserveOutput(size, token); }
            if (emit && size == 0) { await sink(entry, ReadOnlyMemory<byte>.Empty, token).ConfigureAwait(false); }
            for (long remaining = size; remaining > 0;)
            {
                var count = (int)Math.Min(ChunkSize, remaining);
                await ReadExactAsync(buffer.AsMemory(0, count)).ConfigureAwait(false);
                if (emit) { await sink(entry, buffer.AsMemory(0, count), token).ConfigureAwait(false); }
                budget.Check(token);
                remaining -= count;
            }
            if (padding > 0)
            {
                await ReadExactAsync(buffer.AsMemory(0, (int)padding)).ConfigureAwait(false);
                if (!IsZero(buffer.AsSpan(0, (int)padding))) { Fail(); }
            }
        }
        budget.Check(token);
        if (input.Length != length || input.Position != length || (selected is not null && index != entries.Count)) { Fail(); }
        return hash.GetHashAndReset();

        async ValueTask ReadExactAsync(Memory<byte> destination)
        {
            if (destination.Length > length - consumed) { Fail(); }
            while (!destination.IsEmpty)
            {
                budget.Check(token);
                var read = await input.ReadAsync(destination, token).ConfigureAwait(false);
                if (read <= 0 || read > destination.Length) { Fail(); }
                budget.ChargeProcessed(read, token);
                hash.AppendData(destination.Span[..read]);
                consumed += read;
                destination = destination[read..];
            }
        }
    }

    private ArchiveEntry ParseHeader(byte[] header, int index)
    {
        var h = header.AsSpan();
        var stored = Octal(h.Slice(148, 8));
        long sum = 0;
        for (var i = 0; i < BlockSize; i++) { sum += i is >= 148 and < 156 ? 32 : h[i]; }
        if (stored != sum) { Fail(); }
        if (!h.Slice(257, 6).SequenceEqual("ustar\0"u8) || !h.Slice(263, 2).SequenceEqual("00"u8))
        {
            throw new ArchiveException(ArchiveError.UnsupportedEntry);
        }
        var kind = h[156] switch
        {
            0 or (byte)'0' => EntryKind.File,
            (byte)'5' => EntryKind.Directory,
            _ => throw new ArchiveException(ArchiveError.UnsupportedEntry)
        };
        _ = Octal(h.Slice(100, 8));
        _ = Octal(h.Slice(108, 8));
        _ = Octal(h.Slice(116, 8));
        var size = Octal(h.Slice(124, 12));
        _ = Octal(h.Slice(136, 12));
        if (Octal(h.Slice(329, 8), true) != 0 || Octal(h.Slice(337, 8), true) != 0 ||
            !IsZero(h.Slice(157, 100)) || !IsZero(h.Slice(500, 12))) { Fail(); }
        _ = Text(h.Slice(265, 32));
        _ = Text(h.Slice(297, 32));
        var name = Text(h[..100]);
        var prefix = Text(h.Slice(345, 155));
        if (name.Length == 0) { Fail(); }
        if (prefix.Length > 0) { name = prefix + "/" + name; }
        if (kind == EntryKind.Directory && name.EndsWith('/')) { name = name[..^1]; }
        if (kind == EntryKind.Directory && size != 0) { Fail(); }
        if (name.Contains('\\')) { throw new ArchiveException(ArchiveError.UnsafePath); }
        name = string.Join("/", OutputPathPolicy.Validate(name, policy));
        return new ArchiveEntry(index.ToString(CultureInfo.InvariantCulture), name, kind, size);
    }

    private void RequireUnique(ArchiveEntry entry, Dictionary<string, EntryKind> seen)
    {
        if (seen.ContainsKey(entry.Name)) { throw new ArchiveException(ArchiveError.OutputConflict); }
        foreach (var previous in seen)
        {
            if ((previous.Value == EntryKind.File && entry.Name.StartsWith(previous.Key + "/", OutputPathPolicy.Comparison(policy))) ||
                (entry.Kind == EntryKind.File && previous.Key.StartsWith(entry.Name + "/", OutputPathPolicy.Comparison(policy))))
            {
                throw new ArchiveException(ArchiveError.OutputConflict);
            }
        }
        seen.Add(entry.Name, entry.Kind);
    }

    private static string Text(ReadOnlySpan<byte> field)
    {
        var end = field.IndexOf((byte)0);
        if (end >= 0)
        {
            if (!IsZero(field[end..])) { Fail(); }
            field = field[..end];
        }
        try { return Utf8.GetString(field); }
        catch (DecoderFallbackException) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
    }

    private static long Octal(ReadOnlySpan<byte> field, bool allowEmpty = false)
    {
        long value = 0;
        var digits = 0;
        var ended = false;
        foreach (var b in field)
        {
            if (b is 0 or 32)
            {
                if (digits > 0 || b == 0) { ended = true; }
                continue;
            }
            if (ended || b < (byte)'0' || b > (byte)'7') { Fail(); }
            if (value > (long.MaxValue - (b - (byte)'0')) / 8) { Fail(); }
            value = value * 8 + b - (byte)'0';
            digits++;
        }
        if (digits == 0 && !allowEmpty) { Fail(); }
        return value;
    }

    private static bool IsZero(ReadOnlySpan<byte> bytes)
    {
        foreach (var b in bytes) { if (b != 0) { return false; } }
        return true;
    }

    private static void Fail() => throw new ArchiveException(ArchiveError.IntegrityFailure);
}

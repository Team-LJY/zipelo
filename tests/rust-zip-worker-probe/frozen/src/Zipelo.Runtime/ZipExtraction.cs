using Zipelo.Core;

namespace Zipelo.Runtime;

public sealed record ZipExtractedEntry(ArchiveEntry Entry, byte[] Content, uint Crc32);

internal static class ZipExtraction
{
    internal static void ValidatePaths(IReadOnlyList<ArchiveEntry> entries)
    {
        if (entries.Count > WorkerProtocol.MaxEntries) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
        var comparer = OutputPathPolicy.Comparer(OutputPathPolicy.CurrentPolicy);
        var explicitPaths = new HashSet<string>(comparer);
        var nodes = new Dictionary<string, EntryKind>(comparer);
        var ids = new HashSet<string>(StringComparer.Ordinal);
        foreach (var entry in entries)
        {
            if (entry is null || string.IsNullOrEmpty(entry.Id) || !ids.Add(entry.Id) ||
                entry.Kind is not (EntryKind.File or EntryKind.Directory) || entry.DeclaredSize is null or < 0)
            {
                throw new ArchiveException(ArchiveError.UnsupportedEntry);
            }
            if (entry.Name.Length > WorkerProtocol.MaxNameLength) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
            var segments = OutputPathPolicy.Validate(entry.Name, OutputPathPolicy.CurrentPolicy);
            if (segments.Count > 32) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
            var path = string.Join('/', segments);
            if (!explicitPaths.Add(path)) { throw new ArchiveException(ArchiveError.OutputConflict); }
            for (var index = 0; index < segments.Count; index++)
            {
                var prefix = string.Join('/', segments.Take(index + 1));
                var kind = index == segments.Count - 1 ? entry.Kind : EntryKind.Directory;
                if (nodes.TryGetValue(prefix, out var existing) && (existing != kind || kind == EntryKind.File))
                {
                    throw new ArchiveException(ArchiveError.OutputConflict);
                }
                nodes[prefix] = kind;
                if (nodes.Count > 4096) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
            }
        }
    }

    internal static ZipExtractedEntry[] ValidateResult(ZipWorkerResult result, CancellationToken token, bool snapshot)
    {
        if (result.Job.State != JobState.Completed || result.Job.Error != ArchiveError.None ||
            result.CreatedZip is not null || result.ExtractedEntries is null ||
            result.ExtractedEntries.Count != result.Job.Entries.Count)
        {
            throw new ArchiveException(ArchiveError.EngineFailure);
        }
        var items = result.ExtractedEntries.ToArray();
        ValidatePaths(items.Select(item => item.Entry).ToArray());
        long total = 0;
        for (var index = 0; index < items.Length; index++)
        {
            token.ThrowIfCancellationRequested();
            var item = items[index];
            var report = result.Job.Entries[index];
            if (item.Content is null || report.Entry != item.Entry || report.Error != ArchiveError.None ||
                report.Verification != VerificationState.Passed || report.Outcome != EntryOutcome.Staged ||
                item.Entry.DeclaredSize != item.Content.LongLength ||
                (item.Entry.Kind == EntryKind.Directory && item.Content.Length != 0))
            {
                throw new ArchiveException(ArchiveError.EngineFailure);
            }
            total += item.Content.Length;
            if (total > WorkerProtocol.MaxDataBytes) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
            var bytes = snapshot ? item.Content.ToArray() : item.Content;
            var crc = uint.MaxValue;
            for (var offset = 0; offset < bytes.Length; offset += 65536)
            {
                token.ThrowIfCancellationRequested();
                crc = UpdateCrc(crc, bytes.AsSpan(offset, Math.Min(65536, bytes.Length - offset)));
            }
            if (~crc != item.Crc32) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
            items[index] = item with { Content = bytes };
        }
        return items;
    }

    internal static uint UpdateCrc(uint crc, ReadOnlySpan<byte> bytes)
    {
        foreach (var value in bytes)
        {
            crc ^= value;
            for (var bit = 0; bit < 8; bit++) { crc = (crc >> 1) ^ ((crc & 1) != 0 ? 0xEDB88320u : 0u); }
        }
        return crc;
    }
}

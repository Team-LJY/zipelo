using Microsoft.Win32.SafeHandles;
using Zipelo.Core;

namespace Zipelo.Runtime;

internal abstract partial class PendingFolderPublication
{
    internal SafeFileHandle? CreateStreamingEntry(ArchiveEntry entry, CancellationToken token)
    {
        var segments = OutputPathPolicy.Validate(entry.Name, OutputPathPolicy.CurrentPolicy);
        var parent = Root;
        for (var index = 0; index < segments.Count; index++)
        {
            token.ThrowIfCancellationRequested();
            var path = string.Join('/', segments.Take(index + 1));
            var directory = index < segments.Count - 1 || entry.Kind == EntryKind.Directory;
            if (directory && folders.TryGetValue(path, out var existing)) { parent = existing; continue; }
            if (Owned.Count >= 4097) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
            var handle = Create(parent.Handle, segments[index], directory);
            var node = new Node(segments[index], parent, handle, directory, "");
            Owned.Add(node);
            node = new Node(node.Name, parent, handle, directory, Inspect(handle, directory));
            Owned[^1] = node;
            if (directory)
            {
                folders.Add(path, node);
                parent = node;
            }
            else { return handle; }
        }
        return null;
    }

    internal static async Task VerifyStreamingFileAsync(SafeFileHandle handle, long length, uint expectedCrc,
        JobBudget budget, CancellationToken token)
    {
        token.ThrowIfCancellationRequested();
        RandomAccess.FlushToDisk(handle);
        if (RandomAccess.GetLength(handle) != length) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
        var buffer = new byte[65536];
        var crc = uint.MaxValue;
        long offset = 0;
        while (offset < length)
        {
            var count = await RandomAccess.ReadAsync(handle, buffer.AsMemory(0,
                (int)Math.Min(buffer.Length, length - offset)), offset, token).ConfigureAwait(false);
            if (count == 0) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
            budget.ChargeProcessed(count, token);
            crc = ZipExtraction.UpdateCrc(crc, buffer.AsSpan(0, count));
            offset += count;
        }
        if (~crc != expectedCrc) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
        budget.Check(token);
    }
}

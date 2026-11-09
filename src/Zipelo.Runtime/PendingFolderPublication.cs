using Microsoft.Win32.SafeHandles;
using Zipelo.Core;

namespace Zipelo.Runtime;

internal abstract class PendingFolderPublication : IDisposable
{
    protected sealed class Node(string name, Node? parent, SafeFileHandle handle, bool directory, string identity)
    {
        internal string Name { get; } = name;
        internal Node? Parent { get; } = parent;
        internal SafeFileHandle Handle { get; set; } = handle;
        internal bool Directory { get; } = directory;
        internal string Identity { get; } = identity;
    }

    protected readonly List<Node> Owned = [];
    private readonly Dictionary<string, Node> folders = new(OutputPathPolicy.Comparer(OutputPathPolicy.CurrentPolicy));
    internal string TemporaryName { get; } = $".zipelo-{Guid.NewGuid():N}.tmp";
    internal PublicationNativeFailure? CleanupNativeFailure { get; set; }
    protected Node Root => Owned[0];
    protected abstract SafeFileHandle Create(SafeFileHandle parent, string name, bool directory);
    protected abstract string Inspect(SafeFileHandle handle, bool directory);
    protected abstract void VerifyChildren(Node node, IReadOnlyCollection<string> names);
    internal abstract void Initialize();
    internal abstract void Publish(string leaf, CancellationToken token);
    internal abstract bool Abort();
    public abstract void Dispose();

    protected void AddRoot(SafeFileHandle handle)
    {
        // 검사 실패도 호출자의 Abort가 생성 핸들을 정리하도록 먼저 기록함.
        var root = new Node(TemporaryName, null, handle, true, "");
        Owned.Add(root);
        var verified = new Node(TemporaryName, null, handle, true, Inspect(handle, true));
        Owned[0] = verified;
        folders.Add("", verified);
    }

    internal async Task WriteAsync(ZipExtractedEntry item, CancellationToken token)
    {
        var segments = OutputPathPolicy.Validate(item.Entry.Name, OutputPathPolicy.CurrentPolicy);
        var parent = Root;
        for (var index = 0; index < segments.Count; index++)
        {
            token.ThrowIfCancellationRequested();
            var path = string.Join('/', segments.Take(index + 1));
            var directory = index < segments.Count - 1 || item.Entry.Kind == EntryKind.Directory;
            if (directory && folders.TryGetValue(path, out var existing)) { parent = existing; continue; }
            var handle = Create(parent.Handle, segments[index], directory);
            var node = new Node(segments[index], parent, handle, directory, "");
            Owned.Add(node);
            node = new Node(node.Name, parent, handle, directory, Inspect(handle, directory));
            Owned[^1] = node;
            if (directory)
            {
                folders.Add(path, node);
                parent = node;
                continue;
            }
            var buffer = new byte[65536];
            for (var offset = 0; offset < item.Content.Length; offset += buffer.Length)
            {
                token.ThrowIfCancellationRequested();
                var count = Math.Min(buffer.Length, item.Content.Length - offset);
                await RandomAccess.WriteAsync(handle, item.Content.AsMemory(offset, count), offset, token).ConfigureAwait(false);
            }
            RandomAccess.FlushToDisk(handle);
            if (RandomAccess.GetLength(handle) != item.Content.Length) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
            for (var offset = 0; offset < item.Content.Length;)
            {
                var count = await RandomAccess.ReadAsync(handle, buffer.AsMemory(0,
                    Math.Min(buffer.Length, item.Content.Length - offset)), offset, token).ConfigureAwait(false);
                if (count == 0 || !buffer.AsSpan(0, count).SequenceEqual(item.Content.AsSpan(offset, count)))
                {
                    throw new ArchiveException(ArchiveError.IntegrityFailure);
                }
                offset += count;
            }
        }
    }

    internal void Verify(CancellationToken token)
    {
        foreach (var node in Owned)
        {
            token.ThrowIfCancellationRequested();
            if (Inspect(node.Handle, node.Directory) != node.Identity) { throw new ArchiveException(ArchiveError.UnsafePath); }
            if (node.Directory)
            {
                VerifyChildren(node, Owned.Where(child => child.Parent == node).Select(child => child.Name).ToArray());
            }
        }
    }

    protected void DisposeOwned()
    {
        for (var index = Owned.Count - 1; index >= 0; index--) { Owned[index].Handle.Dispose(); }
    }
}

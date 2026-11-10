using Microsoft.Win32.SafeHandles;
using Zipelo.Core;

namespace Zipelo.Runtime;

internal sealed class WindowsFolderPublication(string parent) : PendingFolderPublication
{
    private readonly List<SafeFileHandle> ancestors = WindowsPublication.OpenParent(parent, readSecurity: true);

#if ZIPELO_STREAM_DIAGNOSTICS
    internal Guid DiagnosticJobId { get; set; }

    // v2가 지정한 작업만 관찰하며 파일 이름과 핸들 값은 기록하지 않음.
    private void ObserveAbort(string stage, int index)
    {
        if (DiagnosticJobId == Guid.Empty) { return; }
        var node = Owned[index];
        var state = (node.Directory ? 1 : 0) | (node.Handle.IsClosed ? 2 : 0) |
            (node.Handle.IsInvalid ? 4 : 0);
        Streaming.StreamingExtractionDiagnostics.Record(DiagnosticJobId, stage, state, (ulong)index);
    }
#endif

    internal override void Initialize()
    {
        WindowsFolderGuard.RequireNtfs(ancestors[^1]);
        WindowsFolderGuard.RequirePrivateWriters(ancestors[^1]);
        AddRoot(Create(ancestors[^1], TemporaryName, true));
    }

    protected override SafeFileHandle Create(SafeFileHandle directory, string name, bool folder)
        => WindowsPublication.OpenRelative(directory, name, create: true, directory: folder, read: !folder);

    protected override string Inspect(SafeFileHandle handle, bool directory)
        => WindowsFolderGuard.Inspect(handle, directory);

    protected override void VerifyChildren(Node node, IReadOnlyCollection<string> names)
        => WindowsFolderGuard.VerifyChildren(node.Handle, names);

    internal override void Publish(string leaf, CancellationToken token)
    {
        // NTFS는 열린 자식 파일을 가진 디렉터리 rename을 허용하지 않음.
        // ACL 검증으로 다른 계정의 쓰기를 배제한 트리에서만 자식 핸들을 닫음.
        for (var index = Owned.Count - 1; index > 0; index--)
        {
            token.ThrowIfCancellationRequested();
            Owned[index].Handle.Dispose();
        }
        token.ThrowIfCancellationRequested();
        WindowsPublication.Rename(Root.Handle, ancestors[^1], leaf);
    }

    private SafeFileHandle Reopen(Node node)
    {
        if (!node.Handle.IsClosed) { return node.Handle; }
        if (node.Parent is null) { throw new ArchiveException(ArchiveError.UnsafePath); }
        var handle = WindowsPublication.OpenRelative(Reopen(node.Parent), node.Name,
            create: false, directory: node.Directory, reopenOwned: true);
        try
        {
            if (Inspect(handle, node.Directory) != node.Identity) { throw new ArchiveException(ArchiveError.UnsafePath); }
            node.Handle = handle;
            return handle;
        }
        catch { handle.Dispose(); throw; }
    }

    internal override bool Abort()
    {
        var success = true;
        // 삭제 대상은 소유 핸들 또는 같은 파일 ID로 재확인한 핸들뿐임.
        for (var index = Owned.Count - 1; index >= 0; index--)
        {
            try
            {
#if ZIPELO_STREAM_DIAGNOSTICS
                ObserveAbort("windows.abort.reopen.begin", index);
                var cleanupHandle = Reopen(Owned[index]);
                ObserveAbort("windows.abort.reopen.end", index);
                ObserveAbort("windows.abort.delete.begin", index);
                var deleted = WindowsPublication.DeleteOwned(cleanupHandle, out var failure, DiagnosticJobId, index);
                ObserveAbort("windows.abort.delete.end", index);
                if (DiagnosticJobId != Guid.Empty)
                {
                    Streaming.StreamingExtractionDiagnostics.Record(DiagnosticJobId,
                        "windows.abort.delete.result", deleted ? 1 : 0, (ulong)index);
                }
                if (!deleted)
#else
                if (!WindowsPublication.DeleteOwned(Reopen(Owned[index]), out var failure))
#endif
                {
                    success = false;
                    CleanupNativeFailure ??= failure;
                }
            }
            catch (PublicationNativeException failure) { success = false; CleanupNativeFailure ??= failure.Failure; }
            catch { success = false; }
            finally
            {
#if ZIPELO_STREAM_DIAGNOSTICS
                ObserveAbort("windows.abort.dispose.begin", index);
#endif
                Owned[index].Handle.Dispose();
#if ZIPELO_STREAM_DIAGNOSTICS
                ObserveAbort("windows.abort.dispose.end", index);
#endif
            }
        }
        return success;
    }

    public override void Dispose()
    {
        DisposeOwned();
        for (var index = ancestors.Count - 1; index >= 0; index--) { ancestors[index].Dispose(); }
    }
}

using System.Globalization;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;
using Zipelo.Core;

namespace Zipelo.Runtime;

internal sealed class LinuxFolderPublication : PendingFolderPublication
{
    private readonly SafeFileHandle parent;
    private readonly uint user = GetEffectiveUser();
    private string parentIdentity = "";
    private bool unboundCreation;
    internal int? NativeError { get; private set; }
    internal int? CleanupError { get; private set; }

    internal LinuxFolderPublication(string path) => parent = Open(-100, path, 0x90000, 0, 0x04);

    internal override void Initialize()
    {
        RequireFileSystem();
        parentIdentity = Inspect(parent, true);
        AddRoot(Create(parent, TemporaryName, true));
    }

    private void RequireFileSystem()
    {
        var magic = FileSystemMagic(parent);
        var expectedType = magic switch
        {
            0xEF53 => "ext4",
            0x58465342 => "xfs",
            _ => throw new ArchiveException(ArchiveError.OutputUnavailable)
        };
        var before = MountStatus();
        var mountId = BitConverter.ToUInt64(before, 144);
        var device = FormattableString.Invariant($"{BitConverter.ToUInt32(before, 136)}:{BitConverter.ToUInt32(before, 140)}");

        // procfs 핸들을 고정하고 하위 bind mount와 링크를 통한 정보 대체를 거부함.
        using var proc = Open(-100, "/proc", 0x90000, 0, 0x04);
        if (FileSystemMagic(proc) != 0x9FA0) { throw new ArchiveException(ArchiveError.OutputUnavailable); }
        var name = FormattableString.Invariant($"{GetProcessId()}/task/{GetThreadId()}/mountinfo");
        using var info = Open(proc.DangerousGetHandle().ToInt32(), name, 0x80000, 0, 0x0F);
        if (FileSystemMagic(info) != 0x9FA0) { throw new ArchiveException(ArchiveError.OutputUnavailable); }
        using var stream = new FileStream(info, FileAccess.Read);
        using var bytes = new MemoryStream();
        var buffer = new byte[8192];
        int count;
        while ((count = stream.Read(buffer, 0, buffer.Length)) != 0)
        {
            if (bytes.Length + count > 4 * 1024 * 1024) { throw new ArchiveException(ArchiveError.OutputUnavailable); }
            bytes.Write(buffer, 0, count);
        }
        var content = new UTF8Encoding(false, true).GetString(bytes.ToArray());
        var matches = 0;
        foreach (var line in content.Split('\n', StringSplitOptions.RemoveEmptyEntries))
        {
            var fields = line.Split(' ', StringSplitOptions.RemoveEmptyEntries);
            if (!ulong.TryParse(fields[0], NumberStyles.None, CultureInfo.InvariantCulture, out var id))
            {
                throw new ArchiveException(ArchiveError.OutputUnavailable);
            }
            if (id != mountId) { continue; }
            var separator = Array.IndexOf(fields, "-");
            if (++matches != 1 || separator < 6 || fields.Length != separator + 4 ||
                fields[2] != device || fields[separator + 1] != expectedType)
            {
                throw new ArchiveException(ArchiveError.OutputUnavailable);
            }
        }
        var after = MountStatus();
        if (matches != 1 || BitConverter.ToUInt64(after, 144) != mountId ||
            Identity(before) != Identity(after) || FileSystemMagic(parent) != magic)
        {
            throw new ArchiveException(ArchiveError.OutputUnavailable);
        }
    }

    private byte[] MountStatus()
    {
        // mountinfo의 기존 mount ID와 대조하므로 UNIQUE 플래그를 요청하지 않음.
        var status = Stat(parent, "", 0x1100, 0x17FF);
        if ((BitConverter.ToUInt32(status, 0) & 0x1000) == 0 || BitConverter.ToUInt64(status, 144) == 0)
        {
            throw new ArchiveException(ArchiveError.OutputUnavailable);
        }
        return status;
    }

    private long FileSystemMagic(SafeFileHandle handle)
    {
        var system = new byte[256];
        if (Fstatfs(handle, system) != 0) { ThrowLast(); }
        return BitConverter.ToInt64(system, 0);
    }

    protected override SafeFileHandle Create(SafeFileHandle directory, string name, bool folder)
    {
        if (folder)
        {
            // 기존 권한은 변경하지 않고 새 디렉터리를 0700 이하로 생성함.
            if (Mkdirat(directory, name, 0x1C0) != 0) { ThrowLast(); }
            unboundCreation = true;
            var handle = Open(directory.DangerousGetHandle().ToInt32(), name, 0x90000, 0, 0x0F);
            unboundCreation = false;
            return handle;
        }
        return Open(directory.DangerousGetHandle().ToInt32(), name, 0x800C2, 0x180, 0x0F);
    }

    protected override string Inspect(SafeFileHandle handle, bool directory)
    {
        var status = Stat(handle, "", 0x1100);
        var mode = BitConverter.ToUInt16(status, 28);
        if ((mode & 0xF000) != (directory ? 0x4000 : 0x8000) ||
            BitConverter.ToUInt32(status, 20) != user || (mode & 0x12) != 0 ||
            (!directory && BitConverter.ToUInt32(status, 16) != 1))
        {
            throw new ArchiveException(ArchiveError.OutputUnavailable);
        }
        return Identity(status);
    }

    private static string Identity(byte[] status)
        => $"{BitConverter.ToUInt32(status, 136):X8}:{BitConverter.ToUInt32(status, 140):X8}:{BitConverter.ToUInt64(status, 32):X16}";

    private byte[] Stat(SafeFileHandle directory, string name, int flags, uint mask = 0x7FF)
    {
        var status = new byte[256];
        if (Statx(directory, name, flags, mask, status) != 0) { ThrowLast(); }
        if ((BitConverter.ToUInt32(status, 0) & 0x10F) != 0x10F)
        {
            throw new ArchiveException(ArchiveError.OutputUnavailable);
        }
        return status;
    }

    protected override void VerifyChildren(Node node, IReadOnlyCollection<string> names)
    {
        using var enumeration = Open(node.Handle.DangerousGetHandle().ToInt32(), ".", 0x90000, 0, 0x0F);
        var buffer = new byte[65536];
        var remaining = new HashSet<string>(names, StringComparer.Ordinal);
        while (true)
        {
            var count = Getdents64(217, enumeration, buffer, (uint)buffer.Length);
            if (count < 0) { ThrowLast(); }
            if (count == 0) { break; }
            for (var offset = 0; offset < count;)
            {
                if (count - offset < 20) { throw new ArchiveException(ArchiveError.OutputUnavailable); }
                var size = BitConverter.ToUInt16(buffer, offset + 16);
                if (size < 20 || size > count - offset) { throw new ArchiveException(ArchiveError.OutputUnavailable); }
                var end = Array.IndexOf(buffer, (byte)0, offset + 19, size - 19);
                if (end < 0) { throw new ArchiveException(ArchiveError.OutputUnavailable); }
                var name = new UTF8Encoding(false, true).GetString(buffer, offset + 19, end - offset - 19);
                if (name is not ("." or "..") && !remaining.Remove(name)) { throw new ArchiveException(ArchiveError.UnsafePath); }
                offset += size;
            }
        }
        if (remaining.Count != 0) { throw new ArchiveException(ArchiveError.UnsafePath); }
    }

    private void RequireName(Node node)
    {
        var container = node.Parent?.Handle ?? parent;
        if (Inspect(container, true) != (node.Parent?.Identity ?? parentIdentity) ||
            Identity(Stat(container, node.Name, 0x100)) != node.Identity)
        {
            throw new ArchiveException(ArchiveError.UnsafePath);
        }
    }

    internal override void Publish(string leaf, CancellationToken token)
    {
        RequireName(Root);
        foreach (var node in Owned.Where(node => node.Directory))
        {
            token.ThrowIfCancellationRequested();
            if (Fsync(node.Handle) != 0) { ThrowLast(); }
        }
        token.ThrowIfCancellationRequested();
        // RENAME_NOREPLACE가 없는 커널/FS에서는 대체 경로 없이 실패함.
        if (Renameat2(parent, TemporaryName, parent, leaf, 1) != 0) { ThrowLast(); }
    }

    internal override bool Abort()
    {
        var success = !unboundCreation;
        for (var index = Owned.Count - 1; index >= 0; index--)
        {
            var node = Owned[index];
            try
            {
                RequireName(node);
                if (Unlinkat(node.Parent?.Handle ?? parent, node.Name, node.Directory ? 0x200 : 0) != 0)
                {
                    CleanupError ??= Marshal.GetLastPInvokeError();
                    success = false;
                }
            }
            catch (PosixPublicationException failure) { success = false; CleanupError ??= failure.NativeError; }
            catch { success = false; }
        }
        return success;
    }

    public override void Dispose() { DisposeOwned(); parent.Dispose(); }

    private SafeFileHandle Open(int directory, string name, ulong flags, ulong mode, ulong resolve)
    {
        var how = new OpenHow { Flags = flags, Mode = mode, Resolve = resolve };
        var descriptor = Openat2(437, directory, name, ref how, 24);
        if (descriptor < 0) { ThrowLast(); }
        if (descriptor < 3)
        {
            var duplicate = Fcntl((int)descriptor, 1030, 3);
            var error = Marshal.GetLastPInvokeError();
            Close((int)descriptor);
            if (duplicate < 0) { Fail(error); }
            descriptor = duplicate;
        }
        return new SafeFileHandle((nint)descriptor, true);
    }

    private void ThrowLast() => Fail(Marshal.GetLastPInvokeError());
    private void Fail(int error)
    {
        NativeError = error;
        throw new PosixPublicationException(error switch
        {
            17 => ArchiveError.OutputConflict,
            40 => ArchiveError.UnsafePath,
            _ => ArchiveError.OutputUnavailable
        }, error);
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct OpenHow { internal ulong Flags; internal ulong Mode; internal ulong Resolve; }
    [DllImport("libc", EntryPoint = "syscall", SetLastError = true)]
    private static extern long Openat2(long syscall, int directory, [MarshalAs(UnmanagedType.LPUTF8Str)] string name,
        ref OpenHow how, nuint size);
    [DllImport("libc", EntryPoint = "syscall", SetLastError = true)]
    private static extern long Getdents64(long syscall, SafeFileHandle directory, [Out] byte[] buffer, uint size);
    [DllImport("libc", EntryPoint = "statx", SetLastError = true)]
    private static extern int Statx(SafeFileHandle directory, [MarshalAs(UnmanagedType.LPUTF8Str)] string name,
        int flags, uint mask, [Out] byte[] status);
    [DllImport("libc", EntryPoint = "fstatfs", SetLastError = true)]
    private static extern int Fstatfs(SafeFileHandle directory, [Out] byte[] status);
    [DllImport("libc", EntryPoint = "mkdirat", SetLastError = true)]
    private static extern int Mkdirat(SafeFileHandle directory, [MarshalAs(UnmanagedType.LPUTF8Str)] string name, uint mode);
    [DllImport("libc", EntryPoint = "renameat2", SetLastError = true)]
    private static extern int Renameat2(SafeFileHandle sourceDirectory, [MarshalAs(UnmanagedType.LPUTF8Str)] string source,
        SafeFileHandle targetDirectory, [MarshalAs(UnmanagedType.LPUTF8Str)] string target, uint flags);
    [DllImport("libc", EntryPoint = "unlinkat", SetLastError = true)]
    private static extern int Unlinkat(SafeFileHandle directory, [MarshalAs(UnmanagedType.LPUTF8Str)] string name, int flags);
    [DllImport("libc", EntryPoint = "fsync", SetLastError = true)]
    private static extern int Fsync(SafeFileHandle file);
    [DllImport("libc", EntryPoint = "getpid")]
    private static extern int GetProcessId();
    [DllImport("libc", EntryPoint = "gettid")]
    private static extern int GetThreadId();
    [DllImport("libc", EntryPoint = "geteuid")]
    private static extern uint GetEffectiveUser();
    [DllImport("libc", EntryPoint = "fcntl", SetLastError = true)]
    private static extern int Fcntl(int descriptor, int command, int minimum);
    [DllImport("libc", EntryPoint = "close")]
    private static extern int Close(int descriptor);
}

internal sealed class PosixPublicationException(ArchiveError code, int error) : Exception(code.ToString())
{
    internal ArchiveError Code { get; } = code;
    internal int NativeError { get; } = error;
}

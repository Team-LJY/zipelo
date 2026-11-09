using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
using Zipelo.Core;

namespace Zipelo.Runtime;

internal sealed class LinuxPublication : PendingPublication
{
    private readonly SafeFileHandle directory;
    private readonly SafeFileHandle file;

    private LinuxPublication(SafeFileHandle directory, SafeFileHandle file)
    {
        this.directory = directory;
        this.file = file;
    }

    internal override SafeFileHandle Handle => file;

    internal static LinuxPublication Create(string parent)
    {
        // x64 syscall 번호를 사용함. 호출 전 공통 API에서 아키텍처를 제한함.
        var how = new OpenHow { Flags = 0x80000 | 0x10000, Mode = 0, Resolve = 0x04 };
        var descriptor = OpenAt2(437, -100, parent, ref how, (nuint)Marshal.SizeOf<OpenHow>());
        var directory = OwnDescriptor(checked((int)descriptor));
        try
        {
            // O_TMPFILE | O_RDWR | O_CLOEXEC, 생성 권한은 0600 이하임.
            var file = OwnDescriptor(OpenAt(directory, ".", 0x410000 | 2 | 0x80000, 0x180));
            return new(directory, file);
        }
        catch { directory.Dispose(); throw; }
    }

    internal override void Publish(string leaf)
    {
        // 커널 procfs의 보유 fd를 연결하며 새 목적지 링크는 덮어쓰지 않음.
        var descriptorPath = $"/proc/self/fd/{file.DangerousGetHandle().ToInt64()}";
        if (LinkAt(-100, descriptorPath, directory, leaf, 0x400) != 0) { ThrowLastError(); }
    }

    // 이름 없는 임시 inode는 마지막 핸들이 닫힐 때 제거됨.
    internal override bool Abort() => true;

    public override void Dispose()
    {
        file.Dispose();
        directory.Dispose();
    }

    private static SafeFileHandle OwnDescriptor(int descriptor)
    {
        if (descriptor < 0) { ThrowLastError(); }
        if (descriptor < 3)
        {
            var duplicate = DuplicateDescriptor(descriptor, 1030, 3);
            var error = Marshal.GetLastPInvokeError();
            Close(descriptor);
            if (duplicate < 0) { throw Error(error); }
            descriptor = duplicate;
        }
        return new SafeFileHandle((nint)descriptor, ownsHandle: true);
    }

    private static void ThrowLastError() => throw Error(Marshal.GetLastPInvokeError());
    private static ArchiveException Error(int error) => new(error switch
    {
        17 => ArchiveError.OutputConflict,
        40 => ArchiveError.UnsafePath,
        _ => ArchiveError.OutputUnavailable
    });

    [StructLayout(LayoutKind.Sequential)]
    private struct OpenHow
    {
        internal ulong Flags;
        internal ulong Mode;
        internal ulong Resolve;
    }

    [DllImport("libc", EntryPoint = "syscall", SetLastError = true)]
    private static extern long OpenAt2(long number, int directory,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string path, ref OpenHow how, nuint size);

    [DllImport("libc", EntryPoint = "openat", SetLastError = true)]
    private static extern int OpenAt(SafeFileHandle directory,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string path, int flags, uint mode);

    [DllImport("libc", EntryPoint = "linkat", SetLastError = true)]
    private static extern int LinkAt(int oldDirectory, [MarshalAs(UnmanagedType.LPUTF8Str)] string oldPath,
        SafeFileHandle newDirectory, [MarshalAs(UnmanagedType.LPUTF8Str)] string newPath, int flags);

    [DllImport("libc", EntryPoint = "fcntl", SetLastError = true)]
    private static extern int DuplicateDescriptor(int descriptor, int command, int minimum);

    [DllImport("libc", EntryPoint = "close")]
    private static extern int Close(int descriptor);
}

using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;
using Zipelo.Core;

namespace Zipelo.Runtime;

internal sealed class WindowsPublication : PendingPublication
{
    private readonly List<SafeFileHandle> directories;
    private readonly SafeFileHandle file;
    private readonly string temporaryName;
    private PublicationNativeFailure? cleanupNativeFailure;

    private WindowsPublication(List<SafeFileHandle> directories, SafeFileHandle file, string temporaryName)
    {
        this.directories = directories;
        this.file = file;
        this.temporaryName = temporaryName;
    }

    internal override SafeFileHandle Handle => file;
    internal override string TemporaryFileName => temporaryName;
    internal override PublicationNativeFailure? CleanupNativeFailure => cleanupNativeFailure;

    internal static List<SafeFileHandle> OpenParent(string parent, bool readSecurity = false)
    {
        var root = Path.GetPathRoot(parent);
        if (root is null || root.Length != 3 || !char.IsAsciiLetter(root[0]) || root[1] != ':' || root[2] != '\\')
        {
            throw new ArchiveException(ArchiveError.UnsafePath);
        }
        // UNC, 장치 경로와 네트워크 드라이브는 초기 게시 구현에서 제외함.
        if (GetDriveType(root) is not (2 or 3)) { throw new ArchiveException(ArchiveError.OutputUnavailable); }
        var relative = parent[root.Length..];
        var segments = relative.Length == 0 ? Array.Empty<string>() :
            OutputPathPolicy.Validate(relative, FileNamePolicy.Windows).ToArray();
        var directories = new List<SafeFileHandle>();
        try
        {
            var rootHandle = CreateFile(root, readSecurity ? 0x001200A0u : 0x001000A0u, 3, nint.Zero, 3, 0x02200000, nint.Zero);
            if (rootHandle.IsInvalid)
            {
                var error = unchecked((uint)Marshal.GetLastPInvokeError());
                rootHandle.Dispose();
                throw Error("CreateFileW", error);
            }
            directories.Add(rootHandle);
            RequireDirectory(rootHandle);
            foreach (var segment in segments)
            {
                var directory = OpenRelative(directories[^1], segment, create: false, readSecurity: readSecurity);
                directories.Add(directory);
                RequireDirectory(directory);
            }
            return directories;
        }
        catch
        {
            for (var index = directories.Count - 1; index >= 0; index--) { directories[index].Dispose(); }
            throw;
        }
    }

    internal static WindowsPublication Create(string parent)
    {
        var directories = OpenParent(parent);
        try
        {
            var temporaryName = $".zipelo-{Guid.NewGuid():N}.tmp";
            var file = OpenRelative(directories[^1], temporaryName, create: true);
            return new(directories, file, temporaryName);
        }
        catch
        {
            for (var index = directories.Count - 1; index >= 0; index--) { directories[index].Dispose(); }
            throw;
        }
    }

    internal override void Publish(string leaf) => Rename(file, directories[^1], leaf);

    internal static void Rename(SafeFileHandle file, SafeFileHandle parent, string leaf)
    {
        var name = Encoding.Unicode.GetBytes(leaf);
        // FILE_RENAME_INFORMATION의 x64 배치와 후행 패딩을 포함함. 덮어쓰기 플래그는 0임.
        const int fileNameOffset = 20;
        var size = checked(Marshal.SizeOf<RenameInformation>() + name.Length);
        var buffer = Marshal.AllocHGlobal(size);
        try
        {
            Marshal.Copy(new byte[size], 0, buffer, size);
            Marshal.WriteIntPtr(buffer, 8, parent.DangerousGetHandle());
            Marshal.WriteInt32(buffer, 16, name.Length);
            Marshal.Copy(name, 0, buffer + fileNameOffset, name.Length);
            // Win32 경로 변환을 거치지 않고 보유 부모 핸들에 상대적으로 게시함.
            var status = NtSetInformationFile(file, out _, buffer, (uint)size, 10);
            if (status < 0)
            {
                throw Error("NtSetInformationFile.FileRenameInformation", RtlNtStatusToDosError(status), status);
            }
        }
        finally { Marshal.FreeHGlobal(buffer); }
    }

    internal override bool Abort() => DeleteOwned(file, out cleanupNativeFailure);

    internal static bool DeleteOwned(SafeFileHandle file, out PublicationNativeFailure? failure)
    {
        failure = null;
        var buffer = Marshal.AllocHGlobal(1);
        try
        {
            // 작업 소유 핸들에 삭제를 표시함. 경로를 다시 열거나 삭제하지 않음.
            Marshal.WriteByte(buffer, 1);
            if (SetFileInformation(file, 4, buffer, 1)) { return true; }
            var error = unchecked((uint)Marshal.GetLastPInvokeError());
            failure = new("SetFileInformationByHandle.FileDispositionInfo", error);
            return false;
        }
        finally { Marshal.FreeHGlobal(buffer); }
    }

    public override void Dispose()
    {
        file.Dispose();
        for (var index = directories.Count - 1; index >= 0; index--) { directories[index].Dispose(); }
    }

    internal static SafeFileHandle OpenRelative(SafeFileHandle parent, string name, bool create,
        bool directory = false, bool read = false, bool reopenOwned = false, bool readSecurity = false)
    {
        var nameBuffer = Marshal.StringToHGlobalUni(name);
        var unicodeBuffer = Marshal.AllocHGlobal(Marshal.SizeOf<UnicodeString>());
        try
        {
            var unicode = new UnicodeString
            {
                Length = checked((ushort)(name.Length * 2)),
                MaximumLength = checked((ushort)((name.Length + 1) * 2)),
                Buffer = nameBuffer
            };
            Marshal.StructureToPtr(unicode, unicodeBuffer, false);
            var attributes = new ObjectAttributes
            {
                Length = Marshal.SizeOf<ObjectAttributes>(),
                RootDirectory = parent.DangerousGetHandle(),
                ObjectName = unicodeBuffer,
                Attributes = 0x1040
            };
            // 디렉터리는 삭제 공유 없이 고정하고 최종 reparse point도 따라가지 않음.
            var access = reopenOwned ? 0x001300A1u : !create ? 0x001000A0u : directory ? 0x001300A3u : read ? 0xC0110000u : 0x40110000u;
            if (readSecurity) { access |= 0x20000; }
            var options = 0x00200020u | (directory ? 1u : create ? 0x40u : 0u);
            var status = NtCreateFile(out var handle, access,
                ref attributes, out _, nint.Zero, create && !directory ? 0x100u : 0u, (create || reopenOwned) ? 0u : 3u,
                create ? 2u : 1u, options, nint.Zero, 0);
            if (status < 0)
            {
                handle.Dispose();
                throw Error("NtCreateFile", RtlNtStatusToDosError(status), status);
            }
            return handle;
        }
        finally
        {
            Marshal.FreeHGlobal(unicodeBuffer);
            Marshal.FreeHGlobal(nameBuffer);
        }
    }

    private static void RequireDirectory(SafeFileHandle handle)
    {
        if (!GetFileInformation(handle, 9, out var info, (uint)Marshal.SizeOf<AttributeTagInfo>())) { ThrowLastError("GetFileInformationByHandleEx"); }
        if ((info.Attributes & 0x10) == 0 || (info.Attributes & 0x400) != 0)
        {
            throw new ArchiveException(ArchiveError.UnsafePath);
        }
    }

    private static void ThrowLastError(string operation)
    {
        var error = unchecked((uint)Marshal.GetLastPInvokeError());
        throw Error(operation, error);
    }

    private static PublicationNativeException Error(string operation, uint error, int? status = null) => new(
        error is 80 or 183 ? ArchiveError.OutputConflict : ArchiveError.OutputUnavailable,
        new(operation, error, status));

    [StructLayout(LayoutKind.Explicit, Size = 24)]
    private struct RenameInformation
    {
        [FieldOffset(0)] internal uint Flags;
        [FieldOffset(8)] internal nint RootDirectory;
        [FieldOffset(16)] internal uint FileNameLength;
        [FieldOffset(20)] internal ushort FirstCharacter;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct UnicodeString
    {
        internal ushort Length;
        internal ushort MaximumLength;
        internal nint Buffer;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct ObjectAttributes
    {
        internal int Length;
        internal nint RootDirectory;
        internal nint ObjectName;
        internal uint Attributes;
        internal nint SecurityDescriptor;
        internal nint SecurityQualityOfService;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct IoStatusBlock
    {
        internal nint Status;
        internal nuint Information;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct AttributeTagInfo
    {
        internal uint Attributes;
        internal uint ReparseTag;
    }

    [DllImport("kernel32.dll", EntryPoint = "CreateFileW", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern SafeFileHandle CreateFile(string path, uint access, uint share, nint security,
        uint disposition, uint flags, nint template);

    [DllImport("kernel32.dll", EntryPoint = "GetDriveTypeW", CharSet = CharSet.Unicode)]
    private static extern uint GetDriveType(string root);

    [DllImport("kernel32.dll", EntryPoint = "GetFileInformationByHandleEx", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetFileInformation(SafeFileHandle file, int informationClass,
        out AttributeTagInfo information, uint size);

    [DllImport("kernel32.dll", EntryPoint = "SetFileInformationByHandle", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetFileInformation(SafeFileHandle file, int informationClass, nint information, uint size);

    [DllImport("ntdll.dll")]
    private static extern int NtCreateFile(out SafeFileHandle file, uint access, ref ObjectAttributes attributes,
        out IoStatusBlock status, nint allocationSize, uint fileAttributes, uint share, uint disposition,
        uint options, nint extendedAttributes, uint extendedAttributesLength);

    [DllImport("ntdll.dll")]
    private static extern int NtSetInformationFile(SafeFileHandle file, out IoStatusBlock status,
        nint information, uint size, int informationClass);

    [DllImport("ntdll.dll")]
    private static extern uint RtlNtStatusToDosError(int status);
}

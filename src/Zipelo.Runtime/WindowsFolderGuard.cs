using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text;
using Microsoft.Win32.SafeHandles;
using Zipelo.Core;

namespace Zipelo.Runtime;

internal static class WindowsFolderGuard
{
    internal static void RequireNtfs(SafeFileHandle handle)
    {
        var name = new StringBuilder(32);
        if (!GetVolumeInformationByHandleW(handle, null, 0, out _, out _, out _, name, name.Capacity))
        {
            ThrowLast("GetVolumeInformationByHandleW");
        }
        if (name.ToString() != "NTFS") { throw new ArchiveException(ArchiveError.OutputUnavailable); }
    }

    internal static string Inspect(SafeFileHandle handle, bool directory)
    {
        var attributes = Query(handle, 9, 8);
        var flags = BitConverter.ToUInt32(attributes, 0);
        if ((flags & 0x400) != 0 || ((flags & 0x10) != 0) != directory)
        {
            throw new ArchiveException(ArchiveError.UnsafePath);
        }
        var standard = Query(handle, 1, 24);
        if (standard[20] != 0 || (!directory && BitConverter.ToUInt32(standard, 16) != 1))
        {
            throw new ArchiveException(ArchiveError.UnsafePath);
        }
        RequirePrivateWriters(handle);
        return Convert.ToHexString(Query(handle, 18, 24));
    }

    internal static void RequirePrivateWriters(SafeFileHandle handle)
    {
        if (!OperatingSystem.IsWindows()) { throw new PlatformNotSupportedException(); }
        var error = GetSecurityInfo(handle, 1, 5, out _, out _, out _, out _, out var descriptor);
        if (error != 0) { throw Native("GetSecurityInfo", error); }
        try
        {
            var bytes = new byte[checked((int)GetSecurityDescriptorLength(descriptor))];
            Marshal.Copy(descriptor, bytes, 0, bytes.Length);
            var security = new RawSecurityDescriptor(bytes, 0);
            using var identity = WindowsIdentity.GetCurrent();
            var trusted = new HashSet<string>(StringComparer.Ordinal) { "S-1-5-18", "S-1-5-32-544" };
            if (identity.User is { } user) { trusted.Add(user.Value); }
            if (security.Owner is null || !trusted.Contains(security.Owner.Value) || security.DiscretionaryAcl is null)
            {
                throw new ArchiveException(ArchiveError.OutputUnavailable);
            }
            foreach (GenericAce ace in security.DiscretionaryAcl)
            {
                if (ace is not CommonAce common || common.IsCallback)
                {
                    throw new ArchiveException(ArchiveError.OutputUnavailable);
                }
                if (common.AceQualifier == AceQualifier.AccessDenied) { continue; }
                if (common.AceQualifier != AceQualifier.AccessAllowed) { throw new ArchiveException(ArchiveError.OutputUnavailable); }
                // 알 수 없는 쓰기 권한과 상속 전용 쓰기 ACE도 보수적으로 거부함.
                const uint mutation = 0x520D0156;
                if ((unchecked((uint)common.AccessMask) & mutation) == 0 || trusted.Contains(common.SecurityIdentifier.Value)) { continue; }
                if ((common.AceFlags & AceFlags.InheritOnly) != 0 &&
                    common.SecurityIdentifier.IsWellKnown(WellKnownSidType.CreatorOwnerSid)) { continue; }
                throw new ArchiveException(ArchiveError.OutputUnavailable);
            }
        }
        finally { LocalFree(descriptor); }
    }

    internal static void VerifyChildren(SafeFileHandle directory, IReadOnlyCollection<string> names)
    {
        var remaining = new HashSet<string>(names, StringComparer.OrdinalIgnoreCase);
        var buffer = Marshal.AllocHGlobal(65536);
        try
        {
            var restart = true;
            while (true)
            {
                var status = NtQueryDirectoryFile(directory, 0, 0, 0, out var io, buffer, 65536, 12, false, 0, restart);
                restart = false;
                if (status == unchecked((int)0x80000006)) { break; }
                if (status != 0) { throw Native("NtQueryDirectoryFile", RtlNtStatusToDosError(status), status); }
                var length = checked((int)io.Information);
                var offset = 0;
                do
                {
                    if (offset < 0 || offset > length - 12) { throw new ArchiveException(ArchiveError.OutputUnavailable); }
                    var next = Marshal.ReadInt32(buffer, offset);
                    var size = Marshal.ReadInt32(buffer, offset + 8);
                    if (size < 0 || size % 2 != 0 || size > length - offset - 12)
                    {
                        throw new ArchiveException(ArchiveError.OutputUnavailable);
                    }
                    var name = Marshal.PtrToStringUni(buffer + offset + 12, size / 2)!;
                    if (name is not ("." or "..") && !remaining.Remove(name)) { throw new ArchiveException(ArchiveError.UnsafePath); }
                    if (next == 0) { break; }
                    if (next < 12 + size) { throw new ArchiveException(ArchiveError.OutputUnavailable); }
                    offset = checked(offset + next);
                } while (true);
            }
            if (remaining.Count != 0) { throw new ArchiveException(ArchiveError.UnsafePath); }
        }
        finally { Marshal.FreeHGlobal(buffer); }
    }

    private static byte[] Query(SafeFileHandle handle, int kind, int size)
    {
        var bytes = new byte[size];
        if (!GetFileInformationByHandleEx(handle, kind, bytes, (uint)size)) { ThrowLast("GetFileInformationByHandleEx"); }
        return bytes;
    }

    private static void ThrowLast(string operation) => throw Native(operation, unchecked((uint)Marshal.GetLastPInvokeError()));
    private static PublicationNativeException Native(string operation, uint error, int? status = null)
        => new(ArchiveError.OutputUnavailable, new(operation, error, status));

    [StructLayout(LayoutKind.Sequential)]
    private struct IoStatusBlock { internal nint Status; internal nuint Information; }

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetVolumeInformationByHandleW(SafeFileHandle handle, StringBuilder? volume, int volumeSize,
        out uint serial, out uint maximumComponent, out uint flags, StringBuilder fileSystem, int fileSystemSize);
    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetFileInformationByHandleEx(SafeFileHandle handle, int kind, [Out] byte[] information, uint size);
    [DllImport("advapi32.dll")]
    private static extern uint GetSecurityInfo(SafeFileHandle handle, int kind, uint information,
        out nint owner, out nint group, out nint dacl, out nint sacl, out nint descriptor);
    [DllImport("advapi32.dll")]
    private static extern uint GetSecurityDescriptorLength(nint descriptor);
    [DllImport("kernel32.dll")]
    private static extern nint LocalFree(nint memory);
    [DllImport("ntdll.dll")]
    private static extern int NtQueryDirectoryFile(SafeFileHandle handle, nint completionEvent, nint routine, nint context,
        out IoStatusBlock status, nint buffer, uint length, int kind, [MarshalAs(UnmanagedType.U1)] bool single,
        nint filter, [MarshalAs(UnmanagedType.U1)] bool restart);
    [DllImport("ntdll.dll")]
    private static extern uint RtlNtStatusToDosError(int status);
}

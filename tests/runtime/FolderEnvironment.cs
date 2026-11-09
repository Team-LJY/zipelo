using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Security.AccessControl;
using System.Security.Principal;
using Zipelo.Core;
using Zipelo.SecurityTests;

namespace Zipelo.RuntimeTests;

internal sealed record FolderEnvironment(ArchiveError? Rejection, string Description)
{
    internal static FolderEnvironment Inspect(string newlyCreatedParent)
    {
        if (RuntimeInformation.ProcessArchitecture != Architecture.X64)
        {
            return Closed("x64 이외 아키텍처");
        }
        if (OperatingSystem.IsWindows()) { return Windows(newlyCreatedParent); }
        if (OperatingSystem.IsLinux()) { return Linux(newlyCreatedParent); }
        return Closed("Windows/Linux 이외 OS");
    }

    [SupportedOSPlatform("linux")]
    private static FolderEnvironment Linux(string parent)
    {
        // 별도 경로 관찰로 기대 조건을 정하며 제품 게시 결과로 SKIP 여부를 정하지 않음.
        var buffer = new byte[256];
        Check.Equal(0, Statfs(parent, buffer));
        var magic = BitConverter.ToInt64(buffer, 0);
        var expectedType = magic switch { 0xEF53 => "ext4", 0x58465342 => "xfs", _ => null };
        if (expectedType is null) { return Closed($"Linux 미지원 파일시스템 0x{magic:X}"); }
        var status = new byte[256];
        if (Statx(-100, parent, 0x100, 0x17FF, status) != 0 ||
            (BitConverter.ToUInt32(status, 0) & 0x100A) != 0x100A)
        {
            return Closed("Linux mount ID/소유자/모드 조회 불가");
        }
        var mountId = BitConverter.ToUInt64(status, 144);
        var device = FormattableString.Invariant($"{BitConverter.ToUInt32(status, 136)}:{BitConverter.ToUInt32(status, 140)}");
        string content;
        try { content = File.ReadAllText("/proc/thread-self/mountinfo"); }
        catch (IOException) { return Closed("Linux mountinfo 조회 불가"); }
        catch (UnauthorizedAccessException) { return Closed("Linux mountinfo 읽기 권한 없음"); }
        var matching = content.Split('\n', StringSplitOptions.RemoveEmptyEntries)
            .Select(line => line.Split(' ', StringSplitOptions.RemoveEmptyEntries))
            .Where(parts => parts.Length > 0 && ulong.TryParse(parts[0], out var id) && id == mountId).ToArray();
        if (matching.Length != 1) { return Closed("Linux mountinfo 레코드 누락 또는 중복"); }
        var fields = matching[0];
        var separator = Array.IndexOf(fields, "-");
        if (separator < 6 || fields.Length != separator + 4 || fields[2] != device || fields[separator + 1] != expectedType)
        {
            return Closed("Linux mount ID/장치/파일시스템 유형 불일치");
        }
        var mode = BitConverter.ToUInt16(status, 28);
        if (BitConverter.ToUInt32(status, 20) != GetEffectiveUser() || (mode & 0x12) != 0)
        {
            return Closed("Linux 부모 소유자 또는 그룹/기타 쓰기 조건 불충족");
        }
        return new(null, $"Linux {expectedType}, mount ID/장치 일치, 현재 UID 소유, 그룹 및 기타 쓰기 없음");
    }

    [SupportedOSPlatform("windows")]
    private static FolderEnvironment Windows(string parent)
    {
        var drive = new DriveInfo(Path.GetPathRoot(parent) ?? throw new InvalidOperationException());
        if (parent.StartsWith(@"\\", StringComparison.Ordinal))
        {
            return new(ArchiveError.UnsafePath, "Windows 네트워크 경로");
        }
        if (drive.DriveType is not (DriveType.Fixed or DriveType.Removable))
        {
            return Closed("Windows 로컬 고정/이동식 드라이브 이외 유형");
        }
        if (!string.Equals(drive.DriveFormat, "NTFS", StringComparison.OrdinalIgnoreCase))
        {
            return Closed("Windows 미지원 파일시스템 " + drive.DriveFormat);
        }
        var security = new DirectoryInfo(parent).GetAccessControl(AccessControlSections.Owner | AccessControlSections.Access);
        var descriptor = new RawSecurityDescriptor(security.GetSecurityDescriptorBinaryForm(), 0);
        using var identity = WindowsIdentity.GetCurrent();
        var trusted = new HashSet<string>(StringComparer.Ordinal) { "S-1-5-18", "S-1-5-32-544" };
        if (identity.User is { } user) { trusted.Add(user.Value); }
        if (descriptor.Owner is null || !trusted.Contains(descriptor.Owner.Value) || descriptor.DiscretionaryAcl is null)
        {
            return Closed("NTFS 부모 소유자 또는 DACL 조건 불충족");
        }
        const FileSystemRights mutation = FileSystemRights.Write | FileSystemRights.Delete |
            FileSystemRights.DeleteSubdirectoriesAndFiles | FileSystemRights.ChangePermissions | FileSystemRights.TakeOwnership;
        foreach (GenericAce entry in descriptor.DiscretionaryAcl)
        {
            if (entry is not CommonAce ace || ace.IsCallback)
            {
                return Closed("NTFS 부모의 미지원 ACE");
            }
            if (ace.AceQualifier == AceQualifier.AccessDenied) { continue; }
            if (ace.AceQualifier != AceQualifier.AccessAllowed) { return Closed("NTFS 부모의 미지원 ACE 유형"); }
            var writes = (ace.AccessMask & (int)mutation) != 0 || (ace.AccessMask & 0x52000000) != 0;
            if (!writes || trusted.Contains(ace.SecurityIdentifier.Value)) { continue; }
            if ((ace.AceFlags & AceFlags.InheritOnly) != 0 &&
                ace.SecurityIdentifier.IsWellKnown(WellKnownSidType.CreatorOwnerSid)) { continue; }
            return Closed("NTFS 부모의 신뢰 범위 밖 쓰기 ACE");
        }
        return new(null, "Windows NTFS, 현재 사용자/SYSTEM/Administrators 쓰기 경계");
    }

    private static FolderEnvironment Closed(string reason) => new(ArchiveError.OutputUnavailable, reason);

    [DllImport("libc", EntryPoint = "statfs", SetLastError = true)]
    private static extern int Statfs(string path, [Out] byte[] buffer);

    [DllImport("libc", EntryPoint = "statx", SetLastError = true)]
    private static extern int Statx(int directory, string path, int flags, uint mask, [Out] byte[] buffer);

    [DllImport("libc", EntryPoint = "geteuid")]
    private static extern uint GetEffectiveUser();
}

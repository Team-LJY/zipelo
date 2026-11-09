using System.Text;

namespace Zipelo.Core;

public enum FileNamePolicy { Windows, Linux }

public static class OutputPathPolicy
{
    public static FileNamePolicy CurrentPolicy => OperatingSystem.IsWindows() ? FileNamePolicy.Windows :
        OperatingSystem.IsLinux() ? FileNamePolicy.Linux : throw new PlatformNotSupportedException();

    public static StringComparison Comparison(FileNamePolicy policy) => policy switch
    {
        FileNamePolicy.Windows => StringComparison.OrdinalIgnoreCase,
        FileNamePolicy.Linux => StringComparison.Ordinal,
        _ => throw new ArgumentOutOfRangeException(nameof(policy))
    };

    public static StringComparer Comparer(FileNamePolicy policy) => policy switch
    {
        FileNamePolicy.Windows => StringComparer.OrdinalIgnoreCase,
        FileNamePolicy.Linux => StringComparer.Ordinal,
        _ => throw new ArgumentOutOfRangeException(nameof(policy))
    };

    public static IReadOnlyList<string> Validate(string path, FileNamePolicy policy)
    {
        if (policy == FileNamePolicy.Windows) { return Validate(path); }
        if (policy != FileNamePolicy.Linux) { throw new ArgumentOutOfRangeException(nameof(policy)); }
        if (string.IsNullOrEmpty(path) || path.Length > 4096 || path.Contains('\0'))
        {
            throw new ArchiveException(ArchiveError.UnsafePath);
        }
        var segments = path.Split('/');
        var encoding = new UTF8Encoding(false, true);
        foreach (var segment in segments)
        {
            if (segment.Length == 0 || segment is "." or "..")
            {
                throw new ArchiveException(ArchiveError.UnsafePath);
            }
            try
            {
                if (encoding.GetByteCount(segment) > 255) { throw new ArchiveException(ArchiveError.UnsafePath); }
            }
            catch (EncoderFallbackException) { throw new ArchiveException(ArchiveError.UnsafePath); }
        }
        return Array.AsReadOnly(segments);
    }

    // 기존 단일 인수 API는 Windows 사전 정책을 유지함.
    // 문자열 검사는 사전 정책임. 파일시스템의 링크나 경쟁 상태를 검사한 결과가 아님.
    public static IReadOnlyList<string> Validate(string path)
    {
        if (string.IsNullOrWhiteSpace(path) || path.Length > 4096)
        {
            throw new ArchiveException(ArchiveError.UnsafePath);
        }

        var segments = path.Replace('\\', '/').Split('/');
        foreach (var segment in segments)
        {
            if (segment.Length is 0 or > 255 || segment is "." or ".." ||
                segment.EndsWith(' ') || segment.EndsWith('.') ||
                !segment.IsNormalized(NormalizationForm.FormC) ||
                segment.Any(c => char.IsControl(c) || "<>:\"|?*".Contains(c)))
            {
                throw new ArchiveException(ArchiveError.UnsafePath);
            }

            var stem = segment.Split('.')[0].TrimEnd(' ').ToUpperInvariant();
            if (stem is "CON" or "PRN" or "AUX" or "NUL" or "CONIN$" or "CONOUT$" ||
                (stem.Length == 4 && (stem.StartsWith("COM") || stem.StartsWith("LPT")) &&
                "123456789¹²³".Contains(stem[3])))
            {
                throw new ArchiveException(ArchiveError.UnsafePath);
            }
        }

        return Array.AsReadOnly(segments);
    }
}

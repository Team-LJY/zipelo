using System.IO;

namespace Zipelo.Ui;

public sealed record ArchiveOpenTarget(string? Path, string? Error);

public static class ArchiveOpenArguments
{
    public static ArchiveOpenTarget Parse(IReadOnlyList<string> arguments)
    {
        if (arguments.Count == 0) { return new(null, null); }
        if (arguments.Count != 1)
        {
            return new(null, "압축파일은 한 번에 하나만 열 수 있습니다. 단일 로컬 ZIP 경로를 전달하세요.");
        }
        var path = arguments[0];
        if (path.StartsWith('-') || (OperatingSystem.IsWindows() && path.StartsWith('/')))
        {
            return new(null, "알 수 없는 옵션입니다. 옵션 없이 단일 로컬 ZIP 절대 경로를 전달하세요.");
        }
        return ValidatePath(path);
    }

    public static string? ValidateName(string name) => Path.GetExtension(name).ToLowerInvariant() switch
    {
        ".jar" => "JAR 파일은 지원 대상에서 제외됩니다.",
        ".zip" => null,
        _ => "현재는 .zip 파일의 목록 열기만 지원합니다. 다른 형식은 열 수 없습니다."
    };

    public static ArchiveOpenTarget ValidatePath(string path)
    {
        try
        {
            // URI, UNC 및 장치 경로를 실행 인수나 원격 주소로 재해석하지 않음.
            if (string.IsNullOrWhiteSpace(path) || path.Contains('\0') ||
                !Path.IsPathFullyQualified(path) || path.StartsWith(@"\\") || path.StartsWith("//"))
            {
                return new(null, "로컬 파일의 절대 경로가 필요합니다. URI, 네트워크와 장치 경로는 지원하지 않습니다.");
            }
            var error = ValidateName(path);
            if (error is not null) { return new(null, error); }
            if (!File.Exists(path)) { return new(null, "파일이 없거나 읽을 수 없습니다. 파일 위치와 접근 권한을 확인하세요."); }
            if ((File.GetAttributes(path) & (FileAttributes.Directory | FileAttributes.Device)) != 0)
            {
                return new(null, "폴더나 장치는 열 수 없습니다. 일반 ZIP 파일을 선택하세요.");
            }
            return new(Path.GetFullPath(path), null);
        }
        catch (Exception error) when (error is ArgumentException or IOException or UnauthorizedAccessException or NotSupportedException)
        {
            return new(null, "파일 경로를 확인하지 못했습니다. 올바른 로컬 ZIP 경로를 전달하세요.");
        }
    }
}

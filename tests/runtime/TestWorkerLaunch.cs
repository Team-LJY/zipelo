using Zipelo.Runtime;

namespace Zipelo.RuntimeTests;

internal static class TestWorkerLaunch
{
    private static string ExecutableSuffix => OperatingSystem.IsWindows() ? ".exe" : "";
    private static StringComparison PathComparison => OperatingSystem.IsWindows()
        ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;

    public static ZipWorkerClient Worker => Create("Zipelo.Worker");
    public static ZipWorkerClient Peer => Create("Zipelo.RuntimeTests");

    public static string? DotnetHost
    {
        get
        {
            var processPath = Environment.ProcessPath;
            if (processPath is null || !Path.IsPathFullyQualified(processPath) || !File.Exists(processPath))
            {
                throw new InvalidOperationException("현재 테스트 실행파일의 절대 경로를 확인할 수 없음");
            }

            // 현재 실행 중인 muxer만 재사용하며 PATH에서 호스트를 찾지 않음.
            if (string.Equals(Path.GetFileName(processPath), "dotnet" + ExecutableSuffix, PathComparison))
            {
                if (!string.Equals(typeof(Program).Assembly.Location,
                    OutputFile("Zipelo.RuntimeTests.dll"), PathComparison))
                {
                    throw new InvalidOperationException("테스트 DLL이 고정 출력 폴더와 일치하지 않음");
                }
                return processPath;
            }

            if (!string.Equals(processPath, OutputFile("Zipelo.RuntimeTests" + ExecutableSuffix), PathComparison))
            {
                throw new InvalidOperationException("알 수 없는 테스트 앱호스트임");
            }
            return null;
        }
    }

    private static ZipWorkerClient Create(string name)
    {
        var host = DotnetHost;
        var path = OutputFile(name + (host is null ? ExecutableSuffix : ".dll"));
        // 앱호스트에는 DLL 인수를 붙이지 않아 자기 앱의 무인수 peer 모드를 유지함.
        return new ZipWorkerClient(path, host);
    }

    private static string OutputFile(string name)
    {
        var path = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, name));
        if (!File.Exists(path)) { throw new FileNotFoundException("필수 테스트 출력 파일이 없음", path); }
        if ((File.GetAttributes(path) & (FileAttributes.Directory | FileAttributes.ReparsePoint)) != 0)
        {
            throw new InvalidOperationException("테스트 출력 실행파일의 링크를 허용하지 않음");
        }
        return path;
    }
}

namespace Zipelo.Runtime;

public static class WorkerPlatform
{
    public static string ExecutableName => OperatingSystem.IsWindows() ? "Zipelo.Worker.exe" :
        OperatingSystem.IsLinux() ? "Zipelo.Worker" : throw new PlatformNotSupportedException();

    public static string GetWorkerPath(string applicationDirectory)
    {
        if (!Path.IsPathFullyQualified(applicationDirectory))
        {
            throw new ArgumentException("앱 설치 폴더의 절대 경로가 필요함", nameof(applicationDirectory));
        }
        return Path.Combine(applicationDirectory, "worker", ExecutableName);
    }
}

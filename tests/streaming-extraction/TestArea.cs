using System.Diagnostics;
using System.Security.Cryptography;
using System.Text.Json;
using Zipelo.Core;
using Zipelo.Runtime.Streaming;

namespace Zipelo.StreamingExtractionTests;

internal static class Check
{
    public static void That(bool value, string message)
    {
        if (!value) { throw new InvalidOperationException(message); }
    }
    public static void Equal<T>(T expected, T actual) =>
        That(EqualityComparer<T>.Default.Equals(expected, actual), $"기대 {expected}, 실제 {actual}");
    public static void Bytes(byte[] expected, byte[] actual) =>
        That(SHA256.HashData(expected).SequenceEqual(SHA256.HashData(actual)), "내용 해시 불일치");
}

internal sealed class Blocked(string message) : Exception(message);

internal sealed record PeerPlan(Guid Job, string Mode);

internal static class Launch
{
    private static string Suffix => OperatingSystem.IsWindows() ? ".exe" : "";
    private static StringComparison Comparison => OperatingSystem.IsWindows()
        ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;

    public static string? Host
    {
        get
        {
            var process = Environment.ProcessPath ?? throw new InvalidOperationException("현재 실행 경로 없음");
            Check.That(Path.IsPathFullyQualified(process) && File.Exists(process), "현재 실행 경로가 유효하지 않음");
            if (string.Equals(Path.GetFileName(process), "dotnet" + Suffix, Comparison))
            {
                Check.That(string.Equals(typeof(Program).Assembly.Location,
                    Output("Zipelo.StreamingExtractionTests.dll"), Comparison), "테스트 DLL 위치 불일치");
                return process;
            }
            Check.That(string.Equals(process, Output("Zipelo.StreamingExtractionTests" + Suffix), Comparison),
                "알 수 없는 테스트 앱호스트");
            return null;
        }
    }

    public static string Output(string name)
    {
        var path = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, name));
        Check.That(File.Exists(path) && (File.GetAttributes(path) & FileAttributes.ReparsePoint) == 0,
            "필수 고정 출력 파일 없음 또는 링크: " + name);
        return path;
    }

    public static StreamingZipWorkerClient Client(bool peer = false)
    {
        var host = Host;
        var name = peer ? "Zipelo.StreamingExtractionTests" : "Zipelo.Worker";
        return new(Output(name + (host is null ? Suffix : ".dll")), host);
    }
}

internal sealed class TestArea
{
    public static JobLimits Limits => new(1024, 128L * 1024 * 1024, 16L * 1024 * 1024,
        TimeSpan.FromSeconds(15), 1);
    public string Root { get; }
    public string Parent { get; }
    public string Destination => Path.Combine(Parent, "result");
    public string SourcePath => Path.Combine(Root, "source.zip");
    public Guid Job { get; } = Guid.NewGuid();
    public byte[] Original { get; }
    public string Marker => Path.Combine(Parent, "keep.txt");

    public TestArea(string runRoot, byte[] zip)
    {
        Root = Path.Combine(runRoot, Guid.NewGuid().ToString("N"));
        CreateDirectory(Root);
        Parent = Path.Combine(Root, "destination");
        CreateDirectory(Parent);
        Original = zip;
        File.WriteAllBytes(SourcePath, zip);
        File.WriteAllText(Marker, "기존 자료 보존");
    }

    public static void CreateDirectory(string path)
    {
        if (OperatingSystem.IsLinux())
        {
            Directory.CreateDirectory(path, UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute);
        }
        else { Directory.CreateDirectory(path); }
    }

    public FileStream Open() => new(SourcePath, FileMode.Open, FileAccess.Read, FileShare.Read);
    public void Plan(string mode)
    {
        File.WriteAllText(Path.Combine(Root, "peer.json"), JsonSerializer.Serialize(new PeerPlan(Job, mode)));
        // 실행 파일은 고정 앱 출력이며 실패 주입 파일은 이 시험 소유 폴더에만 존재함.
        Environment.CurrentDirectory = Root;
    }
    public void Release() => File.WriteAllText(Path.Combine(Root, "release"), "1");
    public Task Ready() => Wait(() => File.Exists(Path.Combine(Root, "ready")));
    public static async Task Wait(Func<bool> condition)
    {
        var watch = Stopwatch.StartNew();
        while (!condition())
        {
            if (watch.Elapsed > TimeSpan.FromSeconds(5)) { throw new TimeoutException("시험 장벽 진입 실패"); }
            await Task.Delay(10);
        }
    }

    public void Preserved(Stream? source = null, long position = 0)
    {
        Check.Bytes(Original, File.ReadAllBytes(SourcePath));
        Check.Equal("기존 자료 보존", File.ReadAllText(Marker));
        if (source is not null)
        {
            Check.That(source.CanRead, "호스트가 호출자 입력을 닫음");
            Check.Equal(position, source.Position);
        }
    }

    public static void Successful(StreamingExtractionResult result, int entries)
    {
        Check.That(result.Publication.Published, "게시 실패: " + result.Publication.Error);
        Check.Equal(ArchiveError.None, result.Publication.Error);
        Check.That(result.WorkerExitConfirmed && result.WorkerExitCode == 0, "정상 Worker 종료 미확인");
        Check.Equal(entries, result.VerifiedEntries);
        CleanFlags(result);
    }

    public static void CleanFlags(StreamingExtractionResult result)
    {
        Check.That(!result.InputCleanupFailed && result.InputTemporaryName is null &&
            !result.Publication.CleanupFailed && result.Publication.TemporaryFileName is null, "임시 항목 정리 실패");
    }

    public void Rejected(StreamingExtractionResult result, params ArchiveError[] errors)
    {
        Check.That(!result.Publication.Published && result.Publication.Error != ArchiveError.None, "실패가 성공으로 처리됨");
        Check.That(errors.Length == 0 || errors.Contains(result.Publication.Error),
            "예상하지 않은 오류: " + result.Publication.Error);
        Check.That(result.WorkerExitConfirmed, "Worker 종료 확인 실패");
        Check.That(!Directory.Exists(Destination) && !File.Exists(Destination), "실패 후 결과가 게시됨");
        CleanFlags(result);
        ExactParent("keep.txt");
    }

    public void ExactParent(params string[] names)
    {
        var actual = Directory.GetFileSystemEntries(Parent).Select(Path.GetFileName).Order().ToArray();
        Check.That(actual.SequenceEqual(names.Order()), "예상하지 않은 결과/임시 항목이 남음: " + string.Join(",", actual));
    }

    public void VerifyTree(FixtureEntry[] entries)
    {
        var expected = new HashSet<string>(StringComparer.Ordinal);
        foreach (var entry in entries)
        {
            var parts = entry.Name.TrimEnd('/').Split('/');
            for (var index = 1; index <= parts.Length; index++) { expected.Add(string.Join('/', parts.Take(index))); }
            var path = Path.Combine(Destination, entry.Name.TrimEnd('/').Replace('/', Path.DirectorySeparatorChar));
            if (entry.Name.EndsWith('/')) { Check.That(Directory.Exists(path), "디렉터리 누락"); }
            else { Check.Bytes(entry.Bytes, File.ReadAllBytes(path)); }
        }
        var actual = new HashSet<string>(StringComparer.Ordinal);
        void Visit(string directory)
        {
            foreach (var path in Directory.GetFileSystemEntries(directory))
            {
                var attributes = File.GetAttributes(path);
                Check.That((attributes & FileAttributes.ReparsePoint) == 0, "결과에 링크 포함");
                actual.Add(Path.GetRelativePath(Destination, path).Replace(Path.DirectorySeparatorChar, '/'));
                if ((attributes & FileAttributes.Directory) != 0) { Visit(path); }
            }
        }
        Check.That((File.GetAttributes(Destination) & FileAttributes.ReparsePoint) == 0, "결과 루트가 링크임");
        Visit(Destination);
        Check.That(expected.SetEquals(actual), "게시 목록 불일치");
        ExactParent("keep.txt", "result");
    }
}

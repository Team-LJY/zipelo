using System.Security.Cryptography;
using System.Text.Json;

namespace Zipelo.UbuntuUiHarness;

internal sealed class Fixture : IDisposable
{
    public static readonly byte[] Bytes = Convert.FromHexString("504b0506000000000000000000000000000000000000");
    public string DirectoryPath { get; }
    public string InputPath { get; }
    public static string Digest(byte[] bytes) => Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant();
    public Fixture(string projectDirectory)
    {
        var runs = Path.Combine(projectDirectory, ".runs");
        Directory.CreateDirectory(runs);
        UiSession.Check((File.GetAttributes(runs) & FileAttributes.ReparsePoint) == 0, "linked test parent");
        DirectoryPath = Path.Combine(runs, Guid.NewGuid().ToString("N"));
        UiSession.Check(!Directory.Exists(DirectoryPath) && !File.Exists(DirectoryPath), "existing fixture root");
        Directory.CreateDirectory(DirectoryPath);
        InputPath = Path.Combine(DirectoryPath, "startup.zip");
        using var stream = new FileStream(InputPath, FileMode.CreateNew, FileAccess.Write, FileShare.None);
        stream.Write(Bytes);
    }
    public void Dispose()
    {
        UiSession.Check(File.ReadAllBytes(InputPath).SequenceEqual(Bytes), "fixture changed");
        File.Delete(InputPath);
        Directory.Delete(DirectoryPath, recursive: false);
        UiSession.Check(!Directory.Exists(DirectoryPath), "fixture remains");
    }
    public static string FindProject()
    {
        for (var current = new DirectoryInfo(AppContext.BaseDirectory); current is not null; current = current.Parent)
        {
            if (current.Name == "ubuntu-ui-harness" && current.Parent?.Name == "tests" &&
                File.Exists(Path.Combine(current.FullName, "contract.json"))) { return current.FullName; }
        }
        throw new InvalidOperationException("build output must remain under tests/ubuntu-ui-harness");
    }
}

internal sealed class SourceGuard
{
    private readonly Dictionary<string, string> hashes = [];
    public SourceGuard(string project)
    {
        var root = Path.GetFullPath(Path.Combine(project, "../.."));
        using var doc = JsonDocument.Parse(File.ReadAllText(Path.Combine(project, "source-inputs.json")));
        foreach (var item in doc.RootElement.EnumerateObject())
        {
            var path = Path.Combine(root, item.Name);
            var expected = item.Value.GetString()!;
            UiSession.Check(Fixture.Digest(File.ReadAllBytes(path)) == expected, "pinned source mismatch: " + item.Name);
            hashes.Add(path, expected);
        }
        foreach (var line in File.ReadAllLines(Path.Combine(project, "SHA256SUMS")))
        {
            var parts = line.Split("  ", 2, StringSplitOptions.None);
            UiSession.Check(parts.Length == 2 && Path.GetFileName(parts[1]) == parts[1], "invalid harness manifest");
            var path = Path.Combine(project, parts[1]);
            UiSession.Check(Fixture.Digest(File.ReadAllBytes(path)) == parts[0], "harness source mismatch");
            hashes.Add(path, parts[0]);
        }
        var manifest = Path.Combine(project, "SHA256SUMS");
        hashes.Add(manifest, Fixture.Digest(File.ReadAllBytes(manifest)));
        foreach (var path in new[] { typeof(Program).Assembly.Location, typeof(Zipelo.Ubuntu.MainWindow).Assembly.Location,
                     typeof(Zipelo.Core.JobResult).Assembly.Location, typeof(Avalonia.Application).Assembly.Location,
                     typeof(Avalonia.Themes.Fluent.FluentTheme).Assembly.Location, typeof(Zipelo.Runtime.ZipWorkerClient).Assembly.Location })
        {
            hashes.TryAdd(path, Fixture.Digest(File.ReadAllBytes(path)));
        }
    }
    public void Verify()
    {
        foreach (var pair in hashes)
        {
            UiSession.Check(Fixture.Digest(File.ReadAllBytes(pair.Key)) == pair.Value, "source or assembly changed");
        }
    }
}

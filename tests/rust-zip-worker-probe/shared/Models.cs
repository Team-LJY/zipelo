using System.Security.Cryptography;
using System.Text.Json;

namespace Zipelo.StreamingExtractionTests;

internal sealed record ExpectedEntry(string Name, string Kind, long Length, string Sha256, uint Crc32);
internal sealed record Sample(string Id, string Base64, string Sha256, ExpectedEntry[] Entries);
internal sealed record CaseSpec(string Id, string Fixture, string Operation, string Mode,
    string Expected, string WindowsExpected, string Injection, string FaultKind, bool CauseRequired);
internal sealed record CasePlan(CaseSpec Case, Sample Sample, string Implementation,
    string Worker, string WorkerSha256, string Dotnet, string SourceCommit, string RunDirectory);
internal sealed record EntryObservation(string Name, string Kind, long Length, string? Sha256, uint? Crc32,
    string? Id, string Verification);
internal sealed class Observation
{
    public int SchemaVersion { get; set; } = 1;
    public string CaseId { get; set; } = "";
    public string Implementation { get; set; } = "";
    public string SourceCommit { get; set; } = "";
    public string BinarySha256 { get; set; } = "";
    public string InputSha256 { get; set; } = "";
    public string OsPolicy { get; set; } = "";
    public string JobId { get; set; } = "";
    public string Verdict { get; set; } = "fail";
    public string State { get; set; } = "not_started";
    public string Error { get; set; } = "None";
    public string Injection { get; set; } = "";
    public bool InjectionConfirmed { get; set; }
    public string FaultKind { get; set; } = "";
    public string ObservedCause { get; set; } = "not_exposed_by_host";
    public bool CauseVerified { get; set; }
    public bool Published { get; set; }
    public bool? WorkerExitConfirmed { get; set; }
    public int? WorkerExitCode { get; set; }
    public bool CleanupConfirmed { get; set; }
    public bool InputUnchanged { get; set; }
    public bool ExistingTargetUnchanged { get; set; }
    public bool SourcePositionVerified { get; set; }
    public bool DiagnosticsComplete { get; set; }
    public bool? WorkerStarted { get; set; }
    public string CoverageLayer { get; set; } = "unobserved";
    public long OutputBytes { get; set; }
    public long ProcessedBytes { get; set; }
    public double ElapsedMilliseconds { get; set; }
    public bool StopRequired { get; set; }
    public string Detail { get; set; } = "";
    public List<EntryObservation> Entries { get; set; } = [];
}

internal static class Data
{
    internal static readonly JsonSerializerOptions Json = new() { PropertyNamingPolicy = JsonNamingPolicy.CamelCase };
    internal static string Hash(byte[] bytes) => Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant();

    internal static T Read<T>(string path, long limit)
    {
        var info = new FileInfo(path);
        Check.That(info.Exists && info.Length <= limit && (info.Attributes & FileAttributes.ReparsePoint) == 0,
            "제한된 일반 입력 파일이 필요함");
        return JsonSerializer.Deserialize<T>(File.ReadAllText(path), Json)
            ?? throw new InvalidDataException("JSON 값 없음");
    }

    internal static void WriteNew<T>(string path, T value)
    {
        using var stream = new FileStream(path, FileMode.CreateNew, FileAccess.Write, FileShare.Read);
        JsonSerializer.Serialize(stream, value, Json);
    }

    internal static async Task Gate(string path)
    {
        Check.That(Path.IsPathFullyQualified(path), "절대 장벽 경로가 필요함");
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(10));
        while (!File.Exists(path)) { await Task.Delay(10, timeout.Token); }
        Check.That(new FileInfo(path).Length == 1 && File.ReadAllText(path) == "1", "장벽 불일치");
    }
}

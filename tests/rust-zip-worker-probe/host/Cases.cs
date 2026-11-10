using System.Diagnostics;
using System.Security.Cryptography;
using Zipelo.Core;
using Zipelo.Runtime;
using Zipelo.Runtime.Streaming;

namespace Zipelo.StreamingExtractionTests;

internal static class Cases
{
    internal static async Task Run(CasePlan plan, Observation report, Action<string>? verificationProbe = null)
    {
        Evidence.Validate(plan);
        var cleanupCandidate = false;
        Check.That(plan.Sample.Base64.Length <= 2 * 1024 * 1024, "입력 크기 초과");
        var bytes = Convert.FromBase64String(plan.Sample.Base64);
        Check.That(bytes.Length <= 1024 * 1024 && Data.Hash(bytes) == plan.Sample.Sha256, "입력 해시 불일치");
        Check.That(Path.IsPathFullyQualified(plan.Worker) && File.Exists(plan.Worker) &&
            Data.Hash(File.ReadAllBytes(plan.Worker)) == plan.WorkerSha256, "Worker 해시 불일치");
        var sourcePath = Path.Combine(plan.RunDirectory, "source.zip");
        using (var file = new FileStream(sourcePath, FileMode.CreateNew)) { file.Write(bytes); }
        var parent = Path.Combine(plan.RunDirectory, "destination");
        MakeDirectory(parent);
        var marker = Path.Combine(parent, "keep.txt");
        File.WriteAllText(marker, "기존 합성 자료 보존");
        var markerHash = Data.Hash(File.ReadAllBytes(marker));
        var destination = Path.Combine(parent, "result");
        var expected = OperatingSystem.IsWindows() && plan.Case.WindowsExpected != ""
            ? plan.Case.WindowsExpected : plan.Case.Expected;
        var mode = plan.Case.Mode;
        var special = new List<(string Path, string Hash)>();
        var allowed = new HashSet<string>(StringComparer.Ordinal) { "keep.txt" };
        var injected = plan.Case.Injection != "unavailable";
        using var cancellation = CancellationTokenSource.CreateLinkedTokenSource(CaseLifetime.Token);
        Stream source = mode == "snapshot-cancel"
            ? new CancelReadStream(bytes, cancellation)
            : new MemoryStream(bytes, false);
        using (source)
        {
            try
            {
                if (!injected)
                {
                    report.Verdict = "blocked";
                    report.Detail = mode;
                    return;
                }
                if (mode is "destination-file" or "destination-directory")
                {
                    if (mode == "destination-directory") { MakeDirectory(destination); }
                    var keep = mode == "destination-file" ? destination : Path.Combine(destination, "existing.txt");
                    File.WriteAllText(keep, "동명 원본 표식");
                    special.Add((keep, Data.Hash(File.ReadAllBytes(keep))));
                    allowed.Add("result");
                }
                if (mode is "destination-link" or "parent-link")
                {
                    var outside = Path.Combine(plan.RunDirectory, "outside");
                    MakeDirectory(outside);
                    var keep = Path.Combine(outside, "existing.txt");
                    File.WriteAllText(keep, "링크 대상 원본 표식");
                    special.Add((keep, Data.Hash(File.ReadAllBytes(keep))));
                    try
                    {
                        if (mode == "destination-link")
                        {
                            Directory.CreateSymbolicLink(destination, outside);
                            allowed.Add("result");
                        }
                        else
                        {
                            var link = Path.Combine(plan.RunDirectory, "parent-link");
                            Directory.CreateSymbolicLink(link, outside);
                            destination = Path.Combine(link, "result");
                        }
                    }
                    catch (Exception error) when (error is UnauthorizedAccessException or IOException or PlatformNotSupportedException)
                    {
                        injected = false;
                        report.Verdict = "blocked";
                        report.Detail = "link_not_injected:" + error.GetType().Name;
                        return;
                    }
                }
                report.InjectionConfirmed = true;
                if (mode == "before-cancel") { await cancellation.CancelAsync(); }
                var limits = new JobLimits(1024, 128L * 1024 * 1024, 16L * 1024 * 1024,
                    TimeSpan.FromSeconds(15), 1);
                limits = mode switch
                {
                    "entries-exact" => limits with { MaxEntries = 2 },
                    "entries-over" => limits with { MaxEntries = 1 },
                    "output-exact" => limits with { MaxOutputBytes = 4 },
                    "output-over" => limits with { MaxOutputBytes = 3 },
                    "processed-over" => limits with { MaxProcessedBytes = bytes.Length - 1 },
                    _ => limits
                };
                if (plan.Case.Operation == "list")
                {
                    Data.WriteNew(Path.Combine(plan.RunDirectory, "list-relay.json"),
                        new RelayPlan(Guid.Parse(report.JobId), plan.Invocation));
                    var value = await new ZipWorkerClient(typeof(Cases).Assembly.Location, plan.Dotnet)
                        .ListAsync(Guid.Parse(report.JobId), source, limits, cancellation.Token);
                    var proof = Data.Read<RelayProof>(Path.Combine(plan.RunDirectory, "list-identity.json"), 128 * 1024);
                    Check.That(ListRelay.Matches(proof, Guid.Parse(report.JobId), plan.Invocation),
                        "목록 실행 identity 불일치");
                    report.IdentityConfirmed = true;
                    report.IdentityEvidence = "list_relay_process_start_and_hash_before_after";
                    report.WorkerStarted = true;
                    report.WorkerExitCode = proof.ExitCode;
                    report.State = value.Job.State.ToString();
                    report.Error = value.Job.Error.ToString();
                    report.CoverageLayer = "worker_list";
                    foreach (var item in value.Job.Entries)
                    {
                        report.Entries.Add(new(item.Entry.Name, item.Entry.Kind.ToString(),
                            item.Entry.DeclaredSize ?? -1, null, null, item.Entry.Id, item.Verification.ToString()));
                    }
                    var matches = value.Job.State == JobState.Completed && value.Job.Error == ArchiveError.None &&
                        value.CreatedZip is null && value.ExtractedEntries is null &&
                        value.Job.Entries.Count == plan.Sample.Entries.Length;
                    for (var index = 0; matches && index < plan.Sample.Entries.Length; index++)
                    {
                        var actual = value.Job.Entries[index];
                        var entry = plan.Sample.Entries[index];
                        matches &= actual.Entry.Id == "entry-" + index && actual.Entry.Name == entry.Name &&
                            actual.Entry.Kind.ToString() == entry.Kind && actual.Entry.DeclaredSize == entry.Length &&
                            actual.Verification == VerificationState.NotChecked && actual.Outcome == EntryOutcome.Listed &&
                            actual.Error == ArchiveError.None;
                    }
                    report.Verdict = matches ? "pass" : "fail";
                    // v1은 종료 확인 필드를 노출하지 않으므로 외부 소유 프로세스 검사를 요구함.
                    report.WorkerExitConfirmed = null;
                    return;
                }
                var worker = plan.Worker;
                var workerHost = plan.Dotnet;
                var peer = plan.Case.Operation == "peer";
                if (peer)
                {
                    worker = typeof(Cases).Assembly.Location;
                    var peerMode = mode.StartsWith("cancel-", StringComparison.Ordinal) ? mode[7..] :
                        mode.StartsWith("deadline-", StringComparison.Ordinal) ? mode[9..] : mode;
                    Evidence.WritePeer(Path.Combine(plan.RunDirectory, "peer.json"),
                        new PeerPlan(Guid.Parse(report.JobId), peerMode));
                }
                if (mode.StartsWith("deadline-", StringComparison.Ordinal))
                {
                    limits = limits with { MaxDuration = TimeSpan.FromSeconds(2) };
                }
                var client = new ObservedWorkerClient(new StreamingZipWorkerClient(worker, workerHost));
                var pending = client.ExtractToNewFolderAsync(Guid.Parse(report.JobId), source, destination, limits,
                    cancellation.Token);
                if (peer && mode.StartsWith("cancel-", StringComparison.Ordinal))
                {
                    var ready = Path.Combine(plan.RunDirectory, "ready");
                    var wait = Stopwatch.StartNew();
                    while (!File.Exists(ready) && !pending.IsCompleted && wait.Elapsed < TimeSpan.FromSeconds(5))
                    {
                        await Task.Delay(10, CaseLifetime.Token);
                    }
                    if (!File.Exists(ready))
                    {
                        injected = false;
                        report.InjectionConfirmed = false;
                    }
                    await cancellation.CancelAsync();
                }
                var result = await pending;
                report.Published = result.Publication.Published;
                report.Error = result.Publication.Error.ToString();
                report.State = result.Publication.Published ? "Completed" :
                    result.Publication.Error == ArchiveError.Cancelled ? "Cancelled" : "Failed";
                report.WorkerExitConfirmed = result.WorkerExitConfirmed;
                report.WorkerExitCode = result.WorkerExitCode;
                report.OutputBytes = result.Usage.OutputBytes;
                report.ProcessedBytes = result.Usage.ProcessedBytes;
                cleanupCandidate = !result.InputCleanupFailed && result.InputTemporaryName is null &&
                    !result.Publication.CleanupFailed && result.Publication.TemporaryFileName is null;
                if (mode == "after-publish-cancel") { await cancellation.CancelAsync(); }
                if (result.Publication.Published)
                {
                    allowed.Add("result");
                    var valid = VerifyTree(destination, plan.Sample.Entries, report) &&
                        result.VerifiedEntries == plan.Sample.Entries.Length &&
                        result.Usage.OutputBytes == plan.Sample.Entries.Sum(e => e.Length) &&
                        result.WorkerExitCode == 0;
                    report.Verdict = valid && expected == "success" && report.Error == "None" ? "pass" : "fail";
                }
                else
                {
                    var absent = mode is "destination-file" or "destination-directory" or "destination-link" ||
                        (!File.Exists(destination) && !Directory.Exists(destination));
                    report.Verdict = expected != "success" && expected.Split('|').Contains(report.Error) && absent ? "pass" : "fail";
                }
                if (!injected)
                {
                    report.Verdict = "blocked";
                    report.Detail = "barrier_not_reached";
                }
                if (plan.Case.CauseRequired && report.Verdict == "pass")
                {
                    report.Verdict = "blocked";
                    report.Detail = "host_does_not_expose_decoder_format_vs_eof";
                }
                if (plan.Case.Id == "platform.empty" && report.Error == "OutputUnavailable")
                {
                    report.Verdict = "blocked";
                    report.StopRequired = true;
                    report.Detail = "publication_platform_unavailable";
                }
            }
            finally
            {
                Safety.Verify(report, () =>
                {
                    report.InjectionConfirmed = injected && report.InjectionConfirmed;
                    verificationProbe?.Invoke("source");
                    report.InputUnchanged = Data.Hash(File.ReadAllBytes(sourcePath)) == plan.Sample.Sha256 &&
                        Data.Hash(bytes) == plan.Sample.Sha256;
                    report.SourcePositionVerified = source.CanRead &&
                        (plan.Case.Operation == "list" ? source.Position == bytes.Length : source.Position == 0);
                    if (!report.InjectionConfirmed && source.Position == 0) { report.SourcePositionVerified = true; }
                    verificationProbe?.Invoke("marker");
                    report.ExistingTargetUnchanged = Data.Hash(File.ReadAllBytes(marker)) == markerHash &&
                        special.All(item => Data.Hash(File.ReadAllBytes(item.Path)) == item.Hash);
                    verificationProbe?.Invoke("tree");
                    var exact = Directory.GetFileSystemEntries(parent).Select(Path.GetFileName).ToHashSet(StringComparer.Ordinal)
                        .SetEquals(allowed);
                    if (mode is "destination-directory" or "destination-link")
                    {
                        exact &= Directory.GetFileSystemEntries(destination).Select(Path.GetFileName)
                            .SequenceEqual(new[] { "existing.txt" });
                    }
                    if (mode == "destination-link" && report.InjectionConfirmed)
                    {
                        exact &= new DirectoryInfo(destination).LinkTarget == Path.Combine(plan.RunDirectory, "outside");
                    }
                    if (mode == "parent-link" && report.InjectionConfirmed)
                    {
                        exact &= Directory.GetFileSystemEntries(Path.Combine(plan.RunDirectory, "outside"))
                            .Select(Path.GetFileName).SequenceEqual(new[] { "existing.txt" });
                    }
                    if (plan.Case.Operation is "list" or "blocked" || !report.InjectionConfirmed)
                    {
                        cleanupCandidate = exact;
                    }
                    else { cleanupCandidate &= exact; }
                    var confirmed = report.InputUnchanged && report.ExistingTargetUnchanged &&
                        report.SourcePositionVerified && cleanupCandidate;
                    if (!confirmed)
                    {
                        report.Verdict = "fail";
                        report.Detail = "preservation_or_cleanup_failed";
                    }
                    return confirmed;
                });
            }
        }
    }

    private static void MakeDirectory(string path)
    {
        Check.That(!Directory.Exists(path) && !File.Exists(path), "새 시험 경로가 필요함");
        if (OperatingSystem.IsLinux())
        {
            Directory.CreateDirectory(path, UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute);
        }
        else { Directory.CreateDirectory(path); }
    }

    private static bool VerifyTree(string root, ExpectedEntry[] expected, Observation report)
    {
        var wanted = new HashSet<string>(StringComparer.Ordinal);
        foreach (var entry in expected)
        {
            var name = entry.Name.TrimEnd('/');
            var parts = name.Split('/');
            for (var i = 1; i <= parts.Length; i++) { wanted.Add(string.Join('/', parts.Take(i))); }
            var path = Path.Combine(root, name.Replace('/', Path.DirectorySeparatorChar));
            if (entry.Kind == "Directory")
            {
                if (!Directory.Exists(path)) { return false; }
                report.Entries.Add(new(entry.Name, "Directory", 0, Data.Hash([]), 0, null, "Passed"));
            }
            else
            {
                if (!File.Exists(path) || (File.GetAttributes(path) & FileAttributes.ReparsePoint) != 0) { return false; }
                var data = File.ReadAllBytes(path);
                var hash = Data.Hash(data);
                var crc = ZipFixture.Crc(data);
                report.Entries.Add(new(entry.Name, "File", data.LongLength, hash, crc, null, "Passed"));
                if (hash != entry.Sha256 || data.LongLength != entry.Length || crc != entry.Crc32) { return false; }
            }
        }
        var actual = new HashSet<string>(StringComparer.Ordinal);
        void Visit(string folder)
        {
            Check.That((File.GetAttributes(folder) & FileAttributes.ReparsePoint) == 0, "게시 트리 링크 발견");
            foreach (var path in Directory.GetFileSystemEntries(folder))
            {
                var attributes = File.GetAttributes(path);
                Check.That((attributes & FileAttributes.ReparsePoint) == 0, "게시 항목 링크 발견");
                actual.Add(Path.GetRelativePath(root, path).Replace(Path.DirectorySeparatorChar, '/'));
                if ((attributes & FileAttributes.Directory) != 0) { Visit(path); }
            }
        }
        Visit(root);
        return actual.SetEquals(wanted);
    }
}

internal sealed class CancelReadStream(byte[] bytes, CancellationTokenSource cancellation) : MemoryStream(bytes, false)
{
    public override ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken token = default)
    {
        cancellation.Cancel();
        return base.ReadAsync(buffer, token);
    }
}

using System.Security.Cryptography;
using Zipelo.Core;
using Zipelo.Runtime.Streaming;
using Zipelo.StreamingExtractionTests;

namespace Zipelo.PublicationBarrierTests;

internal sealed record ProbePlan(string Mode, string RunDirectory, Invocation Invocation);
internal sealed class ProbeReport
{
    public string Scope { get; set; } = "고정 Rust worker + 파생 시험 host";
    public string WorkerCommit { get; set; } = Probe.WorkerCommit;
    public string WorkerSha256 { get; set; } = Probe.WorkerHash;
    public string InputSha256 { get; set; } = Probe.InputHash;
    public string Mode { get; set; } = "";
    public Guid JobId { get; set; } = Guid.NewGuid();
    public string BarrierId { get; set; } = "";
    public string Verdict { get; set; } = "fail";
    public string Detail { get; set; } = "not_completed";
    public bool StopRequired { get; set; } = true;
    public bool PreservationConfirmed { get; set; }
    public bool CleanupConfirmed { get; set; }
    public bool WorkerExitConfirmed { get; set; }
    public bool DiagnosticsComplete { get; set; }
    public bool BarrierConfirmed { get; set; }
    public int WorkerPid { get; set; }
    public StreamingExtractionResult? Result { get; set; }
    public string Error => Result?.Publication.Error.ToString() ?? "not_started";
    public bool? Published => Result?.Publication.Published;
}

internal static class Probe
{
    internal const string WorkerCommit = "9626abbf22f3a4c66a996e64601694ed5fe98763";
    internal const string WorkerHash = "62e157e304b01cac7c99a8eb60a147066877cf141b0314db5cf1a76aab55a5f7";
    internal const string InputHash = "a6e206b9f9fefd406d765562664ce023aa30dcacf66e942e264e276d95600b51";
    internal const string PayloadHash = "237356e18b503616912abb8ffaed3a72591e397d4ac294c4637917d48a3f529d";

    internal static void Validate(ProbePlan plan)
    {
        Check.That(OperatingSystem.IsLinux() && System.Runtime.InteropServices.RuntimeInformation.ProcessArchitecture ==
            System.Runtime.InteropServices.Architecture.X64, "Linux x64 전용 시험임");
        Check.That(plan.Mode is "control" or "cancel-new" or "cancel-existing", "사례 이름 오류");
        var call = plan.Invocation;
        Check.That(call.Kind == "native" && call.Executable == call.Worker &&
            call.ExecutableSha256 == WorkerHash && call.WorkerSha256 == WorkerHash &&
            !call.Worker.EndsWith(".dll", StringComparison.OrdinalIgnoreCase) &&
            call.Arguments.SequenceEqual(new[] { "--extract-stream-v2" }), "고정 실제 Rust 호출 필요");
        Evidence.VerifyFiles(call);
        Check.That(Path.IsPathFullyQualified(plan.RunDirectory) && Directory.Exists(plan.RunDirectory),
            "시험 소유 절대 경로 필요");
        NoLinks(plan.RunDirectory);
    }

    internal static void NoLinks(string path)
    {
        var full = Path.GetFullPath(path);
        Check.That((File.GetAttributes(full) & FileAttributes.ReparsePoint) == 0, "링크를 허용하지 않음");
        for (var parent = Directory.GetParent(full); parent is not null; parent = parent.Parent)
        {
            Check.That((parent.Attributes & FileAttributes.ReparsePoint) == 0, "조상 링크를 허용하지 않음");
        }
    }

    private static void MakeDirectory(string path)
    {
        Check.That(!Path.Exists(path), "기존 경로 재사용 금지");
        if (!OperatingSystem.IsLinux()) { throw new PlatformNotSupportedException(); }
        Directory.CreateDirectory(path, UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute);
    }

    internal static void Names(string path, params string[] names)
    {
        NoLinks(path);
        Check.That(Directory.EnumerateFileSystemEntries(path).Select(item => Path.GetFileName(item)).Order(StringComparer.Ordinal)
            .SequenceEqual(names.Order(StringComparer.Ordinal)), "정확한 경로 목록 불일치");
    }

    private static string HashFile(string path)
    {
        NoLinks(path);
        using var input = File.OpenRead(path);
        return Convert.ToHexString(SHA256.HashData(input)).ToLowerInvariant();
    }

    internal static async Task Run(ProbePlan plan, ProbeReport report)
    {
        Validate(plan);
        var bytes = ZipFixture.Build(new FixtureEntry("payload.bin", ZipFixture.Payload()));
        Check.That(Data.Hash(bytes) == InputHash, "기존 stored-chunk 입력 불일치");
        var sourcePath = Path.Combine(plan.RunDirectory, "source.zip");
        using (var file = new FileStream(sourcePath, FileMode.CreateNew)) { file.Write(bytes); }
        var parent = Path.Combine(plan.RunDirectory, "destination");
        MakeDirectory(parent);
        var marker = Path.Combine(parent, "keep.txt");
        using (var file = new FileStream(marker, FileMode.CreateNew)) { file.Write("preserved sibling\n"u8); }
        var markerHash = HashFile(marker);
        var destination = Path.Combine(parent, "result");
        string? existingHash = null;
        if (plan.Mode == "cancel-existing")
        {
            MakeDirectory(destination);
            var existing = Path.Combine(destination, "existing.txt");
            using (var file = new FileStream(existing, FileMode.CreateNew)) { file.Write("existing target\n"u8); }
            existingHash = HashFile(existing);
        }
        using var source = new FileStream(sourcePath, FileMode.Open, FileAccess.Read, FileShare.Read);
        using var cancellation = CancellationTokenSource.CreateLinkedTokenSource(CaseLifetime.Token);
        var barrier = Barrier.Install(report.JobId);
        report.BarrierId = barrier.Nonce;
        var limits = new JobLimits(1024, 128L * 1024 * 1024, 16L * 1024 * 1024, TimeSpan.FromSeconds(15), 1);
        var client = new ObservedWorkerClient(new StreamingZipWorkerClient(plan.Invocation.Worker));
        var pending = Task.Run(() => client.ExtractToNewFolderAsync(report.JobId, source, destination, limits,
            cancellation.Token));
        try
        {
            await Task.WhenAny(pending, barrier.Arrival.Task);
            Check.That(barrier.Arrival.Task.IsCompletedSuccessfully, "게시 장벽에 도달하지 못함");
            var temporary = await barrier.Arrival.Task;
            Check.That(Path.GetFileName(temporary) == temporary && temporary.StartsWith(".zipelo-", StringComparison.Ordinal),
                "작업 임시 이름 오류");
            var staging = Path.Combine(parent, temporary);
            Names(staging, "payload.bin");
            Check.That(HashFile(Path.Combine(staging, "payload.bin")) == PayloadHash, "게시 전 출력 내용 오류");
            Names(parent, plan.Mode == "cancel-existing" ? ["keep.txt", "result", temporary] : ["keep.txt", temporary]);
            Check.That(source.Position == 0 && HashFile(sourcePath) == InputHash &&
                HashFile(marker) == markerHash, "장벽 도달 시 원본 보존 실패");
            if (existingHash is not null)
            {
                Names(destination, "existing.txt");
                Check.That(HashFile(Path.Combine(destination, "existing.txt")) == existingHash, "기존 대상 변경");
            }
            barrier.Record("probe.snapshot.confirmed");
            if (plan.Mode != "control")
            {
                await cancellation.CancelAsync();
                Check.That(cancellation.IsCancellationRequested, "취소 요청 완료 미확인");
                barrier.Record("probe.cancel.confirmed");
            }
            else
            {
                Check.That(!cancellation.IsCancellationRequested, "무취소 대조군 취소됨");
                barrier.Record("probe.control.confirmed");
            }
            barrier.Release();
            var result = await pending;
            report.Result = result;
            report.WorkerExitConfirmed = result.WorkerExitConfirmed && result.WorkerExitCode == 0;
            report.CleanupConfirmed = !result.InputCleanupFailed && !result.Publication.CleanupFailed &&
                result.InputTemporaryName is null && result.Publication.TemporaryFileName is null;
            Check.That(barrier.Returned && result.JobId == report.JobId && result.VerifiedEntries == 1 &&
                report.WorkerExitConfirmed && report.CleanupConfirmed, "장벽 반환 또는 종료와 정리 미확인");
            var control = plan.Mode == "control";
            Check.That(result.Publication.Published == control && result.Publication.Error ==
                (control ? ArchiveError.None : ArchiveError.Cancelled), "원래 최종 검사 결과 불일치");
            if (control)
            {
                Names(destination, "payload.bin");
                Check.That(HashFile(Path.Combine(destination, "payload.bin")) == PayloadHash, "대조군 게시 출력 오류");
            }
            else if (existingHash is not null)
            {
                Names(destination, "existing.txt");
                Check.That(HashFile(Path.Combine(destination, "existing.txt")) == existingHash, "기존 대상 보존 실패");
            }
            else { Check.That(!Path.Exists(destination), "취소 후 게시됨"); }
            Names(parent, control || existingHash is not null ? ["keep.txt", "result"] : ["keep.txt"]);
            Check.That(source.Position == 0 && HashFile(sourcePath) == InputHash &&
                HashFile(marker) == markerHash, "원본 또는 기존 표식 보존 실패");
            report.PreservationConfirmed = true;
            Evidence.VerifyFiles(plan.Invocation);
        }
        finally
        {
            // 실패 시 장벽을 열어 종료를 유도하되 취소 성공으로 인정하지 않음.
            if (!pending.IsCompleted)
            {
                barrier.FailAndRelease();
                await cancellation.CancelAsync();
                await pending;
            }
        }
    }
}

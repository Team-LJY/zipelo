using System.Diagnostics;
using System.Globalization;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;

namespace Zipelo.StreamingExtractionTests;

internal sealed record Invocation(string Kind, string Executable, string ExecutableSha256,
    string Worker, string WorkerSha256, string[] Arguments);
internal sealed record TraceRow(long Ticks, Guid Job, int HostPid, string Stage, long Value, string Identity);

internal static class Evidence
{
    internal static void WritePeer(string path, PeerPlan plan)
    {
        using var stream = new FileStream(path, FileMode.CreateNew, FileAccess.Write, FileShare.Read);
        JsonSerializer.Serialize(stream, plan);
    }

    internal static Invocation Planned(CasePlan plan)
    {
        var managed = plan.Worker.EndsWith(".dll", StringComparison.OrdinalIgnoreCase);
        var arguments = new List<string>();
        if (managed) { arguments.Add(plan.Worker); }
        if (plan.Case.Operation != "list") { arguments.Add("--extract-stream-v2"); }
        return new(managed ? "dotnet-managed" : "native", managed ? plan.Dotnet : plan.Worker,
            managed ? plan.DotnetSha256 : plan.WorkerSha256, plan.Worker, plan.WorkerSha256, arguments.ToArray());
    }

    internal static bool Same(Invocation left, Invocation right) =>
        left.Kind == right.Kind && left.Executable == right.Executable &&
        left.ExecutableSha256 == right.ExecutableSha256 && left.Worker == right.Worker &&
        left.WorkerSha256 == right.WorkerSha256 && left.Arguments.SequenceEqual(right.Arguments);

    internal static void Validate(CasePlan plan)
    {
        Check.That(Same(Planned(plan), plan.Invocation), "실행 계획과 호출 방식 불일치");
        if (plan.Implementation is "csharp" or "host-peer" or "contract")
        {
            Check.That(plan.Invocation.Kind == "dotnet-managed" &&
                Path.GetFileNameWithoutExtension(plan.Dotnet).Equals("dotnet", StringComparison.OrdinalIgnoreCase),
                "C#은 고정 dotnet과 managed DLL로 실행해야 함");
        }
        else { Check.That(plan.Implementation == "rust" && plan.Invocation.Kind == "native", "후보 호출 방식 오류"); }
        VerifyFiles(plan.Invocation);
    }

    internal static void VerifyFiles(Invocation invocation)
    {
        foreach (var item in new[] { (invocation.Executable, invocation.ExecutableSha256),
            (invocation.Worker, invocation.WorkerSha256) })
        {
            Check.That(Path.IsPathFullyQualified(item.Item1), "절대 실행 경로가 필요함");
            var file = new FileInfo(item.Item1);
            Check.That(file.Exists && (file.Attributes & FileAttributes.ReparsePoint) == 0, "일반 실행 파일이 필요함");
            for (var parent = file.Directory; parent is not null; parent = parent.Parent)
            {
                Check.That((parent.Attributes & FileAttributes.ReparsePoint) == 0, "실행 경로 링크를 허용하지 않음");
            }
            Check.That(Data.Hash(File.ReadAllBytes(item.Item1)) == item.Item2, "실행 파일 해시 불일치");
        }
    }

    internal static string PathIdentity(string path) =>
        Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(path)));

    internal static TraceRow[] Parse(string[] lines, Guid job, int hostPid)
    {
        Check.That(job != Guid.Empty && hostPid > 0 && lines.Length > 0, "진단 작업과 시작 행이 필요함");
        var rows = new List<TraceRow>();
        long startTicks = -1;
        for (var index = 0; index < lines.Length; index++)
        {
            var parts = lines[index].Split('\t');
            Check.That(parts.Length == 7, "진단 필드 수 오류");
            var row = new TraceRow(long.Parse(parts[0], CultureInfo.InvariantCulture),
                Guid.ParseExact(parts[1], "N"), int.Parse(parts[2], CultureInfo.InvariantCulture),
                parts[3], long.Parse(parts[4], CultureInfo.InvariantCulture), parts[6]);
            Check.That(row.HostPid == hostPid && row.Ticks >= 0, "진단 PID 또는 시각 불일치");
            if (index == 0)
            {
                // frozen 생산자의 첫 시작 행만 작업 ID 검사에서 제외함.
                Check.That(row.Stage == "diagnostic.start" && row.Job == Guid.Empty &&
                    row.Value == Stopwatch.Frequency && parts[5] == "0" && row.Identity == "",
                    "진단 시작 행 계약 불일치");
                startTicks = row.Ticks;
                continue;
            }
            Check.That(row.Stage != "diagnostic.start" && row.Job == job && row.Ticks >= startTicks,
                "진단 작업 또는 시작 순서 불일치");
            rows.Add(row);
        }
        return rows.ToArray();
    }

    internal static int? Started(TraceRow[] rows)
    {
        var starts = rows.Where(row => row.Stage == "process.started").ToArray();
        Check.That(starts.Length <= 1, "작업자 시작 중복");
        if (starts.Length == 0) { return null; }
        Check.That(starts[0].Value > 0 && starts[0].Value <= int.MaxValue, "작업자 PID 오류");
        return (int)starts[0].Value;
    }

    internal static bool StreamIdentity(TraceRow[] rows, Invocation invocation, int pid)
    {
        var start = Array.FindIndex(rows, row => row.Stage == "process.started");
        var worker = Array.FindIndex(rows, row => row.Stage == "process.worker_identity");
        return start >= 0 && worker > start &&
            rows[start].Value == pid && rows[worker].Value == pid &&
            rows[start].Identity == PathIdentity(invocation.Executable) &&
            rows[worker].Identity == PathIdentity(invocation.Worker) &&
            rows.Count(row => row.Stage == "process.worker_identity") == 1;
    }

    internal static bool PeerReached(string mode, TraceRow[] rows, int? pid, int? peerPid, bool ready)
    {
        if (pid is null || pid != peerPid) { return false; }
        if (!mode.StartsWith("deadline-", StringComparison.Ordinal)) { return true; }
        if (!ready) { return false; }
        string[] stages = mode switch
        {
            "deadline-no-eof" => ["endjob.validated", "stdout.eof.begin"],
            "deadline-no-exit" => ["endjob.validated", "stdout.eof.begin", "stdout.eof.end", "process.exit.begin"],
            _ => []
        };
        if (stages.Length == 0) { return false; }
        var previous = Array.FindIndex(rows, row => row.Stage == "process.started");
        if (previous < 0) { return false; }
        foreach (var stage in stages)
        {
            var matches = rows.Select((row, index) => (row, index)).Where(item => item.row.Stage == stage).ToArray();
            if (matches.Length != 1 || matches[0].index <= previous ||
                matches[0].row.Ticks < rows[previous].Ticks) { return false; }
            previous = matches[0].index;
        }
        // 대기 종료까지 통과한 다른 실패를 의도한 정지로 오인하지 않음.
        var forbidden = mode == "deadline-no-eof" ? "stdout.eof.end" : "process.exit.end";
        return !rows.Any(row => row.Stage == forbidden);
    }

    internal static void ConfirmPeer(Observation report, string mode, TraceRow[] rows,
        int? pid, int? peerPid, bool ready)
    {
        var reached = PeerReached(mode, rows, pid, peerPid, ready);
        report.InjectionConfirmed &= reached;
        if ((!reached || report.WorkerExitCode == 71) && !report.StopRequired)
        {
            report.Verdict = "blocked";
            report.Detail = "peer_stage_injection_unconfirmed";
        }
    }

    internal static bool Ready(string root)
    {
        var path = Path.Combine(root, "ready");
        return File.Exists(path) && new FileInfo(path).Length == 1 &&
            (File.GetAttributes(path) & FileAttributes.ReparsePoint) == 0 && File.ReadAllText(path) == "1";
    }
}

internal static class Safety
{
    internal static void Fail(Observation report, Exception error)
    {
        report.Verdict = "fail";
        report.Detail = "harness_exception:" + error.GetType().Name;
        report.CleanupConfirmed = false;
        report.IdentityConfirmed = false;
        report.StopRequired = true;
    }

    internal static void Verify(Observation report, Func<bool> verify)
    {
        report.CleanupConfirmed = false;
        try
        {
            report.CleanupConfirmed = verify();
            if (!report.CleanupConfirmed) { report.StopRequired = true; }
        }
        catch (Exception error)
        {
            Fail(report, error);
            throw;
        }
    }
}

using System.Buffers.Binary;
using System.Diagnostics;
using System.Globalization;

namespace Zipelo.StreamingExtractionTests;

internal static class Regression
{
    internal static async Task<bool> FrozenReader(string root, string dotnet, bool pascal)
    {
        Directory.CreateDirectory(root);
        var job = Guid.NewGuid();
        var plan = new PeerPlan(job, "unsupported");
        if (pascal) { Evidence.WritePeer(Path.Combine(root, "peer.json"), plan); }
        else { Data.WriteNew(Path.Combine(root, "peer.json"), plan); }
        var start = new ProcessStartInfo
        {
            FileName = dotnet,
            WorkingDirectory = root,
            UseShellExecute = false,
            RedirectStandardInput = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true
        };
        start.ArgumentList.Add(typeof(Regression).Assembly.Location);
        start.ArgumentList.Add("--extract-stream-v2");
        using var process = new Process { StartInfo = start };
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(10));
        Check.That(process.Start(), "frozen reader 시작 실패");
        var output = process.StandardOutput.BaseStream.CopyToAsync(Stream.Null, timeout.Token);
        var error = process.StandardError.BaseStream.CopyToAsync(Stream.Null, timeout.Token);
        try
        {
            var config = new byte[64];
            "ZXE2"u8.CopyTo(config);
            BinaryPrimitives.WriteInt32LittleEndian(config.AsSpan(4), 2);
            job.TryWriteBytes(config.AsSpan(8, 16));
            await process.StandardInput.BaseStream.WriteAsync(config, timeout.Token);
            process.StandardInput.Close();
            await process.WaitForExitAsync(timeout.Token);
            await Task.WhenAll(output, error);
            var pid = Path.Combine(root, "peer.pid");
            return pascal
                ? process.ExitCode == 2 && File.ReadAllText(pid) == process.Id.ToString(CultureInfo.InvariantCulture)
                : process.ExitCode == 71 && !File.Exists(pid);
        }
        finally
        {
            timeout.Cancel();
            if (!process.HasExited)
            {
                process.Kill(entireProcessTree: true);
                await process.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(5));
            }
            try { await Task.WhenAll(output, error).WaitAsync(TimeSpan.FromSeconds(1)); }
            catch (OperationCanceledException) { }
        }
    }

    internal static async Task<int> Run(string root, string dotnet)
    {
        Check.That(Path.IsPathFullyQualified(root) && Directory.Exists(root), "회귀 전용 경로가 필요함");
        var results = new List<string>();
        var cleanup = new List<Observation>();
        void Record(string id, bool passed)
        {
            Check.That(passed && !results.Contains(id), "직접 회귀 실패: " + id);
            results.Add(id);
        }
        static bool Rejects(Action action)
        {
            try { action(); return false; }
            catch (InvalidDataException) { return true; }
        }

        Record("peer.frozen-pascal-reader", await FrozenReader(Path.Combine(root, "pascal"), dotnet, true));
        Record("peer.frozen-camel-rejected", await FrozenReader(Path.Combine(root, "camel"), dotnet, false));

        async Task<bool> Preservation(string stage)
        {
            var area = Path.Combine(root, "preservation-" + stage);
            Directory.CreateDirectory(area);
            var worker = typeof(Regression).Assembly.Location;
            var workerHash = Data.Hash(File.ReadAllBytes(worker));
            var dotnetHash = Data.Hash(File.ReadAllBytes(dotnet));
            var preservationInvocation = new Invocation("dotnet-managed", dotnet, dotnetHash, worker, workerHash,
                [worker, "--extract-stream-v2"]);
            var plan = new CasePlan(new("regression." + stage, "empty", "blocked", "unavailable",
                "", "", "unavailable", "", false), new("empty", "", Data.Hash([]), []), "csharp",
                worker, workerHash, dotnet, dotnetHash, "", area, preservationInvocation);
            var report = new Observation { JobId = Guid.NewGuid().ToString(), CleanupConfirmed = true };
            var injected = false;
            try
            {
                await Cases.Run(plan, report, point =>
                {
                    if (point == stage)
                    {
                        injected = true;
                        throw new IOException("합성 보존 검사 I/O 실패");
                    }
                });
            }
            catch (IOException) { }
            cleanup.Add(report);
            return stage == "normal"
                ? !injected && report.CleanupConfirmed && !report.StopRequired
                : injected && !report.CleanupConfirmed && report.StopRequired && report.Verdict == "fail";
        }
        Record("cleanup.source-io", await Preservation("source"));
        Record("cleanup.marker-io", await Preservation("marker"));
        Record("cleanup.tree-io", await Preservation("tree"));
        Record("cleanup.normal-preservation", await Preservation("normal"));
        var prior = new Observation { CleanupConfirmed = true, Verdict = "pass" };
        Safety.Fail(prior, new IOException());
        Record("cleanup.late-body-exception", !prior.CleanupConfirmed && prior.StopRequired && prior.Verdict == "fail");

        var job = Guid.NewGuid();
        var call = new Invocation("dotnet-managed", "/dotnet", "a", "/worker.dll", "b",
            ["/worker.dll", "--extract-stream-v2"]);
        TraceRow[] Trace(params string[] stages) => stages.Select((stage, index) =>
            new TraceRow(index + 1, job, 101, stage, stage.StartsWith("process.", StringComparison.Ordinal) ? 202 : 0,
                stage == "process.started" ? Evidence.PathIdentity(call.Executable) :
                stage == "process.worker_identity" ? Evidence.PathIdentity(call.Worker) : "")).ToArray();
        var eof = Trace("process.started", "process.worker_identity", "endjob.validated", "stdout.eof.begin");
        var exit = Trace("process.started", "process.worker_identity", "endjob.validated", "stdout.eof.begin",
            "stdout.eof.end", "process.exit.begin");
        bool Reached(string mode, TraceRow[] rows, bool ready = true, int peerPid = 202) =>
            Evidence.PeerReached(mode, rows, Evidence.Started(rows), peerPid, ready);
        bool Delayed(string mode)
        {
            var report = new Observation { InjectionConfirmed = true, Verdict = "pass", Error = "BudgetExceeded" };
            Evidence.ConfirmPeer(report, mode, Trace("process.started", "process.worker_identity"), 202, 202, false);
            return !report.InjectionConfirmed && report.Verdict == "blocked";
        }
        Record("deadline.no-eof-ready-ordered", Reached("deadline-no-eof", eof));
        Record("deadline.no-exit-ready-ordered", Reached("deadline-no-exit", exit));
        Record("deadline.pid-only-no-eof", Delayed("deadline-no-eof"));
        Record("deadline.pid-only-no-exit", Delayed("deadline-no-exit"));
        Record("deadline.missing-ready", !Reached("deadline-no-eof", eof, false));
        Record("deadline.wrong-pid", !Reached("deadline-no-eof", eof, true, 203));
        Record("deadline.wrong-order", !Reached("deadline-no-exit",
            Trace("process.started", "stdout.eof.begin", "endjob.validated", "stdout.eof.end", "process.exit.begin")));
        Record("deadline.missing-terminal", !Reached("deadline-no-eof", Trace("process.started", "stdout.eof.begin")));
        Record("deadline.eof-already-ended", !Reached("deadline-no-eof", exit));
        Record("deadline.exit-already-ended", !Reached("deadline-no-exit",
            [.. exit, new(10, job, 101, "process.exit.end", 202, "")]));

        var native = call with { Kind = "native", Executable = "/native", Worker = "/native",
            ExecutableSha256 = "b", Arguments = ["--extract-stream-v2"] };
        var nativeRows = new[] { eof[0] with { Identity = Evidence.PathIdentity("/native") },
            eof[1] with { Identity = Evidence.PathIdentity("/native") } };
        Record("identity.native-trace", Evidence.StreamIdentity(nativeRows, native, 202));
        Record("identity.managed-trace", Evidence.StreamIdentity(eof, call, 202));
        Record("identity.executable-mismatch", !Evidence.StreamIdentity(eof, call with { Executable = "/other" }, 202));
        Record("identity.worker-mismatch", !Evidence.StreamIdentity(eof, call with { Worker = "/other.dll" }, 202));
        Record("identity.arguments-mismatch", !Evidence.Same(call, call with { Arguments = ["--extract-stream-v2"] }));
        string Line(TraceRow row) => FormattableString.Invariant(
            $"{row.Ticks}\t{row.Job:N}\t{row.HostPid}\t{row.Stage}\t{row.Value}\t0\t{row.Identity}");
        var lines = eof.Select(Line).ToArray();
        Record("identity.trace-job-mismatch", Rejects(() => Evidence.Parse(lines, Guid.NewGuid(), 101)));
        Record("identity.trace-host-mismatch", Rejects(() => Evidence.Parse(lines, job, 102)));
        Record("identity.trace-time-order", !Reached("deadline-no-eof",
            [eof[0], eof[1], eof[2] with { Ticks = 100 }, eof[3]]));
        var proof = new RelayProof(job, 101, 202, call, 0, true);
        Record("identity.relay-match", ListRelay.Matches(proof, job, call));
        Record("identity.relay-stale-job", !ListRelay.Matches(proof, Guid.NewGuid(), call));
        Record("identity.relay-hash-mismatch", !ListRelay.Matches(proof, job, call with { WorkerSha256 = "c" }));
        Record("identity.relay-args-mismatch", !ListRelay.Matches(proof, job, call with { Arguments = [] }));
        Data.WriteNew(Path.Combine(root, "regression.json"), new { cases = results, cleanup });
        return 0;
    }
}

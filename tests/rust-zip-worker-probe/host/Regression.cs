using System.Buffers.Binary;
using System.Diagnostics;
using System.Globalization;
using Zipelo.Core;
using Zipelo.Runtime.Streaming;

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

    internal static async Task<bool> FrozenDiagnostics(string root, string dotnet)
    {
        var path = Path.Combine(root, "frozen-diagnostics.tsv");
        var destination = Path.Combine(root, "diagnostic-uncreated-output");
        var job = Guid.NewGuid();
        Check.That(!File.Exists(destination) && !Directory.Exists(destination), "새 사전 취소 경로가 필요함");
        Check.That(StreamingExtractionDiagnostics.IsEnabled &&
            StreamingExtractionDiagnostics.TryStart(path), "frozen 진단 시작 실패");
        StreamingExtractionResult result;
        using var source = new MemoryStream(Array.Empty<byte>(), false);
        try
        {
            var client = new StreamingZipWorkerClient(typeof(Regression).Assembly.Location, dotnet);
            result = await client.ExtractToNewFolderAsync(job, source, destination,
                new JobLimits(1, 1024, 1024, TimeSpan.FromSeconds(2), 1),
                new CancellationToken(canceled: true)).WaitAsync(TimeSpan.FromSeconds(5));
        }
        finally
        {
            await StreamingExtractionDiagnostics.StopAsync().WaitAsync(TimeSpan.FromSeconds(1));
        }
        Check.That(StreamingExtractionDiagnostics.Status == 2 &&
            StreamingExtractionDiagnostics.Dropped == 0, "frozen 진단 종료 미확인");
        var lines = File.ReadAllLines(path);
        var rows = Evidence.Parse(lines, job, Environment.ProcessId);
        return lines.Length == rows.Length + 1 && rows.Length > 0 &&
            rows.All(row => row.Job == job && row.HostPid == Environment.ProcessId) &&
            rows.Any(row => row.Stage == "operation.start") &&
            rows.Any(row => row.Stage == "cleanup.end") && Evidence.Started(rows) is null &&
            result.JobId == job && result.Publication.Error == ArchiveError.Cancelled &&
            result.WorkerExitConfirmed && !result.InputCleanupFailed &&
            result.InputTemporaryName is null && !result.Publication.Published &&
            !result.Publication.CleanupFailed && result.Publication.TemporaryFileName is null &&
            source.Position == 0 && !File.Exists(destination) && !Directory.Exists(destination);
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
            var expectedFailure = new IOException("합성 보존 검사 I/O 실패");
            var observedFailure = false;
            using var lifetime = new CaseLifetime();
            var token = CaseLifetime.Token;
            Check.That(ReferenceEquals(CaseLifetime.Current, lifetime) &&
                token.CanBeCanceled && !token.IsCancellationRequested, "새 회귀 수명 범위가 필요함");
            var body = Task.Run(() => Cases.Run(plan, report, point =>
            {
                if (point == stage)
                {
                    injected = true;
                    throw expectedFailure;
                }
            }));
            try { await body.WaitAsync(TimeSpan.FromSeconds(45)); }
            catch (IOException error) when (ReferenceEquals(error, expectedFailure))
            {
                observedFailure = true;
            }
            finally
            {
                var stopped = await lifetime.StopAsync(body, plan.Case.Id, false);
                if (!stopped || !body.IsCompleted || !token.IsCancellationRequested)
                {
                    var failure = new InvalidOperationException("회귀 수명 종료 미확인");
                    Safety.Fail(report, failure);
                    throw failure;
                }
            }
            cleanup.Add(report);
            return stage == "normal"
                ? body.IsCompletedSuccessfully && !injected && !observedFailure &&
                    report.CleanupConfirmed && !report.StopRequired
                : body.IsFaulted && injected && observedFailure && !report.CleanupConfirmed &&
                    report.StopRequired && report.Verdict == "fail";
        }
        Record("cleanup.source-io", await Preservation("source"));
        Record("cleanup.marker-io", await Preservation("marker"));
        Record("cleanup.tree-io", await Preservation("tree"));
        Record("cleanup.normal-preservation", await Preservation("normal"));
        var prior = new Observation { CleanupConfirmed = true, Verdict = "pass" };
        Safety.Fail(prior, new IOException());
        Record("cleanup.late-body-exception", !prior.CleanupConfirmed && prior.StopRequired && prior.Verdict == "fail");

        Record("diagnostic.frozen-producer-lifecycle", await FrozenDiagnostics(root, dotnet));

        var job = Guid.NewGuid();
        var call = new Invocation("dotnet-managed", "/dotnet", "a", "/worker.dll", "b",
            ["/worker.dll", "--extract-stream-v2"]);
        var diagnosticStart = new TraceRow(0, Guid.Empty, 101, "diagnostic.start", Stopwatch.Frequency, "");
        TraceRow[] Trace(params string[] stages)
        {
            var work = stages.Select((stage, index) =>
                new TraceRow(index + 1, job, 101, stage, stage.StartsWith("process.", StringComparison.Ordinal) ? 202 : 0,
                    stage == "process.started" ? Evidence.PathIdentity(call.Executable) :
                    stage == "process.worker_identity" ? Evidence.PathIdentity(call.Worker) : "")).ToArray();
            return Evidence.Parse([Line(diagnosticStart), .. work.Select(Line)], job, 101);
        }
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
        string[] lines = [Line(diagnosticStart), .. eof.Select(Line)];
        Record("identity.trace-job-mismatch", Rejects(() => Evidence.Parse(lines, Guid.NewGuid(), 101)));
        Record("identity.trace-host-mismatch", Rejects(() => Evidence.Parse(lines, job, 102)));
        Record("identity.trace-time-order", !Reached("deadline-no-eof",
            [eof[0], eof[1], eof[2] with { Ticks = 100 }, eof[3]]));
        var proof = new RelayProof(job, 101, 202, call, 0, true);
        Record("identity.relay-match", ListRelay.Matches(proof, job, call));
        Record("identity.relay-stale-job", !ListRelay.Matches(proof, Guid.NewGuid(), call));
        Record("identity.relay-hash-mismatch", !ListRelay.Matches(proof, job, call with { WorkerSha256 = "c" }));
        Record("identity.relay-args-mismatch", !ListRelay.Matches(proof, job, call with { Arguments = [] }));
        Record("diagnostic.start-accepted", Evidence.Parse(lines, job, 101).SequenceEqual(eof));
        Record("diagnostic.start-only", Evidence.Parse([Line(diagnosticStart)], job, 101).Length == 0);
        Record("diagnostic.start-missing", Rejects(() => Evidence.Parse(eof.Select(Line).ToArray(), job, 101)));
        Record("diagnostic.start-duplicate", Rejects(() =>
            Evidence.Parse([Line(diagnosticStart), .. lines], job, 101)));
        Record("diagnostic.start-misplaced", Rejects(() =>
            Evidence.Parse([Line(eof[0]), Line(diagnosticStart), .. eof.Skip(1).Select(Line)], job, 101)));
        bool RejectStart(TraceRow start) =>
            Rejects(() => Evidence.Parse([Line(start), .. eof.Select(Line)], job, 101));
        Record("diagnostic.start-job", RejectStart(diagnosticStart with { Job = job }));
        Record("diagnostic.start-pid", RejectStart(diagnosticStart with { HostPid = 102 }));
        Record("diagnostic.start-frequency", RejectStart(diagnosticStart with { Value = Stopwatch.Frequency + 1 }));
        var badSequence = Line(diagnosticStart).Split('\t');
        badSequence[5] = "1";
        Record("diagnostic.start-sequence", Rejects(() =>
            Evidence.Parse([string.Join('\t', badSequence), .. eof.Select(Line)], job, 101)));
        Record("diagnostic.start-identity", RejectStart(diagnosticStart with { Identity = "forged" }));
        Record("diagnostic.start-negative-ticks", RejectStart(diagnosticStart with { Ticks = -1 }));
        Record("diagnostic.start-unknown-stage", RejectStart(diagnosticStart with { Stage = "diagnostic.other" }));
        Record("diagnostic.work-job", Rejects(() =>
            Evidence.Parse([Line(diagnosticStart), Line(eof[0] with { Job = Guid.NewGuid() })], job, 101)));
        Record("diagnostic.work-empty-job", Rejects(() =>
            Evidence.Parse([Line(diagnosticStart), Line(eof[0] with { Job = Guid.Empty })], job, 101)));
        Record("diagnostic.work-pid", Rejects(() =>
            Evidence.Parse([Line(diagnosticStart), Line(eof[0] with { HostPid = 102 })], job, 101)));
        Record("diagnostic.work-before-start", Rejects(() =>
            Evidence.Parse([Line(diagnosticStart with { Ticks = 10 }), Line(eof[0])], job, 101)));
        async Task<bool> NativeWire()
        {
            var wireJob = Guid.NewGuid();
            var bytes = NativeExitPeer.Wire(wireJob, 70000);
            Check.That(bytes.Length == 66002 && bytes.AsSpan(0, 4).SequenceEqual("ZXE2"u8) &&
                BinaryPrimitives.ReadInt32LittleEndian(bytes.AsSpan(4)) == 2 &&
                new Guid(bytes.AsSpan(8, 16)) == wireJob &&
                BinaryPrimitives.ReadInt64LittleEndian(bytes.AsSpan(24)) == 70000 &&
                BinaryPrimitives.ReadInt64LittleEndian(bytes.AsSpan(32)) == 128L * 1024 * 1024 &&
                BinaryPrimitives.ReadInt64LittleEndian(bytes.AsSpan(40)) == 16L * 1024 * 1024 &&
                BinaryPrimitives.ReadInt64LittleEndian(bytes.AsSpan(48)) == 1024 &&
                BinaryPrimitives.ReadInt64LittleEndian(bytes.AsSpan(56)) == TimeSpan.FromSeconds(2).Ticks,
                "native config 계약 불일치");
            using var stream = new MemoryStream(bytes, false);
            stream.Position = 64;
            using var channel = new WorkerStreamChannel(stream, WorkerStreamDirection.Read, wireJob);
            var frames = new List<WorkerStreamFrame>();
            while (await channel.ReadAsync() is { } frame) { frames.Add(frame); }
            ushort[] kinds = [1, 2, 3, 3, 4, 5];
            int[] lengths = [0, 8, 65536, 1, 8, 1];
            var item = new Guid("00000001-0000-0000-0000-000000000000");
            return frames.Count == 6 && frames.Select((frame, index) =>
                (ushort)frame.Header.Kind == kinds[index] && frame.Header.Sequence == (ulong)index &&
                frame.Header.JobId == wireJob && frame.Header.ItemId == (index is 0 or 5 ? Guid.Empty : item) &&
                frame.Header.PayloadLength == lengths[index]).All(value => value) &&
                BinaryPrimitives.ReadInt64LittleEndian(frames[1].Payload.Span) == 65537 &&
                BinaryPrimitives.ReadInt64LittleEndian(frames[4].Payload.Span) == 65537 &&
                frames[2].Payload.ToArray().Concat(frames[3].Payload.ToArray()).SequenceEqual(ZipFixture.Payload()) &&
                frames[5].Payload.Span.SequenceEqual(new byte[] { 1 }) &&
                channel.Phase == WorkerStreamPhase.InputCompleted &&
                channel.TerminalStatus == WorkerStreamTerminalStatus.Completed &&
                channel.FramesAccepted == 6 && channel.TotalDataBytes == 65537 &&
                channel.TotalWireBytes == NativeExitPeer.WireBytes;
        }
        Record("native.wire-frozen-reader", await NativeWire());
        var nativeProof = new byte[NativeExitPeer.ProofBytes];
        "NXP1"u8.CopyTo(nativeProof);
        BinaryPrimitives.WriteUInt32LittleEndian(nativeProof.AsSpan(4), 1);
        job.TryWriteBytes(nativeProof.AsSpan(8, 16));
        BinaryPrimitives.WriteUInt32LittleEndian(nativeProof.AsSpan(24), 202);
        BinaryPrimitives.WriteUInt32LittleEndian(nativeProof.AsSpan(28), 101);
        BinaryPrimitives.WriteUInt64LittleEndian(nativeProof.AsSpan(32), 8);
        BinaryPrimitives.WriteUInt64LittleEndian(nativeProof.AsSpan(40), 1234);
        BinaryPrimitives.WriteUInt32LittleEndian(nativeProof.AsSpan(48), 1);
        BinaryPrimitives.WriteUInt32LittleEndian(nativeProof.AsSpan(56), NativeExitPeer.WireBytes);
        BinaryPrimitives.WriteUInt32LittleEndian(nativeProof.AsSpan(60), 1);
        BinaryPrimitives.WriteUInt64LittleEndian(nativeProof.AsSpan(64), 100_000_000);
        BinaryPrimitives.WriteUInt64LittleEndian(nativeProof.AsSpan(72), 150_000_000);
        bool NativeProof(byte[] bytes) => NativeExitPeer.ProofMatches(bytes, job, 202, 101);
        bool RejectProof(int offset, byte value)
        {
            var changed = nativeProof.ToArray();
            changed[offset] = value;
            return !NativeProof(changed);
        }
        Record("native.proof-valid", NativeProof(nativeProof));
        Record("native.proof-length", !NativeProof(nativeProof[..95]));
        Record("native.proof-job", !NativeExitPeer.ProofMatches(nativeProof, Guid.NewGuid(), 202, 101));
        Record("native.proof-pid", !NativeExitPeer.ProofMatches(nativeProof, job, 203, 101));
        Record("native.proof-parent", !NativeExitPeer.ProofMatches(nativeProof, job, 202, 102));
        Record("native.proof-extra-writer", RejectProof(48, 2));
        Record("native.proof-writer-remains", RejectProof(52, 1));
        Record("native.proof-short-wire", RejectProof(56, 0));
        var noLiveness = nativeProof.ToArray();
        BinaryPrimitives.WriteUInt64LittleEndian(noLiveness.AsSpan(72), 100_000_000);
        Record("native.proof-no-liveness", !NativeProof(noLiveness));
        Record("native.proof-version", RejectProof(4, 2));
        Record("native.proof-phase", RejectProof(60, 0));
        Record("native.proof-reserved", RejectProof(95, 1));

        Data.WriteNew(Path.Combine(root, "regression.json"), new { cases = results, cleanup });
        return 0;
    }
}

using Zipelo.PublicationBarrierTests;
using Zipelo.Runtime.Streaming;
using Zipelo.StreamingExtractionTests;

if (args is not ["--gate", var gate, "--plan", var planPath]) { return 2; }
await Data.Gate(gate);
var plan = Data.Read<ProbePlan>(planPath, 16 * 1024);
Probe.Validate(plan);
var root = Path.GetFullPath(plan.RunDirectory);
Check.That(Path.GetDirectoryName(Path.GetFullPath(planPath)) == root &&
    Path.GetDirectoryName(Path.GetFullPath(gate)) == root, "제어 파일 경로 불일치");
Probe.Names(root, "gate", "plan.json");
Environment.CurrentDirectory = root;
var report = new ProbeReport { Mode = plan.Mode };
var diagnostic = Path.Combine(root, "diagnostics.tsv");
try
{
    Check.That(StreamingExtractionDiagnostics.IsEnabled && StreamingExtractionDiagnostics.TryStart(diagnostic),
        "진단 시작 실패");
    using var lifetime = new CaseLifetime();
    var body = Task.Run(() => Probe.Run(plan, report));
    Exception? failure = null;
    try { await body.WaitAsync(TimeSpan.FromSeconds(45)); }
    catch (Exception error) { failure = error; }
    var stopped = await lifetime.StopAsync(body, plan.Mode, failure is not null);
    Check.That(stopped && failure is null, "작업 또는 종료 확인 실패: " + failure?.GetType().Name);
    await StreamingExtractionDiagnostics.StopAsync().WaitAsync(TimeSpan.FromSeconds(1));
    report.DiagnosticsComplete = StreamingExtractionDiagnostics.Status == 2 && StreamingExtractionDiagnostics.Dropped == 0;
    Check.That(report.DiagnosticsComplete && new FileInfo(diagnostic).Length <= 4 * 1024 * 1024, "진단 완료 실패");
    TraceProof.Verify(File.ReadAllLines(diagnostic), plan, report);
    Evidence.VerifyFiles(plan.Invocation);
    Check.That(report.PreservationConfirmed && report.CleanupConfirmed && report.WorkerExitConfirmed &&
        report.BarrierConfirmed, "필수 검증 미완료");
    report.Verdict = "pass";
    report.Detail = "derived_host_boundary_only";
    report.StopRequired = false;
}
catch (Exception error)
{
    report.Detail = error.GetType().Name + ": " + error.Message;
    report.StopRequired = true;
}
Data.WriteNew(Path.Combine(root, "observation.json"), report);
return report.StopRequired ? 124 : 0;

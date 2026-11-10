using System.Diagnostics;
using Zipelo.StreamingExtractionTests;
using Zipelo.Runtime.Streaming;

if (args is ["--extract-stream-v2"]) { return await FakeWorker.Run(); }
if (args is not ["--gate", var gate, "--plan", var planPath]) { return 2; }
await Data.Gate(gate);
var plan = Data.Read<CasePlan>(planPath, 4 * 1024 * 1024);
var root = Path.GetFullPath(plan.RunDirectory);
Check.That(Path.IsPathFullyQualified(plan.RunDirectory) && Directory.Exists(root) &&
    Path.GetDirectoryName(Path.GetFullPath(planPath)) == root, "시험 계획 경로 불일치");
for (DirectoryInfo? current = new(root); current is not null; current = current.Parent)
{
    Check.That((current.Attributes & FileAttributes.ReparsePoint) == 0, "시험 조상의 링크를 허용하지 않음");
}
Environment.CurrentDirectory = root;
var observation = new Observation
{
    CaseId = plan.Case.Id,
    Implementation = plan.Implementation,
    SourceCommit = plan.SourceCommit,
    BinarySha256 = plan.WorkerSha256,
    InputSha256 = plan.Sample.Sha256,
    OsPolicy = OperatingSystem.IsWindows() ? "windows" : "linux",
    JobId = Guid.NewGuid().ToString(),
    Injection = plan.Case.Injection,
    FaultKind = plan.Case.FaultKind
};
var watch = Stopwatch.StartNew();
var diagnostic = Path.Combine(root, "diagnostics.tsv");
if (!StreamingExtractionDiagnostics.IsEnabled || !StreamingExtractionDiagnostics.TryStart(diagnostic))
{
    observation.Verdict = "blocked";
    observation.Detail = "diagnostic_start_failed";
    observation.StopRequired = true;
}
else
{
    using var lifetime = new CaseLifetime();
    var body = Task.Run(() => Cases.Run(plan, observation));
    Exception? failure = null;
    try { await body.WaitAsync(TimeSpan.FromSeconds(45)); }
    catch (Exception error) { failure = error; }
    var stopped = await lifetime.StopAsync(body, plan.Case.Id, failure is not null);
    if (!stopped || failure is TimeoutException)
    {
        observation.Verdict = "fail";
        observation.Detail = "operation_or_shutdown_timeout";
        observation.StopRequired = true;
        observation.WorkerExitConfirmed = false;
        observation.CleanupConfirmed = false;
    }
    else if (failure is not null)
    {
        observation.Verdict = "fail";
        observation.Detail = "harness_exception:" + failure.GetType().Name;
    }
    try
    {
        await StreamingExtractionDiagnostics.StopAsync().WaitAsync(TimeSpan.FromSeconds(1));
        observation.DiagnosticsComplete = StreamingExtractionDiagnostics.Status == 2 &&
            StreamingExtractionDiagnostics.Dropped == 0;
        if (observation.DiagnosticsComplete && plan.Case.Operation != "list")
        {
            var lines = File.ReadAllLines(diagnostic);
            observation.WorkerStarted = lines.Any(line => line.Split('\t').ElementAtOrDefault(3) == "process.started");
            if (plan.Case.Operation == "peer")
            {
                var peerPid = Path.Combine(root, "peer.pid");
                var observedPids = lines.Select(line => line.Split('\t'))
                    .Where(parts => parts.Length >= 5 && parts[3] == "process.started")
                    .Select(parts => parts[4]).ToArray();
                var reached = File.Exists(peerPid) &&
                    observedPids.Contains(File.ReadAllText(peerPid).Trim(), StringComparer.Ordinal);
                observation.InjectionConfirmed &= reached;
                if (!reached || observation.WorkerExitCode == 71)
                {
                    observation.Verdict = "blocked";
                    observation.Detail = "peer_injection_unconfirmed";
                }
            }
            observation.CoverageLayer = plan.Case.Operation == "peer" ? "host_fault_peer" :
                observation.WorkerStarted == true ? "host_and_worker" : "host_preflight";
        }
    }
    catch { observation.DiagnosticsComplete = false; }
    if (!observation.DiagnosticsComplete)
    {
        observation.Verdict = "fail";
        observation.StopRequired = true;
        observation.Detail = "diagnostics_unconfirmed";
    }
}
observation.ElapsedMilliseconds = watch.Elapsed.TotalMilliseconds;
if (!observation.CleanupConfirmed || observation.WorkerExitConfirmed == false) { observation.StopRequired = true; }
Data.WriteNew(Path.Combine(root, "observation.json"), observation);
return observation.StopRequired ? 124 : observation.Verdict == "pass" ? 0 : observation.Verdict == "blocked" ? 3 : 1;

using Zipelo.Core;
using Zipelo.StreamingExtractionTests;

namespace Zipelo.PublicationBarrierTests;

internal static class TraceProof
{
    internal static void Verify(string[] lines, ProbePlan plan, ProbeReport report)
    {
        var rows = Evidence.Parse(lines, report.JobId, Environment.ProcessId);
        TraceRow One(string stage)
        {
            var matches = rows.Where(row => row.Stage == stage).ToArray();
            Check.That(matches.Length == 1, "진단 누락 또는 중복: " + stage);
            return matches[0];
        }
        void Ordered(params string[] stages)
        {
            var previous = -1;
            long ticks = -1;
            foreach (var stage in stages)
            {
                var row = One(stage);
                var index = Array.IndexOf(rows, row);
                Check.That(index > previous && row.Ticks >= ticks, "단계 순서 오류: " + stage);
                previous = index;
                ticks = row.Ticks;
            }
        }
        var pid = Evidence.Started(rows);
        Check.That(pid is not null && Evidence.StreamIdentity(rows, plan.Invocation, pid.Value),
            "실제 worker identity 불일치");
        report.WorkerPid = pid!.Value;
        var control = plan.Mode == "control";
        var decision = control ? "probe.control.confirmed" : "probe.cancel.confirmed";
        Ordered("process.started", "process.worker_identity", "file.verify.end", "endjob.validated",
            "stdout.eof.begin", "stdout.eof.end", "process.exit.begin", "process.exit.end",
            "range.await.begin", "range.await.end", "input.cleanup.begin", "input.cleanup.end",
            "output.verify.begin", "output.verify.end", "output.publish.begin",
            "probe.barrier.arrive", "probe.snapshot.confirmed", decision,
            "probe.barrier.release", "probe.barrier.return", "cleanup.begin", "cleanup.exit.end",
            "cleanup.range.end", "owned.dispose.begin", "owned.dispose.end",
            "process.dispose.begin", "process.dispose.end", "cleanup.end");
        foreach (var stage in new[] { "probe.barrier.arrive", "probe.snapshot.confirmed", decision,
            "probe.barrier.release", "probe.barrier.return" })
        {
            Check.That(One(stage).Identity == report.BarrierId, "장벽 nonce 불일치");
        }
        Check.That(One("process.exit.end").Value == 0 && One("input.cleanup.end").Value == 0 &&
            One("deadline.scheduled_ms").Value == 15000 &&
            One("probe.barrier.return").Value == (control ? 0 : 1), "종료 또는 취소 토큰 상태 오류");
        var cancelled = One("operation.cancelled");
        Check.That(cancelled.Value == (control ? 0 : 1), "취소 원인 불일치");
        if (control)
        {
            Ordered("probe.barrier.return", "output.publish.end", "cleanup.begin", "operation.cancelled");
            Check.That(!rows.Any(row => row.Stage == "output.abort.begin"), "성공 뒤 Abort");
        }
        else
        {
            Ordered("probe.snapshot.confirmed", "operation.cancelled", "probe.cancel.confirmed",
                "probe.barrier.release", "probe.barrier.return", "cleanup.begin", "output.abort.begin",
                "output.abort.end", "owned.dispose.begin");
            Check.That(!rows.Any(row => row.Stage == "output.publish.end") &&
                One("output.abort.end").Value == 0, "취소 뒤 게시 또는 Abort 실패");
        }
        Check.That(One("cleanup.end").Value == (long)(control ? ArchiveError.None : ArchiveError.Cancelled) &&
            !rows.Any(row => row.Stage is "range.fault" or "input.abort.begin"), "의도하지 않은 실패 경로");
        report.BarrierConfirmed = true;
    }
}

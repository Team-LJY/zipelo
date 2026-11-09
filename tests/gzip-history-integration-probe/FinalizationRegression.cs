using Zipelo.GzipBoundaryProbe;

internal static class FinalizationRegression
{
    public static void Register(Action<string, Action> run)
    {
        foreach (var selected in new[] { false, true })
        {
            foreach (var scenario in new[] { "success", "body-budget", "body-cancel", "body-both", "dispose-both" })
            {
                run($"finalization.dispose.{scenario}.{selected}", () =>
                {
                    VerifyDispose(selected, scenario, false);
                    VerifyDispose(selected, scenario, true);
                });
            }
            foreach (var delta in new long[] { -1, ProbeBudget.ChunkSize + 1 })
            {
                for (var mode = 0; mode < 4; mode++)
                {
                    var capturedMode = mode;
                    run($"finalization.counter.{delta}.{mode}.{selected}", () =>
                        VerifyCounter(selected, delta, capturedMode));
                }
            }
        }
    }

    private static void VerifyDispose(bool selected, string scenario, bool throwOnDispose)
    {
        var fixture = Fixture.Wrap([0x01, 0x01, 0x00, 0xfe, 0xff, 0x41], [0x41], 0);
        using var cts = new CancellationTokenSource();
        var clock = new ManualClock();
        var budget = new ProbeBudget(new(Duration: TimeSpan.FromSeconds(1)), clock);
        var pending = new PendingValidation();
        var marker = new IOException("임시 수신기 Dispose 실패");
        var disposeCalls = 0;
        var beforeCommitCalls = 0;
        var bodyBudget = scenario is "body-budget" or "body-both";
        var bodyCancel = scenario is "body-cancel" or "body-both";
        var observer = new ObservedInflater(() =>
        {
            if (bodyBudget) { clock.Advance(TimeSpan.FromSeconds(1)); }
            if (bodyCancel) { cts.Cancel(); }
        });
        var error = Capture(() => pending.Execute(fixture.Bytes, budget, selected, cts.Token,
            beforeCommit: () => beforeCommitCalls++, inflater: observer, onSinkDispose: () =>
            {
                disposeCalls++;
                if (scenario == "dispose-both")
                {
                    clock.Advance(TimeSpan.FromSeconds(1));
                    cts.Cancel();
                }
                if (throwOnDispose) { throw marker; }
            }));
        Check(disposeCalls == 1 && observer.Calls == 1);
        Check(budget.Processed == fixture.Bytes.Length + 1);
        Check(budget.Output == (selected && !bodyBudget && !bodyCancel ? 1 : 0));
        if (throwOnDispose)
        {
            Check(ReferenceEquals(error, marker) && beforeCommitCalls == 0);
            Uncommitted(pending);
        }
        else if (scenario == "success")
        {
            Check(error == null && beforeCommitCalls == 1 && pending.CommitCount == 1);
            Check(pending.CommittedBoundary != null);
            Check(selected ? pending.CommittedHash != null : pending.CommittedHash == null);
        }
        else
        {
            Check(beforeCommitCalls == (scenario == "dispose-both" ? 1 : 0));
            Check(bodyCancel || scenario == "dispose-both"
                ? error is OperationCanceledException
                : error is ProbeException { Failure: Failure.Budget });
            Uncommitted(pending);
        }
        Check(Capture(() => pending.Execute(fixture.Bytes, budget, selected)) is InvalidOperationException);
        Check(disposeCalls == 1);
    }

    private static void VerifyCounter(bool selected, long delta, int mode)
    {
        var fixture = Fixture.Wrap([0x01, 0x01, 0x00, 0xfe, 0xff, 0x41], [0x41], 0);
        using var cts = new CancellationTokenSource();
        var clock = new ManualClock();
        var budget = new ProbeBudget(new(Duration: TimeSpan.FromSeconds(1)), clock);
        var observer = new CounterDeltaInflater(delta, () =>
        {
            if ((mode & 1) != 0) { cts.Cancel(); }
            if ((mode & 2) != 0) { clock.Advance(TimeSpan.FromSeconds(1)); }
        });
        var pending = new PendingValidation();
        var disposeCalls = 0;
        var beforeCommitCalls = 0;
        var error = Capture(() => pending.Execute(fixture.Bytes, budget, selected, cts.Token,
            beforeCommit: () => beforeCommitCalls++, inflater: observer,
            onSinkDispose: () => disposeCalls++));
        Check(error is ProbeException { Failure: Failure.Boundary });
        Check(observer.Calls == 1 && observer.TotalOut == delta && observer.ActualTotalOut == 1);
        Check(budget.Processed == fixture.Bytes.Length && budget.Output == 0);
        Check(disposeCalls == 1 && beforeCommitCalls == 0);
        Uncommitted(pending);
        var overlapping = Capture(() => budget.Check(cts.Token));
        Check((mode & 1) != 0 ? overlapping is OperationCanceledException
            : (mode & 2) != 0 ? overlapping is ProbeException { Failure: Failure.Budget }
            : overlapping == null);
    }

    private static Exception? Capture(Action action)
    {
        try { action(); }
        catch (Exception error) { return error; }
        return null;
    }

    private static void Uncommitted(PendingValidation pending) =>
        Check(pending.CommitCount == 0 && pending.CommittedBoundary == null && pending.CommittedHash == null);

    private static void Check(bool condition)
    {
        if (!condition) { throw new Exception("확정 실패 경계 회귀 실패"); }
    }
}

// 실제 해제 결과는 유지하고 반환 후 관측 카운터만 변조함.
internal sealed class CounterDeltaInflater(long delta, Action afterReturn) : IRawInflater
{
    private readonly SharpInflater inner = new();
    public int Calls { get; private set; }
    public long ActualTotalOut => inner.TotalOut;
    public bool IsFinished => inner.IsFinished;
    public bool IsNeedingInput => inner.IsNeedingInput;
    public bool IsNeedingDictionary => inner.IsNeedingDictionary;
    public long TotalIn => inner.TotalIn;
    public long TotalOut => Calls == 0 ? inner.TotalOut : delta;
    public int RemainingInput => inner.RemainingInput;
    public void SetInput(byte[] bytes, int count) => inner.SetInput(bytes, count);

    public int Inflate(byte[] output)
    {
        var written = inner.Inflate(output);
        Calls++;
        afterReturn();
        return written;
    }
}

using Zipelo.GzipBoundaryProbe;

internal static class BudgetRegression
{
    public static void Register(Action<string, Action> run)
    {
        var malformed = Fixture.Wrap([0x00, 0x01, 0x00, 0xfe, 0xff, 0x41, 0x07], [0x41], 0);
        var valid = Fixture.Wrap([0x01, 0x01, 0x00, 0xfe, 0xff, 0x41], [0x41], 0);
        foreach (var selected in new[] { false, true })
        {
            run($"accounting.decoder-exception.{selected}", () =>
            {
                var budget = new ProbeBudget(new(Processed: malformed.Bytes.Length + 1));
                var observer = new ObservedInflater();
                var pending = new PendingValidation();
                ExpectFailure(Failure.Deflate, () => pending.Execute(malformed.Bytes, budget, selected, inflater: observer));
                Check(observer.TotalOut == 1 && budget.Processed == malformed.Bytes.Length + 1);
                Check(budget.Output == 0);
                Uncommitted(pending);
                RejectNext(budget, selected);
            });
            run($"accounting.exception-over-limit.{selected}", () =>
            {
                var budget = new ProbeBudget(new(Processed: malformed.Bytes.Length));
                var pending = new PendingValidation();
                ExpectFailure(Failure.Budget, () => pending.Execute(malformed.Bytes, budget, selected));
                Check(budget.Processed == budget.ProcessedLimit + 1);
                Uncommitted(pending);
                RejectNext(budget, selected);
            });
            run($"accounting.return-cancel.{selected}", () =>
            {
                using var cts = new CancellationTokenSource();
                var budget = new ProbeBudget(new(Processed: valid.Bytes.Length + 1));
                var observer = new ObservedInflater(cts.Cancel);
                var pending = new PendingValidation();
                Expect<OperationCanceledException>(() => pending.Execute(valid.Bytes, budget, selected,
                    cts.Token, inflater: observer));
                Check(observer.TotalOut == 1 && budget.Processed == valid.Bytes.Length + 1);
                Check(budget.Output == 0);
                Uncommitted(pending);
                RejectNext(budget, selected);
            });
            run($"accounting.return-deadline.{selected}", () =>
            {
                var clock = new ManualClock();
                var budget = new ProbeBudget(new(Duration: TimeSpan.FromSeconds(1)), clock);
                var observer = new ObservedInflater(() => clock.Advance(TimeSpan.FromSeconds(1)));
                var pending = new PendingValidation();
                ExpectFailure(Failure.Budget, () => pending.Execute(valid.Bytes, budget, selected, inflater: observer));
                Check(observer.TotalOut == 1 && budget.Processed == valid.Bytes.Length + 1);
                Check(budget.Output == 0);
                Uncommitted(pending);
                RejectNext(budget, selected);
            });
            run($"accounting.other-exception.{selected}", () =>
            {
                var budget = new ProbeBudget(new(Processed: valid.Bytes.Length + 1));
                var observer = new ObservedInflater(() => throw new IOException("시험 어댑터 실패"));
                var pending = new PendingValidation();
                Expect<IOException>(() => pending.Execute(valid.Bytes, budget, selected, inflater: observer));
                Check(observer.TotalOut == 1 && budget.Processed == valid.Bytes.Length + 1);
                Check(budget.Output == 0);
                Uncommitted(pending);
                RejectNext(budget, selected);
            });
        }
    }

    private static void RejectNext(ProbeBudget budget, bool selected)
    {
        var processed = budget.Processed;
        var metadata = budget.Metadata;
        var next = new PendingValidation();
        var observer = new ObservedInflater();
        ExpectFailure(Failure.Budget, () => next.Execute(Fixture.Wrap([3, 0], [], 0).Bytes,
            budget, selected, inflater: observer));
        Check(budget.Processed == processed && budget.Output == 0);
        Check(budget.Metadata >= metadata && observer.Calls == 0);
        Uncommitted(next);
    }

    private static void Uncommitted(PendingValidation pending) =>
        Check(pending.CommitCount == 0 && pending.CommittedHash == null && pending.CommittedBoundary == null);

    private static void ExpectFailure(Failure failure, Action action)
    {
        try
        {
            action();
        }
        catch (ProbeException error) when (error.Failure == failure)
        {
            return;
        }
        throw new Exception($"예상 실패 없음: {failure}");
    }

    private static void Expect<T>(Action action) where T : Exception
    {
        try
        {
            action();
        }
        catch (T)
        {
            return;
        }
        throw new Exception($"예상 예외 없음: {typeof(T).Name}");
    }

    private static void Check(bool condition)
    {
        if (!condition)
        {
            throw new Exception("실패 경로의 누적 예산 검사 실패");
        }
    }
}

// 실제 inflater 반환 직후의 취소와 기한 경계를 시험함.
internal sealed class ObservedInflater(Action? afterReturn = null) : IRawInflater
{
    private readonly SharpInflater inner = new();
    public int Calls { get; private set; }
    public bool IsFinished => inner.IsFinished;
    public bool IsNeedingInput => inner.IsNeedingInput;
    public bool IsNeedingDictionary => inner.IsNeedingDictionary;
    public long TotalIn => inner.TotalIn;
    public long TotalOut => inner.TotalOut;
    public int RemainingInput => inner.RemainingInput;
    public void SetInput(byte[] bytes, int count) => inner.SetInput(bytes, count);

    public int Inflate(byte[] output)
    {
        Calls++;
        var written = inner.Inflate(output);
        afterReturn?.Invoke();
        return written;
    }
}

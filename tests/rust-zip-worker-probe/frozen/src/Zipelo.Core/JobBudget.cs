namespace Zipelo.Core;

// 제한값은 호스트가 정함. 아래 값은 제품 기본값이나 성능 약속이 아님.
public sealed record JobLimits(long MaxEntries, long MaxProcessedBytes, long MaxOutputBytes, TimeSpan MaxDuration, int MaxPasswordAttempts);
public sealed record BudgetUsage(long Entries, long ProcessedBytes, long OutputBytes);

public sealed class JobBudget
{
    private readonly object gate = new();
    private readonly JobLimits limits;
    private readonly TimeProvider time;
    private readonly long started;
    private long entries;
    private long processedBytes;
    private long outputBytes;
    private int exhausted;

    public JobBudget(JobLimits limits, TimeProvider? timeProvider = null)
    {
        ArgumentNullException.ThrowIfNull(limits);
        if (limits.MaxEntries <= 0 || limits.MaxProcessedBytes <= 0 || limits.MaxOutputBytes <= 0 ||
            limits.MaxDuration <= TimeSpan.Zero || limits.MaxPasswordAttempts <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(limits));
        }

        this.limits = limits;
        time = timeProvider ?? TimeProvider.System;
        started = time.GetTimestamp();
    }

    public BudgetUsage Usage
    {
        get { lock (gate) { return new(entries, processedBytes, outputBytes); } }
    }

    public void Check(CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        if (Volatile.Read(ref exhausted) != 0 || time.GetElapsedTime(started) >= limits.MaxDuration)
        {
            Interlocked.Exchange(ref exhausted, 1);
            throw new ArchiveException(ArchiveError.BudgetExceeded);
        }
    }

    public void AddEntry(CancellationToken cancellationToken) => Charge(1, 0, 0, cancellationToken);

    // 중간 TAR, 솔리드 선행 처리, 복구 재시도도 같은 인스턴스에 누적함.
    public void ChargeProcessed(long bytes, CancellationToken cancellationToken) => Charge(0, bytes, 0, cancellationToken);
    public void ReserveOutput(long bytes, CancellationToken cancellationToken) => Charge(0, 0, bytes, cancellationToken);

    private void Charge(long entryCount, long processed, long output, CancellationToken cancellationToken)
    {
        if (entryCount < 0 || processed < 0 || output < 0)
        {
            throw new ArgumentOutOfRangeException(nameof(processed));
        }

        lock (gate)
        {
            Check(cancellationToken);
            if (entryCount > limits.MaxEntries - entries || processed > limits.MaxProcessedBytes - processedBytes ||
                output > limits.MaxOutputBytes - outputBytes)
            {
                Interlocked.Exchange(ref exhausted, 1);
                throw new ArchiveException(ArchiveError.BudgetExceeded);
            }

            entries += entryCount;
            processedBytes += processed;
            outputBytes += output;
        }
    }
}

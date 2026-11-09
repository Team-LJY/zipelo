namespace Zipelo.Runtime.Streaming;

public enum WorkerStreamFrameKind : ushort
{
    BeginJob = 1,
    BeginItem = 2,
    Data = 3,
    EndItem = 4,
    EndJob = 5
}

public enum WorkerStreamTerminalStatus : byte
{
    Completed = 1,
    Failed = 2,
    Cancelled = 3
}

public enum WorkerStreamPhase
{
    Ready,
    BetweenItems,
    InItem,
    Terminal,
    InputCompleted,
    Faulted
}

public enum WorkerStreamDirection
{
    Read,
    Write
}

public enum WorkerStreamError
{
    InvalidHeader,
    InvalidLength,
    UnexpectedFrame,
    IdMismatch,
    SequenceMismatch,
    BudgetExceeded,
    Truncated,
    TrailingData,
    Faulted
}

public sealed class WorkerStreamException(WorkerStreamError error) : IOException(error.ToString())
{
    public WorkerStreamError Error { get; } = error;
}

public readonly record struct WorkerStreamHeader(
    WorkerStreamFrameKind Kind,
    Guid JobId,
    Guid ItemId,
    ulong Sequence,
    int PayloadLength);

public sealed record WorkerStreamFrame(WorkerStreamHeader Header, ReadOnlyMemory<byte> Payload);

public sealed record WorkerStreamLimits
{
    public long MaxDataBytes { get; init; } = 16 * 1024 * 1024;
    public long MaxWireBytes { get; init; } = 21 * 1024 * 1024;
    public int MaxItems { get; init; } = 1024;
    public int MaxFrames { get; init; } = 65536;
    public TimeSpan MaxDuration { get; init; } = TimeSpan.FromMinutes(5);

    internal void Validate()
    {
        if (MaxDataBytes is < 0 or > 16 * 1024 * 1024 ||
            MaxWireBytes is < 129 or > 21 * 1024 * 1024 ||
            MaxItems is < 0 or > 1024 ||
            MaxFrames is < 2 or > 65536 ||
            MaxDuration <= TimeSpan.Zero || MaxDuration > TimeSpan.FromMinutes(5))
        {
            throw new ArgumentOutOfRangeException(nameof(WorkerStreamLimits));
        }
    }
}

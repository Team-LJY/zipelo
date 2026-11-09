using System.Buffers.Binary;
using System.Diagnostics;

namespace Zipelo.Runtime.Streaming;

// 단일 방향의 한 작업만 검증함. 동시 호출은 채널에서 직렬화해야 함.
public sealed class WorkerStreamValidator
{
    private readonly Guid jobId;
    private readonly WorkerStreamLimits limits;
    private readonly long startedAt = Stopwatch.GetTimestamp();
    private readonly HashSet<Guid> itemIds = [];
    private Guid activeItem;
    private long declaredLength;
    private long itemBytes;

    public WorkerStreamValidator(Guid jobId, WorkerStreamLimits? limits = null)
    {
        if (jobId == Guid.Empty) { throw new ArgumentException("작업 ID가 필요함.", nameof(jobId)); }
        this.jobId = jobId;
        this.limits = limits ?? new WorkerStreamLimits();
        this.limits.Validate();
    }

    public WorkerStreamPhase Phase { get; private set; }
    public WorkerStreamTerminalStatus? TerminalStatus { get; private set; }
    public long TotalDataBytes { get; private set; }
    public long TotalWireBytes { get; private set; }
    public int FramesAccepted { get; private set; }
    public int ItemsAccepted => itemIds.Count;

    public void ValidateHeader(WorkerStreamHeader header, CancellationToken token = default)
    {
        try
        {
            Check(token);
            if (Phase is WorkerStreamPhase.Terminal or WorkerStreamPhase.InputCompleted)
            {
                Reject(WorkerStreamError.TrailingData);
            }
            if (header.JobId != jobId) { Reject(WorkerStreamError.IdMismatch); }
            if (header.Sequence != (ulong)FramesAccepted) { Reject(WorkerStreamError.SequenceMismatch); }
            if (FramesAccepted >= limits.MaxFrames) { Reject(WorkerStreamError.BudgetExceeded); }

            var validLength = header.Kind switch
            {
                WorkerStreamFrameKind.BeginJob => header.PayloadLength == 0,
                WorkerStreamFrameKind.BeginItem or WorkerStreamFrameKind.EndItem => header.PayloadLength == 8,
                WorkerStreamFrameKind.Data => header.PayloadLength is > 0 and <= WorkerStreamChannel.MaxChunkBytes,
                WorkerStreamFrameKind.EndJob => header.PayloadLength == 1,
                _ => false
            };
            if (!validLength) { Reject(WorkerStreamError.InvalidLength); }
            if (header.PayloadLength + WorkerStreamChannel.HeaderBytes > limits.MaxWireBytes - TotalWireBytes)
            {
                Reject(WorkerStreamError.BudgetExceeded);
            }

            switch (header.Kind)
            {
                case WorkerStreamFrameKind.BeginJob:
                    RequirePhase(WorkerStreamPhase.Ready);
                    RequireEmptyItem(header);
                    break;
                case WorkerStreamFrameKind.BeginItem:
                    RequirePhase(WorkerStreamPhase.BetweenItems);
                    if (header.ItemId == Guid.Empty || itemIds.Contains(header.ItemId))
                    {
                        Reject(WorkerStreamError.IdMismatch);
                    }
                    if (itemIds.Count >= limits.MaxItems) { Reject(WorkerStreamError.BudgetExceeded); }
                    break;
                case WorkerStreamFrameKind.Data:
                case WorkerStreamFrameKind.EndItem:
                    RequirePhase(WorkerStreamPhase.InItem);
                    if (header.ItemId != activeItem) { Reject(WorkerStreamError.IdMismatch); }
                    if (header.Kind == WorkerStreamFrameKind.Data &&
                        (header.PayloadLength > declaredLength - itemBytes ||
                         header.PayloadLength > limits.MaxDataBytes - TotalDataBytes))
                    {
                        Reject(WorkerStreamError.BudgetExceeded);
                    }
                    break;
                case WorkerStreamFrameKind.EndJob:
                    if (Phase is not (WorkerStreamPhase.BetweenItems or WorkerStreamPhase.InItem))
                    {
                        Reject(WorkerStreamError.UnexpectedFrame);
                    }
                    RequireEmptyItem(header);
                    break;
            }
        }
        catch
        {
            Abort();
            throw;
        }
    }

    public void Accept(WorkerStreamHeader header, ReadOnlySpan<byte> payload, CancellationToken token = default)
    {
        try
        {
            ValidateHeader(header, token);
            if (payload.Length != header.PayloadLength) { Reject(WorkerStreamError.InvalidLength); }
            switch (header.Kind)
            {
                case WorkerStreamFrameKind.BeginJob:
                    Phase = WorkerStreamPhase.BetweenItems;
                    break;
                case WorkerStreamFrameKind.BeginItem:
                    var length = BinaryPrimitives.ReadUInt64LittleEndian(payload);
                    if (length > (ulong)(limits.MaxDataBytes - TotalDataBytes))
                    {
                        Reject(WorkerStreamError.BudgetExceeded);
                    }
                    declaredLength = (long)length;
                    itemBytes = 0;
                    activeItem = header.ItemId;
                    itemIds.Add(activeItem);
                    Phase = WorkerStreamPhase.InItem;
                    break;
                case WorkerStreamFrameKind.Data:
                    itemBytes += payload.Length;
                    TotalDataBytes += payload.Length;
                    break;
                case WorkerStreamFrameKind.EndItem:
                    if (BinaryPrimitives.ReadUInt64LittleEndian(payload) != (ulong)itemBytes ||
                        itemBytes != declaredLength)
                    {
                        Reject(WorkerStreamError.InvalidLength);
                    }
                    activeItem = Guid.Empty;
                    Phase = WorkerStreamPhase.BetweenItems;
                    break;
                case WorkerStreamFrameKind.EndJob:
                    var status = (WorkerStreamTerminalStatus)payload[0];
                    if (status is not (WorkerStreamTerminalStatus.Completed or
                        WorkerStreamTerminalStatus.Failed or WorkerStreamTerminalStatus.Cancelled))
                    {
                        Reject(WorkerStreamError.UnexpectedFrame);
                    }
                    if (status == WorkerStreamTerminalStatus.Completed && Phase != WorkerStreamPhase.BetweenItems)
                    {
                        Reject(WorkerStreamError.UnexpectedFrame);
                    }
                    TerminalStatus = status;
                    Phase = WorkerStreamPhase.Terminal;
                    break;
            }
            TotalWireBytes += WorkerStreamChannel.HeaderBytes + payload.Length;
            FramesAccepted++;
        }
        catch
        {
            Abort();
            throw;
        }
    }

    public void CompleteInput(CancellationToken token = default)
    {
        try
        {
            Check(token);
            if (Phase != WorkerStreamPhase.Terminal) { Reject(WorkerStreamError.Truncated); }
            Phase = WorkerStreamPhase.InputCompleted;
        }
        catch
        {
            Abort();
            throw;
        }
    }

    public void Abort()
    {
        Phase = WorkerStreamPhase.Faulted;
        TerminalStatus = null;
    }

    private void Check(CancellationToken token)
    {
        if (Phase == WorkerStreamPhase.Faulted) { Reject(WorkerStreamError.Faulted); }
        token.ThrowIfCancellationRequested();
        if (Stopwatch.GetElapsedTime(startedAt) >= limits.MaxDuration)
        {
            Reject(WorkerStreamError.BudgetExceeded);
        }
    }

    private void RequirePhase(WorkerStreamPhase phase)
    {
        if (Phase != phase) { Reject(WorkerStreamError.UnexpectedFrame); }
    }

    private static void RequireEmptyItem(WorkerStreamHeader header)
    {
        if (header.ItemId != Guid.Empty) { Reject(WorkerStreamError.IdMismatch); }
    }

    private static void Reject(WorkerStreamError error) => throw new WorkerStreamException(error);
}

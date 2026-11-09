using System.Buffers.Binary;

namespace Zipelo.Runtime.Streaming;

// v1 실행 경로와 연결되지 않은 명시적 v2 전송 계층임.
public sealed class WorkerStreamChannel : IDisposable
{
    public const int HeaderBytes = 64;
    public const int MaxChunkBytes = 64 * 1024;
    public const ushort Version = 2;
    private const uint Magic = 0x3250495A;

    private readonly Stream stream;
    private readonly WorkerStreamDirection direction;
    private readonly WorkerStreamValidator validator;
    private readonly CancellationTokenSource deadline;
    private int busy;
    private bool disposed;

    public WorkerStreamChannel(
        Stream stream,
        WorkerStreamDirection direction,
        Guid jobId,
        WorkerStreamLimits? limits = null)
    {
        ArgumentNullException.ThrowIfNull(stream);
        if (direction is not (WorkerStreamDirection.Read or WorkerStreamDirection.Write))
        {
            throw new ArgumentOutOfRangeException(nameof(direction));
        }
        if (direction == WorkerStreamDirection.Read ? !stream.CanRead : !stream.CanWrite)
        {
            throw new ArgumentException("스트림 방향이 맞지 않음.", nameof(stream));
        }
        var actualLimits = limits ?? new WorkerStreamLimits();
        validator = new WorkerStreamValidator(jobId, actualLimits);
        deadline = new CancellationTokenSource(actualLimits.MaxDuration);
        this.stream = stream;
        this.direction = direction;
    }

    public WorkerStreamPhase Phase => validator.Phase;
    public WorkerStreamTerminalStatus? TerminalStatus => validator.TerminalStatus;
    public long TotalDataBytes => validator.TotalDataBytes;
    public long TotalWireBytes => validator.TotalWireBytes;
    public int FramesAccepted => validator.FramesAccepted;

    public async ValueTask<WorkerStreamFrame?> ReadAsync(CancellationToken token = default)
    {
        Enter(WorkerStreamDirection.Read);
        try
        {
            using var linked = CancellationTokenSource.CreateLinkedTokenSource(token, deadline.Token);
            var operationToken = linked.Token;
            operationToken.ThrowIfCancellationRequested();
            if (validator.Phase == WorkerStreamPhase.InputCompleted) { return null; }
            if (validator.Phase == WorkerStreamPhase.Faulted)
            {
                throw new WorkerStreamException(WorkerStreamError.Faulted);
            }

            var bytes = new byte[HeaderBytes];
            var first = await stream.ReadAsync(bytes.AsMemory(0, 1), operationToken).ConfigureAwait(false);
            if (first == 0)
            {
                validator.CompleteInput(operationToken);
                return null;
            }
            if (validator.Phase == WorkerStreamPhase.Terminal)
            {
                throw new WorkerStreamException(WorkerStreamError.TrailingData);
            }
            await stream.ReadExactlyAsync(bytes.AsMemory(1), operationToken).ConfigureAwait(false);
            var header = Decode(bytes);
            validator.ValidateHeader(header, operationToken);

            // 검증된 청크 길이만 할당하며 항목 선언 길이로 할당하지 않음.
            var payload = new byte[header.PayloadLength];
            await stream.ReadExactlyAsync(payload, operationToken).ConfigureAwait(false);
            validator.Accept(header, payload, operationToken);
            return new WorkerStreamFrame(header, payload);
        }
        catch (EndOfStreamException)
        {
            validator.Abort();
            throw new WorkerStreamException(WorkerStreamError.Truncated);
        }
        catch (OperationCanceledException) when (deadline.IsCancellationRequested && !token.IsCancellationRequested)
        {
            validator.Abort();
            throw new WorkerStreamException(WorkerStreamError.BudgetExceeded);
        }
        catch
        {
            validator.Abort();
            throw;
        }
        finally
        {
            Volatile.Write(ref busy, 0);
        }
    }

    public async ValueTask WriteAsync(
        WorkerStreamHeader header,
        ReadOnlyMemory<byte> payload,
        CancellationToken token = default)
    {
        Enter(WorkerStreamDirection.Write);
        try
        {
            using var linked = CancellationTokenSource.CreateLinkedTokenSource(token, deadline.Token);
            var operationToken = linked.Token;
            validator.ValidateHeader(header, operationToken);
            if (payload.Length != header.PayloadLength)
            {
                throw new WorkerStreamException(WorkerStreamError.InvalidLength);
            }

            // 비동기 쓰기 중 호출자가 원본 메모리를 바꿀 수 없도록 작은 청크만 복사함.
            var snapshot = payload.ToArray();
            validator.Accept(header, snapshot, operationToken);
            var bytes = Encode(header);
            await stream.WriteAsync(bytes, operationToken).ConfigureAwait(false);
            await stream.WriteAsync(snapshot, operationToken).ConfigureAwait(false);
            await stream.FlushAsync(operationToken).ConfigureAwait(false);
            operationToken.ThrowIfCancellationRequested();
        }
        catch (OperationCanceledException) when (deadline.IsCancellationRequested && !token.IsCancellationRequested)
        {
            validator.Abort();
            throw new WorkerStreamException(WorkerStreamError.BudgetExceeded);
        }
        catch
        {
            validator.Abort();
            throw;
        }
        finally
        {
            Volatile.Write(ref busy, 0);
        }
    }

    public void Dispose()
    {
        if (Interlocked.CompareExchange(ref busy, 1, 0) != 0)
        {
            throw new InvalidOperationException("진행 중인 호출을 취소하고 기다린 뒤 해제해야 함.");
        }
        try
        {
            if (disposed) { return; }
            disposed = true;
            deadline.Dispose();
            // 스트림 수명과 프로세스 종료는 호출자가 관리함.
        }
        finally
        {
            Volatile.Write(ref busy, 0);
        }
    }

    private void Enter(WorkerStreamDirection expected)
    {
        if (Interlocked.CompareExchange(ref busy, 1, 0) != 0)
        {
            throw new InvalidOperationException("동시 채널 호출은 지원하지 않음.");
        }
        if (disposed || direction != expected)
        {
            Volatile.Write(ref busy, 0);
            if (disposed) { throw new ObjectDisposedException(nameof(WorkerStreamChannel)); }
            throw new InvalidOperationException("채널 방향이 맞지 않음.");
        }
    }

    private static WorkerStreamHeader Decode(ReadOnlySpan<byte> bytes)
    {
        if (BinaryPrimitives.ReadUInt32LittleEndian(bytes) != Magic ||
            BinaryPrimitives.ReadUInt16LittleEndian(bytes[4..]) != Version ||
            BinaryPrimitives.ReadUInt32LittleEndian(bytes[12..]) != 0 ||
            BinaryPrimitives.ReadUInt64LittleEndian(bytes[56..]) != 0)
        {
            throw new WorkerStreamException(WorkerStreamError.InvalidHeader);
        }
        var length = BinaryPrimitives.ReadUInt32LittleEndian(bytes[8..]);
        if (length > MaxChunkBytes) { throw new WorkerStreamException(WorkerStreamError.InvalidLength); }
        return new WorkerStreamHeader(
            (WorkerStreamFrameKind)BinaryPrimitives.ReadUInt16LittleEndian(bytes[6..]),
            new Guid(bytes.Slice(16, 16)),
            new Guid(bytes.Slice(32, 16)),
            BinaryPrimitives.ReadUInt64LittleEndian(bytes[48..]),
            (int)length);
    }

    private static byte[] Encode(WorkerStreamHeader header)
    {
        var bytes = new byte[HeaderBytes];
        BinaryPrimitives.WriteUInt32LittleEndian(bytes, Magic);
        BinaryPrimitives.WriteUInt16LittleEndian(bytes.AsSpan(4), Version);
        BinaryPrimitives.WriteUInt16LittleEndian(bytes.AsSpan(6), (ushort)header.Kind);
        BinaryPrimitives.WriteUInt32LittleEndian(bytes.AsSpan(8), (uint)header.PayloadLength);
        header.JobId.TryWriteBytes(bytes.AsSpan(16, 16));
        header.ItemId.TryWriteBytes(bytes.AsSpan(32, 16));
        BinaryPrimitives.WriteUInt64LittleEndian(bytes.AsSpan(48), header.Sequence);
        return bytes;
    }
}

using System.Buffers.Binary;
using Microsoft.Win32.SafeHandles;
using Zipelo.Core;

namespace Zipelo.Runtime.Streaming;

internal static class ExtractionWire
{
    internal const string Mode = "--extract-stream-v2";
    internal const int ChunkBytes = WorkerStreamChannel.MaxChunkBytes;
    internal const long MaxRangeBytes = 48L * 1024 * 1024;
    internal const int MaxRangeRequests = 65536;
    internal const int RangeHeaderBytes = 48;
    private const uint RequestMagic = 0x3252585A;
    private const uint ConfigMagic = 0x3245585A;

    internal static JobLimits Limit(JobLimits limits)
    {
        WorkerProtocol.ValidateLimits(limits);
        return limits with
        {
            MaxEntries = Math.Min(limits.MaxEntries, WorkerProtocol.MaxEntries),
            MaxProcessedBytes = Math.Min(limits.MaxProcessedBytes, 128L * 1024 * 1024),
            MaxOutputBytes = Math.Min(limits.MaxOutputBytes, WorkerProtocol.MaxDataBytes)
        };
    }

    internal static byte[] Config(Guid id, long length, JobLimits limits)
    {
        var bytes = new byte[64];
        BinaryPrimitives.WriteUInt32LittleEndian(bytes, ConfigMagic);
        BinaryPrimitives.WriteInt32LittleEndian(bytes.AsSpan(4), 2);
        id.TryWriteBytes(bytes.AsSpan(8, 16));
        BinaryPrimitives.WriteInt64LittleEndian(bytes.AsSpan(24), length);
        BinaryPrimitives.WriteInt64LittleEndian(bytes.AsSpan(32), limits.MaxProcessedBytes);
        BinaryPrimitives.WriteInt64LittleEndian(bytes.AsSpan(40), limits.MaxOutputBytes);
        BinaryPrimitives.WriteInt64LittleEndian(bytes.AsSpan(48), limits.MaxEntries);
        BinaryPrimitives.WriteInt64LittleEndian(bytes.AsSpan(56), limits.MaxDuration.Ticks);
        return bytes;
    }

    internal static (Guid Id, long Length, JobLimits Limits) ReadConfig(Stream input)
    {
        var bytes = new byte[64];
        input.ReadExactly(bytes);
        var id = new Guid(bytes.AsSpan(8, 16));
        var length = BinaryPrimitives.ReadInt64LittleEndian(bytes.AsSpan(24));
        if (BinaryPrimitives.ReadUInt32LittleEndian(bytes) != ConfigMagic ||
            BinaryPrimitives.ReadInt32LittleEndian(bytes.AsSpan(4)) != 2 ||
            id == Guid.Empty || length is < 22 or > WorkerProtocol.MaxDataBytes)
        {
            throw new ArchiveException(ArchiveError.UnsupportedOperation);
        }
        var limits = new JobLimits(
            BinaryPrimitives.ReadInt64LittleEndian(bytes.AsSpan(48)),
            BinaryPrimitives.ReadInt64LittleEndian(bytes.AsSpan(32)),
            BinaryPrimitives.ReadInt64LittleEndian(bytes.AsSpan(40)),
            TimeSpan.FromTicks(BinaryPrimitives.ReadInt64LittleEndian(bytes.AsSpan(56))), 1);
        if (Limit(limits) != limits) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
        return (id, length, limits);
    }

    internal static WorkerStreamLimits ChannelLimits(JobLimits limits) => new()
    {
        MaxDataBytes = limits.MaxOutputBytes,
        MaxItems = (int)limits.MaxEntries,
        MaxDuration = limits.MaxDuration
    };

    internal static byte[] Length(long length)
    {
        var bytes = new byte[8];
        BinaryPrimitives.WriteInt64LittleEndian(bytes, length);
        return bytes;
    }

    internal static byte[] RangeHeader(Guid id, ulong sequence, long offset, int count)
    {
        var bytes = new byte[RangeHeaderBytes];
        BinaryPrimitives.WriteUInt32LittleEndian(bytes, RequestMagic);
        BinaryPrimitives.WriteInt32LittleEndian(bytes.AsSpan(4), 2);
        id.TryWriteBytes(bytes.AsSpan(8, 16));
        BinaryPrimitives.WriteUInt64LittleEndian(bytes.AsSpan(24), sequence);
        BinaryPrimitives.WriteInt64LittleEndian(bytes.AsSpan(32), offset);
        BinaryPrimitives.WriteInt32LittleEndian(bytes.AsSpan(40), count);
        return bytes;
    }

    internal static async Task ServeRangesAsync(Stream requests, Stream replies, SafeFileHandle snapshot,
        long length, Guid id, JobBudget budget, CancellationToken token)
    {
        var header = new byte[RangeHeaderBytes];
        var buffer = new byte[ChunkBytes];
        ulong sequence = 0;
        long bytesServed = 0;
        StreamingExtractionDiagnostics.Record(id, "range.request.begin", sequence: sequence);
        while (await requests.ReadAsync(header.AsMemory(0, 1), token).ConfigureAwait(false) != 0)
        {
            StreamingExtractionDiagnostics.Record(id, "range.header.begin", sequence: sequence);
            await requests.ReadExactlyAsync(header.AsMemory(1), token).ConfigureAwait(false);
            StreamingExtractionDiagnostics.Record(id, "range.header.end", sequence: sequence);
            var offset = BinaryPrimitives.ReadInt64LittleEndian(header.AsSpan(32));
            var count = BinaryPrimitives.ReadInt32LittleEndian(header.AsSpan(40));
            if (sequence >= MaxRangeRequests ||
                BinaryPrimitives.ReadUInt32LittleEndian(header) != RequestMagic ||
                BinaryPrimitives.ReadInt32LittleEndian(header.AsSpan(4)) != 2 ||
                new Guid(header.AsSpan(8, 16)) != id ||
                BinaryPrimitives.ReadUInt64LittleEndian(header.AsSpan(24)) != sequence ||
                BinaryPrimitives.ReadInt32LittleEndian(header.AsSpan(44)) != 0 ||
                count is <= 0 or > ChunkBytes || offset < 0 || offset > length - count)
            {
                throw new ArchiveException(ArchiveError.EngineFailure);
            }
            if (count > MaxRangeBytes - bytesServed) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
            budget.ChargeProcessed(count, token);
            StreamingExtractionDiagnostics.Record(id, "range.snapshot.begin", count, sequence);
            var read = 0;
            while (read < count)
            {
                var got = await RandomAccess.ReadAsync(snapshot, buffer.AsMemory(read, count - read),
                    offset + read, token).ConfigureAwait(false);
                if (got == 0) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
                read += got;
            }
            StreamingExtractionDiagnostics.Record(id, "range.snapshot.end", count, sequence);
            bytesServed += count;
            sequence++;
            StreamingExtractionDiagnostics.Record(id, "range.response.begin", count, sequence - 1);
            await replies.WriteAsync(header, token).ConfigureAwait(false);
            StreamingExtractionDiagnostics.Record(id, "range.response.header", count, sequence - 1);
            await replies.WriteAsync(buffer.AsMemory(0, count), token).ConfigureAwait(false);
            await replies.FlushAsync(token).ConfigureAwait(false);
            StreamingExtractionDiagnostics.Record(id, "range.response.end", count, sequence - 1);
            StreamingExtractionDiagnostics.Record(id, "range.request.begin", sequence: sequence);
        }
        StreamingExtractionDiagnostics.Record(id, "range.request.eof", sequence: sequence);
        budget.Check(token);
        StreamingExtractionDiagnostics.Record(id, "range.service.end");
    }
}

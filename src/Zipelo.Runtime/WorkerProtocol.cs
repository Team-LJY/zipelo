using System.Buffers.Binary;
using System.Runtime.CompilerServices;
using System.Text.Json;
using Zipelo.Core;

[assembly: InternalsVisibleTo("Zipelo.Worker")]

namespace Zipelo.Runtime;

public sealed record ZipCreateInput(string EntryName, Stream Content);
public sealed record ZipCreationOptions(bool Encrypt = false, long? SplitVolumeBytes = null);

// ZIP 바이트의 메모리 전달 결과임. 디스크 저장이나 해제 완료를 뜻하지 않음.
public sealed record ZipWorkerResult(JobResult Job, byte[]? CreatedZip)
{
    // 해제된 바이트는 메모리 검증 결과임. 디스크 게시 성공을 뜻하지 않음.
    public IReadOnlyList<ZipExtractedEntry>? ExtractedEntries { get; init; }
}

internal sealed record WorkerRequest(Guid JobId, ArchiveAction Action, byte[][] Inputs, string[] Names, JobLimits Limits);

internal static class WorkerProtocol
{
    internal const int MaxDataBytes = 16 * 1024 * 1024;
    internal const int MaxFrameBytes = 32 * 1024 * 1024;
    internal const int MaxEntries = 1024;
    internal const int MaxNameLength = 1024;

    internal static void ValidateLimits(JobLimits limits)
    {
        _ = new JobBudget(limits);
        if (limits.MaxDuration > TimeSpan.FromMinutes(5))
        {
            throw new ArchiveException(ArchiveError.UnsupportedOperation);
        }
    }

    internal static ZipWorkerResult Failure(Guid id, ArchiveError error) => new(
        new(id, error == ArchiveError.Cancelled ? JobState.Cancelled : JobState.Failed,
            Array.Empty<EntryResult>(), error), null);

    internal static async Task WriteAsync<T>(Stream stream, T value, CancellationToken token)
    {
        var payload = JsonSerializer.SerializeToUtf8Bytes(value);
        if (payload.Length > MaxFrameBytes) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
        var header = new byte[4];
        BinaryPrimitives.WriteInt32LittleEndian(header, payload.Length);
        await stream.WriteAsync(header, token).ConfigureAwait(false);
        await stream.WriteAsync(payload, token).ConfigureAwait(false);
        await stream.FlushAsync(token).ConfigureAwait(false);
    }

    internal static async Task<T> ReadAsync<T>(Stream stream, CancellationToken token)
    {
        var header = new byte[4];
        await stream.ReadExactlyAsync(header, token).ConfigureAwait(false);
        var length = BinaryPrimitives.ReadInt32LittleEndian(header);
        if (length <= 0 || length > MaxFrameBytes) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
        var payload = new byte[length];
        await stream.ReadExactlyAsync(payload, token).ConfigureAwait(false);
        return JsonSerializer.Deserialize<T>(payload) ?? throw new ArchiveException(ArchiveError.EngineFailure);
    }
}

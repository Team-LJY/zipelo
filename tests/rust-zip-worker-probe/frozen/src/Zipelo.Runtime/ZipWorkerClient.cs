using System.Diagnostics;
using Zipelo.Core;

namespace Zipelo.Runtime;

public sealed class ZipWorkerClient
{
    private readonly string workerPath;
    private readonly string? dotnetHostPath;
    private static readonly IReadOnlyCollection<EngineCapability> Supported = Array.AsReadOnly(new[]
    {
        new EngineCapability("zip", ArchiveAction.List),
        new EngineCapability("zip", ArchiveAction.Create)
    });

    // 호출자가 신뢰하는 앱 배포 경로만 전달함. 압축파일에서 실행 경로를 얻지 않음.
    public ZipWorkerClient(string workerPath, string? dotnetHostPath = null)
    {
        if (!Path.IsPathFullyQualified(workerPath) ||
            (dotnetHostPath is not null && !Path.IsPathFullyQualified(dotnetHostPath)))
        {
            throw new ArgumentException("작업자와 호스트의 절대 경로가 필요함");
        }
        this.workerPath = workerPath;
        this.dotnetHostPath = dotnetHostPath;
    }

    public bool IsAvailable => File.Exists(workerPath) &&
        (!workerPath.EndsWith(".dll", StringComparison.OrdinalIgnoreCase) ||
            (dotnetHostPath is not null && File.Exists(dotnetHostPath)));
    public IReadOnlyCollection<EngineCapability> Capabilities => IsAvailable ? Supported : Array.Empty<EngineCapability>();
    public bool SupportsEncryption => false;
    public bool SupportsSplitVolumes => false;
    public bool SupportsSafeExtraction => false;
    public bool SupportsFilePublication => NewZipPublisher.IsSupportedPlatform;

    // 입력은 읽기 전용 스트림의 현재 위치부터 복사하며 수명은 호출자가 소유함.
    public Task<ZipWorkerResult> ListAsync(Guid jobId, Stream zip, JobLimits limits, CancellationToken token)
        => ExecuteAsync(jobId, ArchiveAction.List, new[] { zip }, Array.Empty<string>(), limits, token);

    public Task<ZipWorkerResult> ExtractAsync(Guid jobId, Stream zip, JobLimits limits, CancellationToken token)
        => ExecuteAsync(jobId, ArchiveAction.Extract, new[] { zip }, Array.Empty<string>(), limits, token);

    // 일반 UI 해제 능력은 OS별 저장 검증 뒤에 별도로 활성화함.
    public bool SupportsMemoryExtraction => IsAvailable;

    public Task<ZipWorkerResult> CreateAsync(Guid jobId, IReadOnlyList<ZipCreateInput> inputs,
        JobLimits limits, ZipCreationOptions? options, CancellationToken token)
    {
        ArgumentNullException.ThrowIfNull(inputs);
        if (options is { Encrypt: true } || options?.SplitVolumeBytes is not null)
        {
            return Task.FromResult(WorkerProtocol.Failure(jobId, ArchiveError.UnsupportedOperation));
        }
        return ExecuteAsync(jobId, ArchiveAction.Create, inputs.Select(input => input.Content).ToArray(),
            inputs.Select(input => input.EntryName).ToArray(), limits, token);
    }

    private async Task<ZipWorkerResult> ExecuteAsync(Guid jobId, ArchiveAction action,
        IReadOnlyList<Stream> inputs, string[] names, JobLimits limits, CancellationToken token)
    {
        Process? process = null;
        var started = false;
        Task? drain = null;
        using var deadline = CancellationTokenSource.CreateLinkedTokenSource(token);
        try
        {
            if (jobId == Guid.Empty) { throw new ArchiveException(ArchiveError.UnsupportedOperation); }
            WorkerProtocol.ValidateLimits(limits);
            deadline.CancelAfter(limits.MaxDuration);
            var operationToken = deadline.Token;
            operationToken.ThrowIfCancellationRequested();
            if (!IsAvailable) { throw new ArchiveException(ArchiveError.EngineUnavailable); }
            if (inputs.Count == 0 || inputs.Count > WorkerProtocol.MaxEntries ||
                names.Any(name => name is null || name.Length > WorkerProtocol.MaxNameLength))
            {
                throw new ArchiveException(ArchiveError.BudgetExceeded);
            }

            var copies = new byte[inputs.Count][];
            long total = 0;
            var buffer = new byte[64 * 1024];
            for (var index = 0; index < inputs.Count; index++)
            {
                var source = inputs[index];
                if (source is null || !source.CanRead || source.CanWrite)
                {
                    throw new ArchiveException(ArchiveError.UnsupportedOperation);
                }
                using var copy = new MemoryStream();
                int count;
                while ((count = await source.ReadAsync(buffer, operationToken).ConfigureAwait(false)) != 0)
                {
                    total += count;
                    if (total > WorkerProtocol.MaxDataBytes || total > limits.MaxProcessedBytes)
                    {
                        throw new ArchiveException(ArchiveError.BudgetExceeded);
                    }
                    copy.Write(buffer, 0, count);
                }
                copies[index] = copy.ToArray();
            }

            var start = new ProcessStartInfo
            {
                FileName = workerPath.EndsWith(".dll", StringComparison.OrdinalIgnoreCase) ? dotnetHostPath! : workerPath,
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardInput = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true
            };
            if (workerPath.EndsWith(".dll", StringComparison.OrdinalIgnoreCase)) { start.ArgumentList.Add(workerPath); }
            process = new Process { StartInfo = start };
            if (!process.Start()) { throw new ArchiveException(ArchiveError.EngineUnavailable); }
            // stderr는 보관하거나 UI에 노출하지 않고 배출함.
            started = true;
            drain = process.StandardError.BaseStream.CopyToAsync(Stream.Null, operationToken);
            await WorkerProtocol.WriteAsync(process.StandardInput.BaseStream,
                new WorkerRequest(jobId, action, copies, names, limits), operationToken).ConfigureAwait(false);
            process.StandardInput.Close();
            var result = await WorkerProtocol.ReadAsync<ZipWorkerResult>(process.StandardOutput.BaseStream, operationToken)
                .ConfigureAwait(false);
            await process.WaitForExitAsync(operationToken).ConfigureAwait(false);
            await drain.ConfigureAwait(false);
            if (process.ExitCode != 0 || result.Job.JobId != jobId ||
                result.Job.State is not (JobState.Completed or JobState.Partial or JobState.Cancelled or JobState.Failed) ||
                (result.Job.State == JobState.Completed && result.Job.Error != ArchiveError.None) ||
                result.Job.Entries.Count > WorkerProtocol.MaxEntries + 1 ||
                result.CreatedZip?.Length > WorkerProtocol.MaxDataBytes ||
                (action == ArchiveAction.Create && result.Job.State == JobState.Completed && result.CreatedZip is null) ||
                (result.CreatedZip is not null && (action != ArchiveAction.Create || result.Job.State != JobState.Completed)))
            {
                throw new ArchiveException(ArchiveError.EngineFailure);
            }
            if (action == ArchiveAction.Extract && result.Job.State == JobState.Completed)
            {
                _ = ZipExtraction.ValidateResult(result, operationToken, snapshot: false);
            }
            else if (result.ExtractedEntries is not null)
            {
                throw new ArchiveException(ArchiveError.EngineFailure);
            }
            return result;
        }
        catch (OperationCanceledException) when (deadline.IsCancellationRequested)
        {
            return WorkerProtocol.Failure(jobId, token.IsCancellationRequested ? ArchiveError.Cancelled : ArchiveError.BudgetExceeded);
        }
        catch (ArchiveException failure) { return WorkerProtocol.Failure(jobId, failure.Code); }
        catch (ArgumentException) { return WorkerProtocol.Failure(jobId, ArchiveError.UnsupportedOperation); }
        catch { return WorkerProtocol.Failure(jobId, ArchiveError.EngineFailure); }
        finally
        {
            if (process is not null)
            {
                try
                {
                    // 취소 응답 전에 실제 작업자 종료를 기다림. 종료 실패를 성공으로 숨기지 않음.
                    if (started && !process.HasExited)
                    {
                        process.Kill(entireProcessTree: true);
                        await process.WaitForExitAsync(CancellationToken.None).ConfigureAwait(false);
                    }
                }
                finally
                {
                    deadline.Cancel();
                    if (drain is not null)
                    {
                        try { await drain.ConfigureAwait(false); }
                        catch { }
                    }
                    process.Dispose();
                }
            }
        }
    }
}

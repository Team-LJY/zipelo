using System.Collections.Concurrent;
using Zipelo.Core;
using Zipelo.Runtime.Streaming;

namespace Zipelo.StreamingExtractionTests;

internal sealed class CaseLifetime : IDisposable
{
    private static readonly AsyncLocal<CaseLifetime?> Ambient = new();
    private readonly CaseLifetime? previous = Ambient.Value;
    private readonly CancellationTokenSource cancellation = new();
    private readonly ConcurrentQueue<(Guid Job, Task<StreamingExtractionResult> Task)> operations = new();
    private bool settled;

    internal static CaseLifetime Current => Ambient.Value
        ?? throw new InvalidOperationException("사례 수명 범위 밖의 Worker 호출");
    internal static CancellationToken Token => Current.cancellation.Token;

    internal CaseLifetime() => Ambient.Value = this;

    internal async Task<StreamingExtractionResult> ExtractAsync(StreamingZipWorkerClient client,
        Guid job, Stream source, string destination, JobLimits limits, CancellationToken token)
    {
        using var linked = CancellationTokenSource.CreateLinkedTokenSource(cancellation.Token, token);
        var pending = client.ExtractToNewFolderAsync(job, source, destination, limits, linked.Token);
        operations.Enqueue((job, pending));
        return await pending.ConfigureAwait(false);
    }

    internal async Task<bool> StopAsync(Task body, string name, bool report)
    {
        // 25초는 성공 기한 연장이 아니라 취소 후 종료 확인에만 사용함.
        var shutdown = StopCoreAsync(body, name, report);
        try { return await shutdown.WaitAsync(TimeSpan.FromSeconds(25)).ConfigureAwait(false); }
        catch (TimeoutException)
        {
            Console.Error.WriteLine($"SHUTDOWN_UNCONFIRMED {name}: 취소 후 25초 내 작업/Worker 종료 확인 실패");
            foreach (var operation in operations)
            {
                Console.Error.WriteLine($"PENDING job={operation.Job}, status={operation.Task.Status}");
            }
            return false;
        }
        catch (Exception failure)
        {
            Console.Error.WriteLine($"SHUTDOWN_UNCONFIRMED {name}: {failure.GetType().Name}: {failure.Message}");
            return false;
        }
    }

    private async Task<bool> StopCoreAsync(Task body, string name, bool report)
    {
        // 콜백이 막혀도 검사 스레드의 제한된 대기가 유지되도록 비동기 취소함.
        var cancel = cancellation.CancelAsync();
        await Task.WhenAll(Observe(body), Observe(cancel)).ConfigureAwait(false);
        var pending = operations.ToArray();
        await Task.WhenAll(pending.Select(item => Observe(item.Task))).ConfigureAwait(false);
        settled = true;
        var confirmed = cancel.IsCompletedSuccessfully;
        if (!confirmed)
        {
            Console.Error.WriteLine($"SHUTDOWN_UNCONFIRMED {name}: 취소 콜백 실패: {cancel.Exception?.GetBaseException().Message}");
        }
        foreach (var (job, task) in pending)
        {
            if (!task.IsCompletedSuccessfully)
            {
                confirmed = false;
                Console.Error.WriteLine($"SHUTDOWN_UNCONFIRMED {name}: job={job}, status={task.Status}, " +
                    $"error={task.Exception?.GetBaseException().Message}");
                continue;
            }
            var result = task.Result;
            confirmed &= result.WorkerExitConfirmed;
            if (report || !result.WorkerExitConfirmed || result.InputCleanupFailed || result.Publication.CleanupFailed)
            {
                Console.WriteLine($"SHUTDOWN {name}: job={job}, workerExit={result.WorkerExitConfirmed}, " +
                    $"code={result.WorkerExitCode}, published={result.Publication.Published}, error={result.Publication.Error}, " +
                    $"inputCleanupFailed={result.InputCleanupFailed}, inputTemporary={result.InputTemporaryName}, " +
                    $"outputCleanupFailed={result.Publication.CleanupFailed}, outputTemporary={result.Publication.TemporaryFileName}");
            }
        }
        if (report && body.IsFaulted)
        {
            Console.Error.WriteLine($"DRAIN_RESULT {name}: {body.Exception?.GetBaseException().Message}");
        }
        return confirmed;
    }

    private static async Task Observe(Task task)
    {
        try { await task.ConfigureAwait(false); }
        catch { }
    }

    public void Dispose()
    {
        Ambient.Value = previous;
        // 미완료 작업이 사용하는 토큰을 먼저 폐기하지 않음. 호출자는 후속 사례를 중단함.
        if (settled) { cancellation.Dispose(); }
    }
}

internal sealed class ObservedWorkerClient(StreamingZipWorkerClient client)
{
    internal Task<StreamingExtractionResult> ExtractToNewFolderAsync(Guid job, Stream source,
        string destination, JobLimits limits, CancellationToken token) =>
        CaseLifetime.Current.ExtractAsync(client, job, source, destination, limits, token);
}

using System.IO;
using Zipelo.Core;
using Zipelo.Runtime;

namespace Zipelo.Ui;

public sealed record CreationSource(string Name, Func<CancellationToken, Task<Stream>> OpenRead);
public sealed record CreationResult(JobResult Job, ZipPublicationResult? Publication);

public static class ZipCreationWorkflow
{
#if ZIPELO_CREATION_VALIDATION
    public static bool ValidationEnabled => true;
#else
    public static bool ValidationEnabled => false;
#endif
    public const string ScopeMessage = "개발 검증용: 파일 최대 1,024개, 합계 16 MiB, 새 ZIP만 저장합니다. 폴더, 암호, 분할은 지원하지 않습니다.";

    public static async Task<CreationResult> RunAsync(ZipWorkerClient client, Guid jobId,
        IReadOnlyList<CreationSource> sources, string destination, CancellationToken cancellationToken)
    {
        if (!ValidationEnabled || !NewZipPublisher.IsSupportedPlatform)
        {
            throw new ArchiveException(ArchiveError.UnsupportedOperation);
        }
        if (sources.Count is < 1 or > 1024 || !Path.IsPathFullyQualified(destination))
        {
            throw new ArchiveException(ArchiveError.BudgetExceeded);
        }
        using var deadline = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        deadline.CancelAfter(TimeSpan.FromMinutes(2));
        var token = deadline.Token;
        var snapshots = new List<MemoryStream>();
        var inputs = new List<ZipCreateInput>();
        var names = new HashSet<string>(OutputPathPolicy.Comparer(OutputPathPolicy.CurrentPolicy));
        long total = 0;
        try
        {
            var buffer = new byte[65536];
            foreach (var source in sources)
            {
                token.ThrowIfCancellationRequested();
                if (source.Name.Length > 1024 ||
                    OutputPathPolicy.Validate(source.Name, OutputPathPolicy.CurrentPolicy).Count != 1 ||
                    !names.Add(source.Name))
                {
                    throw new ArchiveException(ArchiveError.UnsafePath);
                }
                await using var input = await source.OpenRead(token).ConfigureAwait(false);
                using var snapshot = new MemoryStream();
                int count;
                while ((count = await input.ReadAsync(buffer.AsMemory(), token).ConfigureAwait(false)) != 0)
                {
                    total += count;
                    if (total > 16L * 1024 * 1024) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
                    await snapshot.WriteAsync(buffer.AsMemory(0, count), token).ConfigureAwait(false);
                }
                // Runtime requires read-only inputs; keep writable staging private.
                var readOnlySnapshot = new MemoryStream(snapshot.ToArray(), writable: false);
                snapshots.Add(readOnlySnapshot);
                inputs.Add(new ZipCreateInput(source.Name, readOnlySnapshot));
            }
            var limits = new JobLimits(1024, 16L * 1024 * 1024, 16L * 1024 * 1024,
                TimeSpan.FromMinutes(2), 3);
            var result = await client.CreateAsync(jobId, inputs, limits, new ZipCreationOptions(), token)
                .ConfigureAwait(false);
            if (result.Job.JobId != jobId) { throw new ArchiveException(ArchiveError.EngineFailure); }
            if (result.Job.State != JobState.Completed || result.Job.Error != ArchiveError.None)
            {
                return new CreationResult(result.Job, null);
            }
            var publication = await NewZipPublisher.PublishAsync(result, destination, token).ConfigureAwait(false);
            return new CreationResult(result.Job, publication);
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested && deadline.IsCancellationRequested)
        {
            throw new ArchiveException(ArchiveError.BudgetExceeded);
        }
        finally
        {
            foreach (var snapshot in snapshots) { snapshot.Dispose(); }
        }
    }

    public static string Describe(CreationResult result)
    {
        if (result.Publication is { Published: true }) { return "새 ZIP 파일을 저장했습니다. 기존 파일은 덮어쓰지 않았습니다."; }
        if (result.Publication is { CleanupFailed: true })
        {
            return "ZIP 저장에 실패했고 임시 파일 정리도 완료하지 못했습니다. 선택한 저장 폴더를 확인하세요.";
        }
        var error = result.Publication?.Error ?? result.Job.Error;
        return error switch
        {
            ArchiveError.Cancelled => "생성이 중지되었습니다. 새 ZIP 파일은 게시되지 않았습니다.",
            ArchiveError.OutputConflict => "같은 이름의 파일이 있습니다. 기존 파일을 유지했습니다. 새 이름을 선택하세요.",
            ArchiveError.BudgetExceeded => "파일 크기나 처리 시간 한도를 초과했습니다. 더 작은 파일로 시도하세요.",
            _ => "새 ZIP 파일을 저장하지 못했습니다. 파일과 저장 위치를 확인하고 다시 시도하세요."
        };
    }
}

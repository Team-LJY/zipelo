using System.IO;
using Zipelo.Core;
using Zipelo.Runtime;

namespace Zipelo.Ui;

public sealed record ExtractionResult(JobResult Job, ZipPublicationResult? Publication);

public static class ZipExtractionWorkflow
{
#if ZIPELO_EXTRACTION_VALIDATION
    public static bool ValidationEnabled => true;
#else
    public static bool ValidationEnabled => false;
#endif
    public const string ScopeMessage = "개발 검증용: ZIP 입력/해제 합계 각각 16 MiB, 최대 1,024항목, 2분 한도입니다. 새 폴더만 게시하며 암호/분할/다른 형식은 지원하지 않습니다.";

    public static bool Published(ExtractionResult result) =>
        result.Job.State == JobState.Completed && result.Job.Error == ArchiveError.None &&
        result.Publication is { Published: true, Error: ArchiveError.None, CleanupFailed: false };

    public static void ValidateFolderName(string name)
    {
        if (string.IsNullOrWhiteSpace(name) || name.Length > 128 ||
            OutputPathPolicy.Validate(name, OutputPathPolicy.CurrentPolicy).Count != 1)
        {
            throw new ArchiveException(ArchiveError.UnsafePath);
        }
    }

    public static async Task<ExtractionResult> RunAsync(ZipWorkerClient client, Guid jobId,
        Func<CancellationToken, Task<Stream>> openRead, string destination, CancellationToken cancellationToken)
    {
        if (!ValidationEnabled || !NewFolderPublisher.IsSupportedPlatform || !client.SupportsMemoryExtraction)
        {
            throw new ArchiveException(ArchiveError.UnsupportedOperation);
        }
        if (!Path.IsPathFullyQualified(destination)) { throw new ArchiveException(ArchiveError.UnsafePath); }
        ValidateFolderName(Path.GetFileName(destination));
        using var deadline = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        deadline.CancelAfter(TimeSpan.FromMinutes(2));
        var token = deadline.Token;
        try
        {
            token.ThrowIfCancellationRequested();
            using var staging = new MemoryStream();
            await using (var input = await openRead(token).ConfigureAwait(false))
            {
                if (!input.CanRead || input.CanWrite) { throw new ArchiveException(ArchiveError.UnsupportedOperation); }
                var buffer = new byte[65536];
                int count;
                while ((count = await input.ReadAsync(buffer.AsMemory(), token).ConfigureAwait(false)) != 0)
                {
                    if (staging.Length + count > 16L * 1024 * 1024)
                    {
                        throw new ArchiveException(ArchiveError.BudgetExceeded);
                    }
                    await staging.WriteAsync(buffer.AsMemory(0, count), token).ConfigureAwait(false);
                }
            }
            // 동일한 제한 스냅샷을 읽기 전용으로 작업자에 전달함.
            using var snapshot = new MemoryStream(staging.ToArray(), writable: false);
            var limits = new JobLimits(1024, 16L * 1024 * 1024, 16L * 1024 * 1024,
                TimeSpan.FromMinutes(2), 3);
            var result = await client.ExtractAsync(jobId, snapshot, limits, token).ConfigureAwait(false);
            if (result.Job.JobId != jobId) { throw new ArchiveException(ArchiveError.EngineFailure); }
            if (result.Job.Error == ArchiveError.Cancelled && !cancellationToken.IsCancellationRequested &&
                deadline.IsCancellationRequested)
            {
                result = result with { Job = result.Job with { State = JobState.Failed, Error = ArchiveError.BudgetExceeded } };
            }
            if (result.Job.State != JobState.Completed || result.Job.Error != ArchiveError.None)
            {
                return new ExtractionResult(result.Job, null);
            }
            var publication = await NewFolderPublisher.PublishAsync(result, destination, token).ConfigureAwait(false);
            if (!publication.Published && publication.Error == ArchiveError.Cancelled &&
                !cancellationToken.IsCancellationRequested && deadline.IsCancellationRequested)
            {
                publication = publication with { Error = ArchiveError.BudgetExceeded };
            }
            // 게시 이후에는 늦은 취소보다 실제 게시 결과를 우선함.
            return new ExtractionResult(result.Job, publication);
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested && deadline.IsCancellationRequested)
        {
            throw new ArchiveException(ArchiveError.BudgetExceeded);
        }
    }

    public static string Describe(ExtractionResult result)
    {
        if (result.Publication is { CleanupFailed: true })
        {
            return "압축 풀기 또는 임시 결과 정리가 완료되지 않았습니다. 선택한 부모 폴더의 임시 항목을 확인하세요. 원본과 기존 대상은 덮어쓰지 않았습니다.";
        }
        if (Published(result)) { return "검증한 내용을 새 결과 폴더에 게시했습니다. 기존 파일/폴더는 덮어쓰지 않았습니다."; }
        var error = result.Publication?.Error ?? result.Job.Error;
        return error switch
        {
            ArchiveError.Cancelled => "압축 풀기가 중지되었습니다. 새 결과 폴더는 게시되지 않았습니다.",
            ArchiveError.OutputConflict => "같은 이름의 대상이 있거나 ZIP 내부 이름이 충돌합니다. 기존 대상은 유지됩니다. 다른 새 폴더 이름/위치를 선택하거나 ZIP을 확인하세요.",
            ArchiveError.OutputUnavailable => "이 위치에 안전하게 게시할 수 없습니다. 권한과 지원 파일시스템을 확인하고 다른 부모 폴더를 선택하세요.",
            ArchiveError.UnsafePath => "안전하지 않은 경로나 링크 조건으로 게시하지 않았습니다. ZIP과 새 저장 위치를 확인하세요.",
            ArchiveError.UnsupportedEntry => "링크 또는 지원하지 않는 항목이 있어 게시하지 않았습니다.",
            ArchiveError.IntegrityFailure => "CRC/길이 등 무결성 검증에 실패해 게시하지 않았습니다. 정상 결과로 사용하지 마세요.",
            ArchiveError.BudgetExceeded => "입력/해제 크기 또는 처리 시간 한도를 초과했습니다. 더 작은 ZIP으로 시도하세요.",
            ArchiveError.UnsupportedOperation => "이 ZIP의 암호/압축 방식이나 현재 플랫폼은 개발 검증 해제에서 지원하지 않습니다.",
            _ => "새 결과 폴더의 안전한 게시를 확인하지 못했습니다. 기존 목록과 선택을 유지했습니다."
        };
    }
}

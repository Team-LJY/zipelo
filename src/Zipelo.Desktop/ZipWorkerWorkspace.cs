using System.IO;
using Zipelo.Core;
using Zipelo.Runtime;

namespace Zipelo.Desktop;

internal static class ZipWorkerWorkspace
{
    public static IArchiveWorkspace? TryCreate(out string unavailableReason)
    {
        // 사용자 입력이나 압축파일에서 실행 경로를 얻지 않음.
        unavailableReason = "";
        var workerDirectory = Path.Combine(AppContext.BaseDirectory, "worker");
        if (!HasRequiredFiles(workerDirectory))
        {
            unavailableReason = "작업에 필요한 프로그램 파일이 누락되었습니다. Zipelo 폴더 전체를 복원한 뒤 다시 실행하세요.";
            return null;
        }
        var workerPath = Path.Combine(workerDirectory, "Zipelo.Worker.exe");
        var client = new ZipWorkerClient(workerPath);
        if (!client.IsAvailable)
        {
            unavailableReason = WorkspaceMessages.Error(ArchiveError.EngineUnavailable);
            return null;
        }

        var capabilities = client.Capabilities.Where(capability =>
            capability.Format.Equals("zip", StringComparison.OrdinalIgnoreCase) &&
            capability.Action == ArchiveAction.List).ToArray();
        var listing = new CoreWorkspace(async (request, progress, passwords, token) =>
        {
            if (request.Action != WorkspaceAction.List || request.Sources.Count != 1 ||
                !request.Format.Equals("zip", StringComparison.OrdinalIgnoreCase) ||
                Path.GetExtension(request.Sources[0]).Equals(".jar", StringComparison.OrdinalIgnoreCase))
            {
                throw new ArchiveException(ArchiveError.UnsupportedOperation);
            }

            if (!HasRequiredFiles(workerDirectory))
            {
                throw new ArchiveException(ArchiveError.EngineUnavailable);
            }

            // 파싱은 작업자에서 수행함. Desktop은 읽기 전용 스트림만 전달함.
            await using var input = new FileStream(request.Sources[0], FileMode.Open, FileAccess.Read,
                FileShare.Read, 65536, FileOptions.Asynchronous | FileOptions.SequentialScan);
            var limits = new JobLimits(1024, 16L * 1024 * 1024, 16L * 1024 * 1024,
                TimeSpan.FromMinutes(2), 3);
            var result = await client.ListAsync(request.JobId, input, limits, token).ConfigureAwait(false);
            // 런타임은 현재 실시간 진행과 암호 요청을 제공하지 않음. 가짜 이벤트를 만들지 않음.
            return result.Job;
        }, capabilities,
            "단일 ZIP 목록을 지원합니다. 일반 사용자용 생성과 압축 풀기는 검증 전입니다.");
        return new CreationWorkspace(listing, client, () => HasRequiredFiles(workerDirectory));
    }

    private static bool HasRequiredFiles(string directory)
    {
        string[] required =
        [
            "Zipelo.Worker.exe",
            "Zipelo.Worker.dll",
            "Zipelo.Worker.runtimeconfig.json",
            "Zipelo.Worker.deps.json",
            "Zipelo.Core.dll",
            "Zipelo.Runtime.dll"
        ];
        return required.All(name => File.Exists(Path.Combine(directory, name)));
    }

}

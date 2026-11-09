using Zipelo.Core;
using Zipelo.Runtime;
using Zipelo.Ui;

namespace Zipelo.Ubuntu;

public interface IUbuntuArchiveWorkspace
{
    bool CanListZip { get; }
    bool CanCreateZip { get; }
    Task<CreationResult> CreateAsync(Guid jobId, IReadOnlyList<CreationSource> sources,
        string destination, CancellationToken cancellationToken);
    string AvailabilityMessage { get; }
    Task<JobResult> ListAsync(Guid jobId, Stream input, CancellationToken cancellationToken);
}

public sealed class UbuntuWorkspace : IUbuntuArchiveWorkspace
{
    private readonly string directory = Path.Combine(AppContext.BaseDirectory, "worker");

    // Linux 작업자는 같은 배포 폴더의 apphost만 사용함. PATH나 현재 폴더를 검색하지 않음.
    private string WorkerPath => WorkerPlatform.GetWorkerPath(AppContext.BaseDirectory);

    public bool CanListZip => OperatingSystem.IsLinux() && HasRequiredFiles() &&
        new ZipWorkerClient(WorkerPath).Capabilities.Any(capability =>
            capability.Format.Equals("zip", StringComparison.OrdinalIgnoreCase) && capability.Action == ArchiveAction.List);

    public bool CanCreateZip => CanListZip && ZipCreationWorkflow.ValidationEnabled && NewZipPublisher.IsSupportedPlatform;

    public Task<CreationResult> CreateAsync(Guid jobId, IReadOnlyList<CreationSource> sources,
        string destination, CancellationToken cancellationToken)
    {
        if (!CanCreateZip) { throw new ArchiveException(ArchiveError.UnsupportedOperation); }
        return ZipCreationWorkflow.RunAsync(new ZipWorkerClient(WorkerPath), jobId, sources, destination, cancellationToken);
    }

    public string AvailabilityMessage => CanListZip
        ? "Ubuntu UI 개발 중입니다. ZIP 목록을 연결했습니다. " +
            (ZipCreationWorkflow.ValidationEnabled ? ZipCreationWorkflow.ScopeMessage : "생성 연결은 개발 검증 빌드에서만 사용할 수 있습니다.")
        : "목록 읽기에 필요한 프로그램 파일이 준비되지 않았습니다. 생성과 압축 풀기도 준비 중입니다.";

    public async Task<JobResult> ListAsync(Guid jobId, Stream input, CancellationToken cancellationToken)
    {
        if (!CanListZip)
        {
            throw new ArchiveException(ArchiveError.EngineUnavailable);
        }
        var limits = new JobLimits(1024, 16L * 1024 * 1024, 16L * 1024 * 1024,
            TimeSpan.FromMinutes(2), 3);
        var result = await new ZipWorkerClient(WorkerPath).ListAsync(jobId, input, limits, cancellationToken)
            .ConfigureAwait(false);
        if (result.Job.JobId != jobId)
        {
            throw new ArchiveException(ArchiveError.EngineFailure);
        }
        return result.Job;
    }

    private bool HasRequiredFiles()
    {
        string[] required =
        [
            WorkerPlatform.ExecutableName, "Zipelo.Worker.dll", "Zipelo.Worker.runtimeconfig.json",
            "Zipelo.Worker.deps.json", "Zipelo.Core.dll", "Zipelo.Runtime.dll"
        ];
        return required.All(name => File.Exists(Path.Combine(directory, name)));
    }
}

using System.IO;
using System.Security;
using Zipelo.Core;
using Zipelo.Runtime;
using Zipelo.Ui;

namespace Zipelo.Desktop;

internal sealed class CreationWorkspace(CoreWorkspace listing, ZipWorkerClient client, Func<bool> filesReady)
    : IArchiveWorkspace, IWorkspaceCapabilities
{
    public bool IsAvailable => listing.IsAvailable && filesReady();
    public string AvailabilityMessage => listing.AvailabilityMessage +
        (ZipCreationWorkflow.ValidationEnabled ? " " + ZipCreationWorkflow.ScopeMessage : " 생성 연결은 개발 검증 빌드에서만 사용할 수 있습니다.");

    public bool CanExecute(WorkspaceAction action, string format) => IsAvailable &&
        (action == WorkspaceAction.Create
            ? ZipCreationWorkflow.ValidationEnabled && NewZipPublisher.IsSupportedPlatform &&
                format.Equals("zip", StringComparison.OrdinalIgnoreCase)
            : listing.CanExecute(action, format));

    public async Task<WorkspaceResult> ExecuteAsync(WorkspaceRequest request, IProgress<WorkspaceProgress> progress,
        Func<PasswordChallenge, CancellationToken, Task<SecureString?>> requestPassword, CancellationToken token)
    {
        if (!CanExecute(request.Action, request.Format)) { throw new ArchiveException(ArchiveError.UnsupportedOperation); }
        if (request.Action != WorkspaceAction.Create)
        {
            return await listing.ExecuteAsync(request, progress, requestPassword, token).ConfigureAwait(false);
        }
        if (request.Destination is null) { throw new ArchiveException(ArchiveError.OutputUnavailable); }
        var sources = request.Sources.Select(path => new CreationSource(Path.GetFileName(path), cancellation =>
        {
            cancellation.ThrowIfCancellationRequested();
            // 디렉터리 재귀 수집 없이 사용자가 고른 파일만 읽음.
            if ((File.GetAttributes(path) & FileAttributes.Directory) != 0)
            {
                throw new ArchiveException(ArchiveError.UnsupportedEntry);
            }
            return Task.FromResult<Stream>(new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read,
                65536, FileOptions.Asynchronous | FileOptions.SequentialScan));
        })).ToArray();
        var result = await ZipCreationWorkflow.RunAsync(client, request.JobId, sources, request.Destination, token)
            .ConfigureAwait(false);
        var mapped = CoreWorkspace.MapResult(result.Job);
        var error = result.Publication?.Error ?? result.Job.Error;
        var outcome = result.Publication?.Published == true ? WorkspaceOutcome.Completed
            : error == ArchiveError.Cancelled ? WorkspaceOutcome.Cancelled : WorkspaceOutcome.Failed;
        return mapped with { Outcome = outcome, Error = error, Creation = result };
    }
}

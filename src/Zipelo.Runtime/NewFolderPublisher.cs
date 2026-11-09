using System.Runtime.InteropServices;
using Zipelo.Core;

namespace Zipelo.Runtime;

public static class NewFolderPublisher
{
    // 구현 진입 가능 여부임. 일반 UI 활성화나 OS 검증 완료를 뜻하지 않음.
    public static bool IsSupportedPlatform => RuntimeInformation.ProcessArchitecture == Architecture.X64 &&
        (OperatingSystem.IsWindows() || OperatingSystem.IsLinux());

    public static async Task<ZipPublicationResult> PublishAsync(ZipWorkerResult result,
        string destinationPath, CancellationToken token)
    {
        PendingFolderPublication? pending = null;
        var published = false;
        var error = ArchiveError.None;
        var cleanupFailed = false;
        int? posixError = null;
        string? temporaryName = null;
        PublicationNativeFailure? nativeFailure = null;
        PublicationNativeFailure? cleanupNativeFailure = null;
        try
        {
            token.ThrowIfCancellationRequested();
            if (!IsSupportedPlatform) { throw new ArchiveException(ArchiveError.OutputUnavailable); }
            var entries = ZipExtraction.ValidateResult(result, token, snapshot: true);
            if (!Path.IsPathFullyQualified(destinationPath)) { throw new ArchiveException(ArchiveError.UnsafePath); }
            var fullPath = Path.GetFullPath(destinationPath);
            if (OperatingSystem.IsLinux()) { _ = OutputPathPolicy.Validate(fullPath[1..], FileNamePolicy.Linux); }
            var parent = Path.GetDirectoryName(fullPath);
            var leaf = Path.GetFileName(fullPath);
            if (parent is null || OutputPathPolicy.Validate(leaf, OutputPathPolicy.CurrentPolicy).Count != 1)
            {
                throw new ArchiveException(ArchiveError.UnsafePath);
            }
            pending = OperatingSystem.IsWindows() ? new WindowsFolderPublication(parent) : new LinuxFolderPublication(parent);
            pending.Initialize();
            foreach (var entry in entries) { await pending.WriteAsync(entry, token).ConfigureAwait(false); }
            pending.Verify(token);
            token.ThrowIfCancellationRequested();
            pending.Publish(leaf, token);
            published = true;
        }
        catch (OperationCanceledException) when (token.IsCancellationRequested) { error = ArchiveError.Cancelled; }
        catch (PublicationNativeException failure) { error = failure.Code; nativeFailure = failure.Failure; }
        catch (PosixPublicationException failure) { error = failure.Code; posixError = failure.NativeError; }
        catch (ArchiveException failure) { error = failure.Code; }
        catch (ArgumentException) { error = ArchiveError.UnsafePath; }
        catch { error = ArchiveError.OutputUnavailable; }
        finally
        {
            if (pending is not null)
            {
                if (!published)
                {
                    try { cleanupFailed = !pending.Abort(); }
                    catch { cleanupFailed = true; }
                    if (cleanupFailed)
                    {
                        temporaryName = pending.TemporaryName;
                        cleanupNativeFailure = pending.CleanupNativeFailure;
                    }
                }
                pending.Dispose();
            }
        }
        return new(published, error, cleanupFailed, temporaryName)
        {
            NativeFailure = nativeFailure,
            CleanupNativeFailure = cleanupNativeFailure,
            PosixError = posixError,
            CleanupPosixError = (pending as LinuxFolderPublication)?.CleanupError
        };
    }
}

using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
using Zipelo.Core;

namespace Zipelo.Runtime;

public sealed record ZipPublicationResult(bool Published, ArchiveError Error, bool CleanupFailed,
    string? TemporaryFileName = null)
{
    public int? PosixError { get; init; }
    public int? CleanupPosixError { get; init; }
    public PublicationNativeFailure? NativeFailure { get; init; }
    public PublicationNativeFailure? CleanupNativeFailure { get; init; }
}

// 경로와 OS 오류 메시지 없이 고정 API 이름과 숫자 코드만 전달함.
public sealed record PublicationNativeFailure(string Operation, uint Win32Error, int? NtStatus = null);

internal sealed class PublicationNativeException(ArchiveError code, PublicationNativeFailure failure)
    : Exception(code.ToString())
{
    internal ArchiveError Code { get; } = code;
    internal PublicationNativeFailure Failure { get; } = failure;
}

public static class NewZipPublisher
{
    // 구현 대상 여부임. OS별 실행 검증 완료나 대상 파일시스템 지원을 뜻하지 않음.
    public static bool IsSupportedPlatform => RuntimeInformation.ProcessArchitecture == Architecture.X64 &&
        (OperatingSystem.IsWindows() || OperatingSystem.IsLinux());

    public static async Task<ZipPublicationResult> PublishAsync(ZipWorkerResult result,
        string destinationPath, CancellationToken token)
    {
        PendingPublication? pending = null;
        var published = false;
        var error = ArchiveError.None;
        var cleanupFailed = false;
        string? temporaryFileName = null;
        PublicationNativeFailure? nativeFailure = null;
        PublicationNativeFailure? cleanupNativeFailure = null;
        try
        {
            token.ThrowIfCancellationRequested();
            if (!IsSupportedPlatform) { throw new ArchiveException(ArchiveError.OutputUnavailable); }
            var bytes = result.CreatedZip;
            if (result.Job.State != JobState.Completed || result.Job.Error != ArchiveError.None ||
                bytes is null || bytes.Length == 0)
            {
                throw new ArchiveException(ArchiveError.UnsupportedOperation);
            }
            if (bytes.Length > WorkerProtocol.MaxDataBytes) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
            if (!Path.IsPathFullyQualified(destinationPath)) { throw new ArchiveException(ArchiveError.UnsafePath); }
            var fullPath = Path.GetFullPath(destinationPath);
            if (OperatingSystem.IsLinux())
            {
                _ = OutputPathPolicy.Validate(fullPath[1..], FileNamePolicy.Linux);
            }
            var parent = Path.GetDirectoryName(fullPath);
            var leaf = Path.GetFileName(fullPath);
            if (parent is null || OutputPathPolicy.Validate(leaf, OutputPathPolicy.CurrentPolicy).Count != 1)
            {
                throw new ArchiveException(ArchiveError.UnsafePath);
            }
            // 호출자가 생성물 배열을 이후 변경해도 쓰는 내용이 달라지지 않도록 복사함.
            var snapshot = bytes.ToArray();
            pending = OperatingSystem.IsWindows() ? WindowsPublication.Create(parent) : LinuxPublication.Create(parent);
            for (var offset = 0; offset < snapshot.Length; offset += 65536)
            {
                token.ThrowIfCancellationRequested();
                var count = Math.Min(65536, snapshot.Length - offset);
                await RandomAccess.WriteAsync(pending.Handle, snapshot.AsMemory(offset, count), offset, token)
                    .ConfigureAwait(false);
            }
            RandomAccess.FlushToDisk(pending.Handle);
            token.ThrowIfCancellationRequested();
            pending.Publish(leaf);
            // 게시 성공 이후의 늦은 취소는 이미 존재하는 새 파일을 취소 결과로 바꾸지 않음.
            published = true;
        }
        catch (OperationCanceledException) when (token.IsCancellationRequested) { error = ArchiveError.Cancelled; }
        catch (PublicationNativeException failure)
        {
            error = failure.Code;
            nativeFailure = failure.Failure;
        }
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
                        temporaryFileName = pending.TemporaryFileName;
                        cleanupNativeFailure = pending.CleanupNativeFailure;
                    }
                }
                pending.Dispose();
            }
        }
        return new(published, error, cleanupFailed, temporaryFileName)
        {
            NativeFailure = nativeFailure,
            CleanupNativeFailure = cleanupNativeFailure
        };
    }
}

internal abstract class PendingPublication : IDisposable
{
    internal abstract SafeFileHandle Handle { get; }
    internal virtual string? TemporaryFileName => null;
    internal virtual PublicationNativeFailure? CleanupNativeFailure => null;
    internal abstract void Publish(string leaf);
    internal abstract bool Abort();
    public abstract void Dispose();
}

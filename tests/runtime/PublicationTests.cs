using System.IO.Compression;
using Zipelo.Core;
using Zipelo.Runtime;
using Zipelo.SecurityTests;

namespace Zipelo.RuntimeTests;

internal sealed partial class RuntimeTests
{
    private string PublicationDirectory()
    {
        var directory = Path.Combine(sandbox, Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(directory);
        return directory;
    }

    private static async Task<ZipWorkerResult> Artifact()
    {
        var result = await Create(Worker, "input.txt", new byte[] { 2, 4, 6 });
        Check.Equal(JobState.Completed, result.Job.State);
        Check.That(result.CreatedZip is { Length: > 0 }, "실제 생성물이 없음");
        return result;
    }

    private static void PublicationFailed(ZipPublicationResult result, ArchiveError error)
    {
        Check.That(!result.Published, "실패한 게시를 성공으로 표시함");
        Check.Equal(error, result.Error);
        Check.That(!result.CleanupFailed && result.TemporaryFileName is null, "작업 임시 파일 정리에 실패함");
    }

    private async Task PublishNewFile()
    {
        var directory = PublicationDirectory();
        var destination = Path.Combine(directory, "new.zip");
        var artifact = await Artifact();
        var result = await NewZipPublisher.PublishAsync(artifact, destination, default);
        Check.That(result.Published && !result.CleanupFailed, "새 파일 게시 또는 정리에 실패함");
        Check.Equal(ArchiveError.None, result.Error);
        Check.That(result.TemporaryFileName is null, "성공한 게시에 임시 파일이 보고됨");
        var actual = await File.ReadAllBytesAsync(destination);
        Check.That(actual.SequenceEqual(artifact.CreatedZip!), "게시 바이트가 생성물과 다름");
        Check.That(Directory.GetFileSystemEntries(directory).SequenceEqual(new[] { destination }), "임시 파일이 남음");
        using var input = File.OpenRead(destination);
        using var zip = new ZipArchive(input, ZipArchiveMode.Read);
        Check.Equal("input.txt", zip.Entries.Single().FullName);
    }

    private async Task PublishConflictPreservesSource(bool useSourcePath)
    {
        var directory = PublicationDirectory();
        var source = Path.Combine(directory, "input.txt");
        var occupied = Path.Combine(directory, "existing.zip");
        byte[] original = [3, 5, 7];
        byte[] existing = [11, 13];
        await File.WriteAllBytesAsync(source, original);
        await File.WriteAllBytesAsync(occupied, existing);
        ZipWorkerResult artifact;
        await using (var input = new FileStream(source, FileMode.Open, FileAccess.Read, FileShare.Read))
        {
            artifact = await Worker.CreateAsync(Guid.NewGuid(), [new("input.txt", input)], Limits, null, default);
        }
        Check.Equal(JobState.Completed, artifact.Job.State);
        var destination = useSourcePath ? source : occupied;
        var result = await NewZipPublisher.PublishAsync(artifact, destination, default);
        // 오류 분류가 틀려도 파일 보존과 임시 파일 잔존 여부를 먼저 확인함.
        var sourceAfter = await File.ReadAllBytesAsync(source);
        var occupiedAfter = await File.ReadAllBytesAsync(occupied);
        Check.That(original.SequenceEqual(sourceAfter), "원본 파일이 변경됨");
        Check.That(existing.SequenceEqual(occupiedAfter), "기존 출력 파일이 변경됨");
        Check.Equal(2, Directory.GetFileSystemEntries(directory).Length);
        PublicationFailed(result, ArchiveError.OutputConflict);
    }

    private async Task PublishPreCancelled()
    {
        var directory = PublicationDirectory();
        var sentinel = Path.Combine(directory, "existing.txt");
        await File.WriteAllTextAsync(sentinel, "보존");
        var artifact = await Artifact();
        using var cancellation = new CancellationTokenSource();
        cancellation.Cancel();
        foreach (var destination in new[] { sentinel, Path.Combine(directory, "new.zip") })
        {
            PublicationFailed(await NewZipPublisher.PublishAsync(artifact, destination, cancellation.Token),
                ArchiveError.Cancelled);
        }
        Check.Equal("보존", await File.ReadAllTextAsync(sentinel));
        Check.Equal(1, Directory.GetFileSystemEntries(directory).Length);
    }

    private async Task PublishFailurePreservesFiles()
    {
        var directory = PublicationDirectory();
        var sentinel = Path.Combine(directory, "sentinel.txt");
        await File.WriteAllTextAsync(sentinel, "보존");
        var artifact = await Artifact();
        var failed = artifact with { Job = artifact.Job with { State = JobState.Failed, Error = ArchiveError.IntegrityFailure } };
        PublicationFailed(await NewZipPublisher.PublishAsync(failed, sentinel, default), ArchiveError.UnsupportedOperation);
        PublicationFailed(await NewZipPublisher.PublishAsync(artifact, "relative.zip", default), ArchiveError.UnsafePath);
        PublicationFailed(await NewZipPublisher.PublishAsync(artifact, Path.Combine(directory, "absent", "new.zip"), default),
            ArchiveError.OutputUnavailable);
        Check.Equal("보존", await File.ReadAllTextAsync(sentinel));
        Check.Equal(1, Directory.GetFileSystemEntries(directory).Length);
    }

    private async Task PublishRejectsLinks(bool parentLink)
    {
        var directory = PublicationDirectory();
        var target = Path.Combine(directory, "target");
        Directory.CreateDirectory(target);
        var sentinel = Path.Combine(target, "sentinel.txt");
        await File.WriteAllTextAsync(sentinel, "보존");
        var link = Path.Combine(directory, parentLink ? "link" : "existing.zip");
        FileSystemInfo linkInfo;
        try
        {
            linkInfo = parentLink
                ? Directory.CreateSymbolicLink(link, target)
                : File.CreateSymbolicLink(link, sentinel);
        }
        catch (Exception exception) when (OperatingSystem.IsWindows() &&
            (exception is UnauthorizedAccessException || exception is IOException &&
                (exception.HResult & 0xFFFF) == 1314))
        {
            throw new CaseSkippedException("Windows 심볼릭 링크 생성 권한이 없어 해당 사례 미실행");
        }
        try
        {
            var originalLinkTarget = linkInfo.LinkTarget;
            Check.That(originalLinkTarget is not null, "검사용 링크가 생성되지 않음");
            var artifact = await Artifact();
            var destination = parentLink ? Path.Combine(link, "new.zip") : link;
            var result = await NewZipPublisher.PublishAsync(artifact, destination, default);
            Check.Equal("보존", await File.ReadAllTextAsync(sentinel));
            Check.Equal(1, Directory.GetFileSystemEntries(target).Length);
            Check.Equal(2, Directory.GetFileSystemEntries(directory).Length);
            linkInfo.Refresh();
            Check.That((linkInfo.Attributes & FileAttributes.ReparsePoint) != 0, "기존 링크가 교체됨");
            Check.Equal(originalLinkTarget, linkInfo.LinkTarget);
            PublicationFailed(result, parentLink ? ArchiveError.UnsafePath : ArchiveError.OutputConflict);
        }
        finally { linkInfo.Delete(); }
    }
}

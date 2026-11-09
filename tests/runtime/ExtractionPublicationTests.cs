using System.Security.Cryptography;
using System.Text.Json;
using Zipelo.Core;
using Zipelo.Runtime;
using Zipelo.SecurityTests;

namespace Zipelo.RuntimeTests;

internal sealed partial class ExtractionTests
{
    private static void RequireFolderImplementation()
    {
        if (!NewFolderPublisher.IsSupportedPlatform)
        {
            throw new CaseSkippedException("이 OS의 새 폴더 게시가 실패 폐쇄 상태이므로 실제 게시 검사 미실행");
        }
    }

    private static void RequireFolderDestination(string parent)
    {
        var environment = FolderEnvironment.Inspect(parent);
        if (environment.Rejection is not null)
        {
            throw new CaseSkippedException("실제 게시 수용 검사 미실행: " + environment.Description);
        }
    }

    private async Task FolderPlatform()
    {
        var entryAvailable = System.Runtime.InteropServices.RuntimeInformation.ProcessArchitecture ==
            System.Runtime.InteropServices.Architecture.X64 && (OperatingSystem.IsWindows() || OperatingSystem.IsLinux());
        Check.Equal(entryAvailable, NewFolderPublisher.IsSupportedPlatform);
        Check.That(!Worker.SupportsSafeExtraction, "OS 검증 전에 일반 UI의 해제를 활성화함");
        var area = await NewArea();
        var environment = FolderEnvironment.Inspect(area.Root);
        Console.WriteLine("OBSERVED folder.platform_contract: " + environment.Description);
        var extracted = await ExtractArea(area);
        var destination = Path.Combine(area.Root, "result");
        if (environment.Rejection is not { } expectedError)
        {
            var result = await NewFolderPublisher.PublishAsync(extracted, destination, default);
            Check.That(result.Published, "지원 저장 조건에서 실제 게시가 실패함");
            Check.Equal(ArchiveError.None, result.Error);
            Check.That(!result.CleanupFailed && result.TemporaryFileName is null, "성공 뒤 임시 결과가 남음");
            await Preserved(area, "source.zip", "sentinel.txt", "result");
            await VerifyPublishedTree(destination, Successful(extracted));
            return;
        }

        var existing = Path.Combine(area.Root, "existing");
        Directory.CreateDirectory(existing);
        await File.WriteAllTextAsync(Path.Combine(existing, "keep.txt"), "기존 폴더 표식");
        foreach (var target in new[] { destination, area.Source, existing })
        {
            var result = await NewFolderPublisher.PublishAsync(extracted, target, default);
            await Preserved(area, "source.zip", "sentinel.txt", "existing");
            ExactChildren(existing, "keep.txt");
            Check.Equal("기존 폴더 표식", await File.ReadAllTextAsync(Path.Combine(existing, "keep.txt")));
            PublicationFailure(result, expectedError);
        }
    }

    private async Task PublishFolder()
    {
        RequireFolderImplementation();
        var area = await NewArea();
        RequireFolderDestination(area.Root);
        var destination = Path.Combine(area.Root, "result");
        var extracted = await ExtractArea(area);
        Successful(extracted);
        var result = await NewFolderPublisher.PublishAsync(extracted, destination, default);
        await Preserved(area, "source.zip", "sentinel.txt", "result");
        Check.That(result.Published, "안전 규칙을 만족하는 새 폴더 게시가 실패함");
        Check.Equal(ArchiveError.None, result.Error);
        Check.That(!result.CleanupFailed && result.TemporaryFileName is null, "성공 뒤 임시 결과가 남음");
        ExactChildren(destination, "hello.txt", "nested");
        ExactChildren(Path.Combine(destination, "nested"), "data.bin", "empty.txt");
        using var manifest = JsonDocument.Parse(await File.ReadAllTextAsync(
            Path.Combine(root, "tests", "extraction", "fixtures.json")));
        foreach (var expected in manifest.RootElement.GetProperty("normal_files").EnumerateObject())
        {
            var path = Path.Combine(destination, expected.Name.Replace('/', Path.DirectorySeparatorChar));
            var bytes = await File.ReadAllBytesAsync(path);
            Check.Equal(expected.Value.GetProperty("bytes").GetInt32(), bytes.Length);
            Check.Equal(expected.Value.GetProperty("sha256").GetString(), Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant());
        }
    }

    private async Task FolderConflict(bool folder)
    {
        RequireFolderImplementation();
        var area = await NewArea();
        RequireFolderDestination(area.Root);
        var destination = folder ? Path.Combine(area.Root, "existing") : area.Source;
        if (folder)
        {
            Directory.CreateDirectory(destination);
            await File.WriteAllTextAsync(Path.Combine(destination, "keep.txt"), "기존 폴더 내용");
        }
        var result = await NewFolderPublisher.PublishAsync(await ExtractArea(area), destination, default);
        await Preserved(area, folder ? ["source.zip", "sentinel.txt", "existing"] : ["source.zip", "sentinel.txt"]);
        if (folder)
        {
            ExactChildren(destination, "keep.txt");
            Check.Equal("기존 폴더 내용", await File.ReadAllTextAsync(Path.Combine(destination, "keep.txt")));
        }
        PublicationFailure(result, ArchiveError.OutputConflict);
    }

    private async Task FolderPreCancelled()
    {
        var area = await NewArea();
        var extracted = await ExtractArea(area);
        using var cancellation = new CancellationTokenSource();
        cancellation.Cancel();
        foreach (var destination in new[] { Path.Combine(area.Root, "result"), area.Source })
        {
            var result = await NewFolderPublisher.PublishAsync(extracted, destination, cancellation.Token);
            await Preserved(area, "source.zip", "sentinel.txt");
            PublicationFailure(result, ArchiveError.Cancelled);
        }
    }

    private async Task FolderRejectsTamperedBytes()
    {
        RequireFolderImplementation();
        var area = await NewArea();
        var extracted = await ExtractArea(area);
        var items = Successful(extracted);
        items.Single(item => item.Entry.Name == "hello.txt").Content[0] ^= 1;
        var result = await NewFolderPublisher.PublishAsync(extracted, Path.Combine(area.Root, "result"), default);
        await Preserved(area, "source.zip", "sentinel.txt");
        PublicationFailure(result, ArchiveError.IntegrityFailure);
    }

    private async Task FolderRejectsFailedExtraction()
    {
        RequireFolderImplementation();
        var area = await NewArea();
        var extracted = await Extract("bad-payload-crc.zip");
        Failure(extracted, ArchiveError.IntegrityFailure);
        var result = await NewFolderPublisher.PublishAsync(extracted, Path.Combine(area.Root, "result"), default);
        await Preserved(area, "source.zip", "sentinel.txt");
        PublicationFailure(result, ArchiveError.EngineFailure);
    }

    private async Task FolderRejectsLink(bool parent)
    {
        RequireFolderImplementation();
        var area = await NewArea();
        if (!parent) { RequireFolderDestination(area.Root); }
        var target = Path.Combine(area.Root, "target");
        Directory.CreateDirectory(target);
        await File.WriteAllTextAsync(Path.Combine(target, "keep.txt"), "링크 대상 내용");
        var link = Path.Combine(area.Root, "link");
        FileSystemInfo info;
        try { info = Directory.CreateSymbolicLink(link, target); }
        catch (UnauthorizedAccessException) when (OperatingSystem.IsWindows())
        {
            throw new CaseSkippedException("Windows 심볼릭 링크 생성 권한이 없어 해당 사례 미실행");
        }
        catch (IOException failure) when (OperatingSystem.IsWindows() && (failure.HResult & 0xFFFF) == 1314)
        {
            throw new CaseSkippedException("Windows 심볼릭 링크 생성 권한이 없어 해당 사례 미실행");
        }
        try
        {
            var originalTarget = info.LinkTarget;
            Check.That(originalTarget is not null, "검사 링크의 대상 정보가 없음");
            var destination = parent ? Path.Combine(link, "result") : link;
            var result = await NewFolderPublisher.PublishAsync(await ExtractArea(area), destination, default);
            await Preserved(area, "source.zip", "sentinel.txt", "target", "link");
            ExactChildren(target, "keep.txt");
            Check.Equal("링크 대상 내용", await File.ReadAllTextAsync(Path.Combine(target, "keep.txt")));
            info.Refresh();
            Check.That((info.Attributes & FileAttributes.ReparsePoint) != 0, "기존 링크를 교체함");
            Check.Equal(originalTarget, info.LinkTarget);
            PublicationFailure(result, parent ? ArchiveError.UnsafePath : ArchiveError.OutputConflict);
        }
        finally { info.Delete(); }
    }

    private static void PublicationFailure(ZipPublicationResult result, ArchiveError error)
    {
        Check.That(!result.Published, "실패한 해제 결과를 게시 완료로 표시함");
        Check.Equal(error, result.Error);
        Check.That(!result.CleanupFailed && result.TemporaryFileName is null, "작업 소유 임시 결과를 정리하지 못함");
    }

    private async Task<Area> NewArea()
    {
        var directory = Path.Combine(sandbox, "extract-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(directory);
        var source = Path.Combine(directory, "source.zip");
        File.Copy(FixturePath("normal-deflated.zip"), source);
        await File.WriteAllTextAsync(Path.Combine(directory, "sentinel.txt"), "기존 표식");
        return new(directory, source, SHA256.HashData(await File.ReadAllBytesAsync(source)));
    }

    private static async Task<ZipWorkerResult> ExtractArea(Area area)
    {
        await using var input = new FileStream(area.Source, FileMode.Open, FileAccess.Read, FileShare.Read);
        var result = await Worker.ExtractAsync(Guid.NewGuid(), input, Limits, default);
        Successful(result);
        return result;
    }

    private static async Task Preserved(Area area, params string[] expected)
    {
        var hash = SHA256.HashData(await File.ReadAllBytesAsync(area.Source));
        Check.That(area.SourceHash.SequenceEqual(hash), "해제나 게시 실패로 원본이 변경됨");
        Check.Equal("기존 표식", await File.ReadAllTextAsync(Path.Combine(area.Root, "sentinel.txt")));
        ExactChildren(area.Root, expected, allowLinks: true);
    }

    private static void ExactChildren(string directory, params string[] expected)
        => ExactChildren(directory, expected, allowLinks: false);

    private static void ExactChildren(string directory, string[] expected, bool allowLinks)
    {
        var paths = Directory.GetFileSystemEntries(directory);
        Check.That(paths.Select(path => Path.GetFileName(path) ?? throw new InvalidOperationException()).Order(StringComparer.Ordinal)
            .SequenceEqual(expected.Order(StringComparer.Ordinal)), "예상하지 않은 파일이나 임시 결과가 남음");
        if (!allowLinks)
        {
            Check.That(paths.All(path => (File.GetAttributes(path) & FileAttributes.ReparsePoint) == 0),
                "새 결과에 링크가 포함됨");
        }
    }

    private sealed record Area(string Root, string Source, byte[] SourceHash);
}

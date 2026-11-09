using System.Security.Cryptography;
using Zipelo.Core;
using Zipelo.Runtime;
using Zipelo.SecurityTests;

namespace Zipelo.RuntimeTests;

internal sealed partial class ExtractionTests
{
    private async Task FolderConcurrentPublish()
    {
        RequireFolderImplementation();
        var area = await NewArea();
        RequireFolderDestination(area.Root);
        var external = Path.Combine(sandbox, "folder-external-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(external);
        var externalFile = Path.Combine(external, "keep.txt");
        var externalBytes = "출력 부모 밖의 보존 표식"u8.ToArray();
        await File.WriteAllBytesAsync(externalFile, externalBytes);
        var inputs = new[] { await ExtractArea(area), await Extract("cumulative.zip") };
        foreach (var input in inputs) { Successful(input); }
        var destination = Path.Combine(area.Root, "result");
        var start = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        using var cancellation = new CancellationTokenSource(TimeSpan.FromSeconds(30));

        async Task<ZipPublicationResult> Attempt(int index)
        {
            await start.Task;
            return await NewFolderPublisher.PublishAsync(inputs[index], destination, cancellation.Token);
        }

        // 두 요청을 같은 시작 장벽에서 풀되 네이티브 호출의 정확한 겹침을 보장하지는 않음.
        var attempts = new[] { Attempt(0), Attempt(1) };
        start.SetResult();
        var results = await Task.WhenAll(attempts);
        await Preserved(area, "source.zip", "sentinel.txt", "result");
        ExactChildren(external, "keep.txt");
        var afterExternal = await File.ReadAllBytesAsync(externalFile);
        Check.That(externalBytes.SequenceEqual(afterExternal), "출력 부모 밖의 기존 파일을 변경함");
        Check.Equal(1, results.Count(result => result.Published));
        var winner = Array.FindIndex(results, result => result.Published);
        Check.Equal(ArchiveError.None, results[winner].Error);
        Check.That(!results[winner].CleanupFailed && results[winner].TemporaryFileName is null,
            "동시 게시 성공 뒤 임시 결과나 정리 실패가 남음");
        PublicationFailure(results[1 - winner], ArchiveError.OutputConflict);
        await VerifyPublishedTree(destination, Successful(inputs[winner]));
    }

    private static async Task VerifyPublishedTree(string directory, IReadOnlyList<ZipExtractedEntry> items)
    {
        Check.That((File.GetAttributes(directory) & FileAttributes.ReparsePoint) == 0,
            "게시 루트가 링크로 바뀜");
        var expected = new Dictionary<string, bool>(StringComparer.Ordinal);
        foreach (var item in items)
        {
            var segments = item.Entry.Name.Split('/');
            for (var index = 0; index < segments.Length; index++)
            {
                expected[string.Join('/', segments.Take(index + 1))] =
                    index < segments.Length - 1 || item.Entry.Kind == EntryKind.Directory;
            }
        }
        var actual = new Dictionary<string, bool>(StringComparer.Ordinal);
        var pending = new Queue<(string Path, string Prefix)>();
        pending.Enqueue((directory, ""));
        while (pending.TryDequeue(out var parent))
        {
            foreach (var path in Directory.GetFileSystemEntries(parent.Path))
            {
                var attributes = File.GetAttributes(path);
                Check.That((attributes & FileAttributes.ReparsePoint) == 0, "게시 결과에 외부 링크가 포함됨");
                var name = Path.GetFileName(path);
                var relative = parent.Prefix.Length == 0 ? name : parent.Prefix + "/" + name;
                var folder = (attributes & FileAttributes.Directory) != 0;
                Check.That(expected.TryGetValue(relative, out var expectedFolder) && expectedFolder == folder,
                    "다른 게시 작업의 내용이나 예상하지 않은 항목이 섞임");
                actual.Add(relative, folder);
                if (folder) { pending.Enqueue((path, relative)); }
            }
        }
        Check.Equal(expected.Count, actual.Count);
        foreach (var item in items.Where(item => item.Entry.Kind == EntryKind.File))
        {
            var path = Path.Combine(directory, item.Entry.Name.Replace('/', Path.DirectorySeparatorChar));
            var bytes = await File.ReadAllBytesAsync(path);
            Check.Equal(item.Content.Length, bytes.Length);
            Check.That(SHA256.HashData(item.Content).SequenceEqual(SHA256.HashData(bytes)),
                "동시 게시 승자의 파일 해시가 다름");
        }
    }
}

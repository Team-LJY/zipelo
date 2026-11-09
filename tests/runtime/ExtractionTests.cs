using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Zipelo.Core;
using Zipelo.Runtime;
using Zipelo.SecurityTests;

namespace Zipelo.RuntimeTests;

internal sealed partial class ExtractionTests(string root, string sandbox)
{
    private static JobLimits Limits => new(32, 1024 * 1024, 1024 * 1024, TimeSpan.FromSeconds(15), 3);
    private static ZipWorkerClient Worker => TestWorkerLaunch.Worker;

    public IEnumerable<(string Name, Func<Task> Run)> Cases()
    {
        yield return ("extract.stored_contents_hashes_and_source", () => Normal("normal-stored.zip"));
        yield return ("extract.deflated_contents_hashes_and_source", () => Normal("normal-deflated.zip"));
        foreach (var name in new[] { "traversal", "absolute-posix", "absolute-unc" })
        {
            yield return ($"extract.reject_{name}", () => Reject(name + ".zip", ArchiveError.UnsafePath));
        }
        yield return ("extract.backslash_policy_by_os", () => NamesByOs("backslash-traversal.zip",
            ArchiveError.UnsafePath, new Dictionary<string, byte[]> { ["..\\escape.txt"] = Payload }));
        yield return ("extract.drive_name_policy_by_os", () => NamesByOs("absolute-drive.zip",
            ArchiveError.UnsafePath, new Dictionary<string, byte[]> { ["C:/escape.txt"] = Payload }));
        foreach (var name in new[] { "duplicate", "file-before-child", "child-before-file" })
        {
            yield return ($"extract.reject_{name}", () => Reject(name + ".zip", ArchiveError.OutputConflict));
        }
        yield return ("extract.case_policy_by_os", () => NamesByOs("case-distinct.zip", ArchiveError.OutputConflict,
            new Dictionary<string, byte[]> { ["Readme.txt"] = "first"u8.ToArray(), ["README.TXT"] = "second"u8.ToArray() }));
        yield return ("extract.legal_linux_names", () => NamesByOs("linux-names.zip", ArchiveError.UnsafePath,
            new Dictionary<string, byte[]> { ["CON"] = "one"u8.ToArray(), ["name:stream"] = "two"u8.ToArray() }));
        foreach (var name in new[] { "symlink", "unix-hardlink", "special-fifo" })
        {
            yield return ($"extract.reject_{name}", () => Reject(name + ".zip", ArchiveError.UnsupportedEntry));
        }
        foreach (var name in new[] { "bad-payload-crc", "bad-header-crc", "length-smaller", "length-larger",
            "local-central-length", "truncated" })
        {
            yield return ($"extract.reject_{name}", () => Reject(name + ".zip", ArchiveError.IntegrityFailure));
        }
        yield return ("extract.cumulative_output_budget", () => Budget("output"));
        yield return ("extract.cumulative_processed_budget", () => Budget("processed"));
        yield return ("extract.cumulative_entry_budget", () => Budget("entries"));
        yield return ("extract.cancel_before_read", PreCancelled);
        yield return ("extract.cancel_during_input_copy", CancelInput);
        yield return ("folder.platform_contract", FolderPlatform);
        yield return ("folder.new_folder_contents_hashes_and_source", PublishFolder);
        yield return ("folder.existing_folder_preserved_and_temporary_cleanup", () => FolderConflict(true));
        yield return ("folder.existing_source_file_preserved_and_temporary_cleanup", () => FolderConflict(false));
        yield return ("folder.precancel_preserves_originals", FolderPreCancelled);
        yield return ("folder.tampered_bytes_not_published", FolderRejectsTamperedBytes);
        yield return ("folder.failed_extraction_not_published", FolderRejectsFailedExtraction);
        yield return ("folder.reject_parent_symlink", () => FolderRejectsLink(true));
        yield return ("folder.reject_leaf_symlink", () => FolderRejectsLink(false));
        yield return ("folder.concurrent_same_name_one_winner", FolderConcurrentPublish);
    }

    private static byte[] Payload => "zipelo-extraction-content\n"u8.ToArray();
    private string FixturePath(string name) => Path.Combine(root, "tests", "extraction", "fixtures", name);

    private async Task<ZipWorkerResult> Extract(string name, JobLimits? limits = null)
    {
        var path = FixturePath(name);
        var before = SHA256.HashData(await File.ReadAllBytesAsync(path));
        ZipWorkerResult result;
        await using (var input = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read))
        {
            result = await Worker.ExtractAsync(Guid.NewGuid(), input, limits ?? Limits, default);
            Check.That(input.CanRead && !input.CanWrite, "해제 요청이 원본 스트림 소유권을 변경함");
        }
        var after = SHA256.HashData(await File.ReadAllBytesAsync(path));
        Check.That(before.SequenceEqual(after), "해제 요청이 원본 ZIP을 변경함");
        return result;
    }

    private static IReadOnlyList<ZipExtractedEntry> Successful(ZipWorkerResult result)
    {
        Check.Equal(JobState.Completed, result.Job.State);
        Check.Equal(ArchiveError.None, result.Job.Error);
        Check.That(result.CreatedZip is null, "해제 요청이 생성 ZIP을 반환함");
        var items = result.ExtractedEntries ?? throw new InvalidOperationException();
        Check.Equal(items.Count, result.Job.Entries.Count);
        for (var index = 0; index < items.Count; index++)
        {
            Check.Equal(items[index].Entry, result.Job.Entries[index].Entry);
            Check.Equal(EntryOutcome.Staged, result.Job.Entries[index].Outcome);
            Check.Equal(VerificationState.Passed, result.Job.Entries[index].Verification);
            Check.Equal(ArchiveError.None, result.Job.Entries[index].Error);
            Check.Equal((long?)items[index].Content.LongLength, items[index].Entry.DeclaredSize);
        }
        return items;
    }

    private static void Failure(ZipWorkerResult result, ArchiveError error)
    {
        Check.Equal(error, result.Job.Error);
        Check.Equal(error == ArchiveError.Cancelled ? JobState.Cancelled : JobState.Failed, result.Job.State);
        Check.That(result.CreatedZip is null && result.ExtractedEntries is null,
            "실패나 취소 뒤 부분 해제 결과를 반환함");
    }

    private async Task Normal(string name)
    {
        var items = Successful(await Extract(name));
        Check.Equal(4, items.Count);
        var directory = items.Single(item => item.Entry.Kind == EntryKind.Directory);
        Check.Equal("nested", directory.Entry.Name);
        Check.Equal(0, directory.Content.Length);
        using var manifest = JsonDocument.Parse(await File.ReadAllTextAsync(
            Path.Combine(root, "tests", "extraction", "fixtures.json")));
        var expected = manifest.RootElement.GetProperty("normal_files");
        foreach (var item in items.Where(item => item.Entry.Kind == EntryKind.File))
        {
            var entry = expected.GetProperty(item.Entry.Name);
            Check.Equal(entry.GetProperty("bytes").GetInt32(), item.Content.Length);
            Check.Equal(entry.GetProperty("sha256").GetString(), Convert.ToHexString(SHA256.HashData(item.Content)).ToLowerInvariant());
        }
        Check.Equal(3, items.Count(item => item.Entry.Kind == EntryKind.File));
    }

    private async Task Reject(string name, ArchiveError error) => Failure(await Extract(name), error);

    private async Task NamesByOs(string name, ArchiveError windowsError, IReadOnlyDictionary<string, byte[]> expected)
    {
        var result = await Extract(name);
        if (OperatingSystem.IsWindows()) { Failure(result, windowsError); return; }
        var items = Successful(result);
        Check.Equal(expected.Count, items.Count);
        var names = new HashSet<string>(StringComparer.Ordinal);
        foreach (var item in items)
        {
            Check.That(names.Add(item.Entry.Name), "대소문자가 다른 파일을 합침");
            Check.That(expected[item.Entry.Name].SequenceEqual(item.Content), "Linux 이름이나 내용을 변경함");
        }
    }

    private async Task Budget(string kind)
    {
        var inputBytes = new FileInfo(FixturePath("cumulative.zip")).Length;
        var exact = Limits with { MaxEntries = 2, MaxOutputBytes = 16, MaxProcessedBytes = inputBytes + 16 };
        Check.Equal(2, Successful(await Extract("cumulative.zip", exact)).Count);
        var tooSmall = kind switch
        {
            "output" => exact with { MaxOutputBytes = 15 },
            "processed" => exact with { MaxProcessedBytes = inputBytes + 15 },
            "entries" => exact with { MaxEntries = 1 },
            _ => throw new InvalidOperationException()
        };
        Failure(await Extract("cumulative.zip", tooSmall), ArchiveError.BudgetExceeded);
    }

    private static async Task PreCancelled()
    {
        using var cancellation = new CancellationTokenSource();
        cancellation.Cancel();
        using var input = new MemoryStream(Payload, false);
        Failure(await Worker.ExtractAsync(Guid.NewGuid(), input, Limits, cancellation.Token), ArchiveError.Cancelled);
        Check.Equal(0L, input.Position);
        Check.That(input.CanRead, "사전 취소로 호출자의 스트림을 닫음");
    }

    private static async Task CancelInput()
    {
        using var input = new GatedInput();
        using var cancellation = new CancellationTokenSource();
        var operation = Worker.ExtractAsync(Guid.NewGuid(), input, Limits, cancellation.Token);
        try
        {
            await input.Entered.Task.WaitAsync(TimeSpan.FromSeconds(5));
            cancellation.Cancel();
            Failure(await operation.WaitAsync(TimeSpan.FromSeconds(5)), ArchiveError.Cancelled);
            Check.That(input.CanRead, "입력 복사 취소로 호출자의 스트림을 닫음");
        }
        finally
        {
            cancellation.Cancel();
            await operation.WaitAsync(TimeSpan.FromSeconds(5));
        }
    }

    private sealed class GatedInput : MemoryStream
    {
        public TaskCompletionSource Entered { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
        public GatedInput() : base(Payload, false) { }
        public override async ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken token = default)
        {
            Entered.TrySetResult();
            await Task.Delay(Timeout.Infinite, token);
            return 0;
        }
    }
}

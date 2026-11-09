using System.Diagnostics;
using System.IO.Compression;
using System.Security.Cryptography;
using System.Text;
using Zipelo.Core;
using Zipelo.Runtime;
using Zipelo.SecurityTests;

namespace Zipelo.RuntimeTests;

internal sealed partial class RuntimeTests(string root, string sandbox)
{
    private static JobLimits Limits => new(32, 1024 * 1024, 1024 * 1024, TimeSpan.FromSeconds(15), 3);
    private static ZipWorkerClient Worker => TestWorkerLaunch.Worker;
    private static ZipWorkerClient Peer => TestWorkerLaunch.Peer;

    public IEnumerable<(string Name, Func<Task> Run)> Cases()
    {
        yield return ("runtime.absolute_worker_path_and_missing_files", Paths);
        yield return ("runtime.readonly_input_and_options", InputsAndOptions);
        yield return ("runtime.actual_bytes_and_16mib_limit", InputLimits);
        yield return ("runtime.cancel_before_read_and_during_copy", InputCancellation);
        yield return ("runtime.reject_frame_lengths_and_corruption", InvalidFrames);
        yield return ("runtime.reject_exit_id_and_missing_artifact", InvalidResults);
        yield return ("runtime.reject_nonterminal_result", () => InvalidState("nonterminal"));
        yield return ("runtime.reject_success_with_error", () => InvalidState("success-error"));
        yield return ("runtime.cancel_waits_for_worker_exit", () => StopWorker(false));
        yield return ("runtime.deadline_waits_for_worker_exit", () => StopWorker(true));
        yield return ("zip.real_list_preserves_source_and_unverified_entries", ListZip);
        yield return ("zip.real_create_roundtrip_and_cumulative_budgets", CreateZip);
        yield return ("zip.os_names_case_and_parent_collisions", ZipNames);
        yield return ("zip.corruption_and_listing_budget", ListFailures);
        yield return ("publication.new_file_exact_bytes", PublishNewFile);
        yield return ("publication.conflict_preserves_existing_file", () => PublishConflictPreservesSource(false));
        yield return ("publication.conflict_preserves_input_source", () => PublishConflictPreservesSource(true));
        yield return ("publication.precancel_preserves_existing", PublishPreCancelled);
        yield return ("publication.failure_preserves_existing", PublishFailurePreservesFiles);
        yield return ("publication.rejects_parent_symlink", () => PublishRejectsLinks(true));
        yield return ("publication.rejects_leaf_symlink", () => PublishRejectsLinks(false));
        foreach (var item in new ExtractionTests(root, sandbox).Cases()) { yield return item; }
    }

    private static void Failed(ZipWorkerResult result, ArchiveError error)
    {
        Check.Equal(error, result.Job.Error);
        Check.Equal(error == ArchiveError.Cancelled ? JobState.Cancelled : JobState.Failed, result.Job.State);
        Check.That(result.CreatedZip is null, "실패한 작업이 생성물을 반환함");
    }

    private static async Task<ZipWorkerResult> Create(ZipWorkerClient client, string name,
        byte[]? bytes = null, JobLimits? limits = null, CancellationToken token = default)
    {
        using var input = new MemoryStream(bytes ?? new byte[] { 1 }, false);
        return await client.CreateAsync(Guid.NewGuid(), [new(name, input)], limits ?? Limits, null, token);
    }

    private Task Paths()
    {
        Check.Throws<ArgumentException>(() => new ZipWorkerClient("worker/Zipelo.Worker"));
        Check.Throws<ArgumentException>(() => new ZipWorkerClient(Path.Combine(sandbox, "worker.dll"), "dotnet"));
        Check.Throws<ArgumentException>(() => WorkerPlatform.GetWorkerPath("relative"));
        var expected = OperatingSystem.IsWindows() ? "Zipelo.Worker.exe" : "Zipelo.Worker";
        Check.Equal(Path.Combine(sandbox, "worker", expected), WorkerPlatform.GetWorkerPath(sandbox));
        var missing = new ZipWorkerClient(Path.Combine(sandbox, "absent-worker.dll"), TestWorkerLaunch.DotnetHost);
        Check.That(!missing.IsAvailable && missing.Capabilities.Count == 0, "없는 작업자를 사용 가능으로 표시함");
        Check.That(!Worker.SupportsSafeExtraction && !Worker.SupportsEncryption && !Worker.SupportsSplitVolumes,
            "미구현 기능을 사용 가능으로 표시함");
        Check.That(Worker.IsAvailable && Peer.IsAvailable, "검사에 필요한 빌드 산출물이 없음");
        return Task.CompletedTask;
    }

    private static async Task InputsAndOptions()
    {
        using var writable = new MemoryStream(new byte[] { 1 });
        Failed(await Worker.ListAsync(Guid.NewGuid(), writable, Limits, default), ArchiveError.UnsupportedOperation);
        using var input = new MemoryStream(new byte[] { 1 }, false);
        foreach (var option in new[] { new ZipCreationOptions(Encrypt: true), new ZipCreationOptions(SplitVolumeBytes: 1) })
        {
            Failed(await Worker.CreateAsync(Guid.NewGuid(), [new("a", input)], Limits, option, default),
                ArchiveError.UnsupportedOperation);
            Check.Equal(0L, input.Position);
        }
        Failed(await Create(Worker, new string('x', 1025)), ArchiveError.BudgetExceeded);
        var many = Enumerable.Range(0, 1025).Select(index => new ZipCreateInput(index.ToString(), input)).ToArray();
        Failed(await Worker.CreateAsync(Guid.NewGuid(), many, Limits, null, default), ArchiveError.BudgetExceeded);
        Check.That(input.CanRead, "호출자 입력의 수명을 작업자가 종료함");
    }

    private static async Task InputLimits()
    {
        using var tiny = new MemoryStream(new byte[] { 1, 2, 3, 4 }, false);
        Failed(await Worker.ListAsync(Guid.NewGuid(), tiny, Limits with { MaxProcessedBytes = 3 }, default),
            ArchiveError.BudgetExceeded);
        // 큰 파일을 만들지 않고 일정한 버퍼로 입력 상한을 시험함.
        using var large = new GeneratedInput(16L * 1024 * 1024 + 1);
        Failed(await Worker.ListAsync(Guid.NewGuid(), large, Limits with { MaxProcessedBytes = 32L * 1024 * 1024 },
            default), ArchiveError.BudgetExceeded);
        Check.Equal(16L * 1024 * 1024 + 1, large.BytesRead);
    }

    private static async Task InputCancellation()
    {
        using var cancelled = new CancellationTokenSource();
        cancelled.Cancel();
        using var untouched = new MemoryStream(new byte[] { 1 }, false);
        Failed(await Worker.ListAsync(Guid.NewGuid(), untouched, Limits, cancelled.Token), ArchiveError.Cancelled);
        Check.Equal(0L, untouched.Position);
        using var source = new WaitingInput();
        using var pending = new CancellationTokenSource();
        var operation = Worker.ListAsync(Guid.NewGuid(), source, Limits, pending.Token);
        try
        {
            await source.Entered.Task.WaitAsync(TimeSpan.FromSeconds(5));
            pending.Cancel();
            Failed(await operation.WaitAsync(TimeSpan.FromSeconds(5)), ArchiveError.Cancelled);
        }
        finally { pending.Cancel(); }
    }

    private static async Task InvalidFrames()
    {
        foreach (var mode in new[] { "zero", "negative", "oversized" })
        {
            Failed(await Create(Peer, mode), ArchiveError.BudgetExceeded);
        }
        foreach (var mode in new[] { "truncated", "bad-json" })
        {
            Failed(await Create(Peer, mode), ArchiveError.EngineFailure);
        }
    }

    private static async Task InvalidResults()
    {
        foreach (var mode in new[] { "exit", "wrong-id", "missing-zip" })
        {
            Failed(await Create(Peer, mode), ArchiveError.EngineFailure);
        }
    }

    private static async Task InvalidState(string mode)
    {
        Failed(await Create(Peer, mode), ArchiveError.EngineFailure);
    }

    private async Task StopWorker(bool deadline)
    {
        var directory = Path.Combine(sandbox, Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(directory);
        var marker = Path.Combine(directory, "child.pid");
        using var cancellation = new CancellationTokenSource();
        var operation = Create(Peer, "hang", Encoding.UTF8.GetBytes(marker),
            Limits with { MaxDuration = TimeSpan.FromSeconds(deadline ? 5 : 15) }, cancellation.Token);
        Process? child = null;
        try
        {
            // 자식의 준비 표식을 기다리므로 취소 시점을 시작 시간 추측에 의존하지 않음.
            using var guard = new CancellationTokenSource(TimeSpan.FromSeconds(8));
            while (child is null)
            {
                if (File.Exists(marker) && int.TryParse(await File.ReadAllTextAsync(marker, guard.Token), out var id))
                {
                    child = Process.GetProcessById(id);
                }
                else
                {
                    Check.That(!operation.IsCompleted, "자식 준비 전에 작업이 종료됨");
                    await Task.Delay(10, guard.Token);
                }
            }
            if (!deadline) { cancellation.Cancel(); }
            var result = await operation.WaitAsync(TimeSpan.FromSeconds(10));
            Failed(result, deadline ? ArchiveError.BudgetExceeded : ArchiveError.Cancelled);
            Check.That(child.HasExited, "취소 응답 뒤에도 작업자가 남아 있음");
        }
        finally
        {
            cancellation.Cancel();
            try { await operation.WaitAsync(TimeSpan.FromSeconds(10)); }
            finally
            {
                if (child is not null)
                {
                    try
                    {
                        if (!child.HasExited)
                        {
                            child.Kill(entireProcessTree: true);
                            await child.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(5));
                        }
                    }
                    finally { child.Dispose(); }
                }
            }
        }
    }

    private async Task ListZip()
    {
        var source = Path.Combine(sandbox, "list-source.zip");
        File.Copy(Path.Combine(root, "tests", "security", "fixtures", "normal.zip"), source);
        var before = SHA256.HashData(await File.ReadAllBytesAsync(source));
        await using (var input = new FileStream(source, FileMode.Open, FileAccess.Read, FileShare.Read))
        {
            var result = await Worker.ListAsync(Guid.NewGuid(), input, Limits, default);
            Check.Equal(JobState.Completed, result.Job.State);
            Check.Equal(ArchiveError.None, result.Job.Error);
            Check.That(result.CreatedZip is null, "목록 요청이 생성물을 반환함");
            Check.Equal(2, result.Job.Entries.Count);
            Check.Equal("hello.txt", result.Job.Entries[0].Entry.Name);
            Check.That(result.Job.Entries.All(entry => entry.Outcome == EntryOutcome.Listed &&
                entry.Verification == VerificationState.NotChecked), "목록을 내용 검증 완료로 표시함");
            Check.That(input.CanRead && !input.CanWrite, "읽기 전용 입력의 소유권이 깨짐");
        }
        var after = SHA256.HashData(await File.ReadAllBytesAsync(source));
        Check.That(before.SequenceEqual(after), "목록 요청이 원본을 변경함");
    }

    private static async Task CreateZip()
    {
        byte[] bytes = [7, 8, 9];
        var result = await Create(Worker, "hello.txt", bytes);
        Check.Equal(JobState.Completed, result.Job.State);
        Check.Equal(ArchiveError.None, result.Job.Error);
        var zip = result.CreatedZip ?? throw new InvalidOperationException();
        using var data = new MemoryStream(zip, false);
        using var archive = new ZipArchive(data, ZipArchiveMode.Read);
        Check.Equal(1, archive.Entries.Count);
        Check.Equal("hello.txt", archive.Entries[0].FullName);
        using var decoded = new MemoryStream();
        using var entryStream = archive.Entries[0].Open();
        await entryStream.CopyToAsync(decoded);
        Check.That(bytes.SequenceEqual(decoded.ToArray()), "생성 ZIP 내용이 입력과 다름");
        Failed(await Create(Worker, "hello.txt", bytes, Limits with { MaxProcessedBytes = bytes.Length }),
            ArchiveError.BudgetExceeded);
        Failed(await Create(Worker, "hello.txt", bytes, Limits with { MaxOutputBytes = 16 }), ArchiveError.BudgetExceeded);
        Failed(await Create(Worker, "hello.txt", bytes, Limits with { MaxEntries = 1 }), ArchiveError.BudgetExceeded);
    }

    private static async Task ZipNames()
    {
        using var first = new MemoryStream(new byte[] { 1 }, false);
        using var second = new MemoryStream(new byte[] { 2 }, false);
        var result = await Worker.CreateAsync(Guid.NewGuid(),
            [new("Readme.txt", first), new("README.TXT", second)], Limits, null, default);
        if (OperatingSystem.IsWindows()) { Failed(result, ArchiveError.OutputConflict); }
        else
        {
            Check.Equal(JobState.Completed, result.Job.State);
            using var stream = new MemoryStream(result.CreatedZip ?? throw new InvalidOperationException(), false);
            using var zip = new ZipArchive(stream, ZipArchiveMode.Read);
            Check.That(zip.Entries.Select(entry => entry.FullName).SequenceEqual(new[] { "Readme.txt", "README.TXT" }),
                "Linux 이름을 합치거나 변경함");
        }
        foreach (var name in new[] { "CON", "name:stream", "name.", "e\u0301.txt", "back\\slash" })
        {
            var named = await Create(Worker, name);
            if (OperatingSystem.IsWindows() && name != "back\\slash") { Failed(named, ArchiveError.UnsafePath); }
            else
            {
                Check.Equal(JobState.Completed, named.Job.State);
                using var stream = new MemoryStream(named.CreatedZip ?? throw new InvalidOperationException(), false);
                using var zip = new ZipArchive(stream, ZipArchiveMode.Read);
                Check.Equal(OperatingSystem.IsWindows() ? name.Replace('\\', '/') : name, zip.Entries[0].FullName);
            }
        }
        first.Position = 0;
        second.Position = 0;
        Failed(await Worker.CreateAsync(Guid.NewGuid(), [new("file", first), new("file/child", second)],
            Limits, null, default), ArchiveError.OutputConflict);
    }

    private async Task ListFailures()
    {
        using var broken = File.OpenRead(Path.Combine(root, "tests", "security", "fixtures", "truncated.zip"));
        Failed(await Worker.ListAsync(Guid.NewGuid(), broken, Limits, default), ArchiveError.IntegrityFailure);
        using var normal = File.OpenRead(Path.Combine(root, "tests", "security", "fixtures", "normal.zip"));
        Failed(await Worker.ListAsync(Guid.NewGuid(), normal, Limits with { MaxEntries = 1 }, default),
            ArchiveError.BudgetExceeded);
    }

    private sealed class GeneratedInput(long length) : Stream
    {
        public long BytesRead { get; private set; }
        public override bool CanRead => true;
        public override bool CanSeek => false;
        public override bool CanWrite => false;
        public override long Length => throw new NotSupportedException();
        public override long Position { get => throw new NotSupportedException(); set => throw new NotSupportedException(); }
        public override ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken token = default)
        {
            token.ThrowIfCancellationRequested();
            var count = (int)Math.Min(buffer.Length, length - BytesRead);
            buffer.Span[..count].Clear();
            BytesRead += count;
            return ValueTask.FromResult(count);
        }
        public override int Read(byte[] buffer, int offset, int count) => throw new NotSupportedException();
        public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
        public override void Flush() => throw new NotSupportedException();
        public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
        public override void SetLength(long value) => throw new NotSupportedException();
    }

    private sealed class WaitingInput : MemoryStream
    {
        public TaskCompletionSource Entered { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
        public WaitingInput() : base(new byte[] { 1 }, false) { }
        public override async ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken token = default)
        {
            Entered.TrySetResult();
            await Task.Delay(Timeout.Infinite, token);
            return 0;
        }
    }
}

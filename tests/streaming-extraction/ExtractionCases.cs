using System.Diagnostics;
using Zipelo.Core;
using Zipelo.Runtime;
using Zipelo.Runtime.Streaming;

namespace Zipelo.StreamingExtractionTests;

internal static class ExtractionCases
{
    private static FixtureEntry Item => new("payload.bin", ZipFixture.Payload());

    public static async Task Real(string root, string mode)
    {
        FixtureEntry[] entries = mode switch
        {
            "empty" => [],
            "empty-items" => [new("empty", []), new("folder/", [])],
            "deflate" => [new("compressed.bin", ZipFixture.Payload(131073), true)],
            "nested" => [new("folder/", []), new("folder/a", [1, 2]), new("b", [3])],
            _ => [Item]
        };
        var area = new TestArea(root, ZipFixture.Build(entries));
        using var source = area.Open();
        var result = await Launch.Client().ExtractToNewFolderAsync(area.Job, source, area.Destination, TestArea.Limits, default);
        TestArea.Successful(result, entries.Length);
        area.VerifyTree(entries);
        area.Preserved(source);
        Check.Equal(entries.Sum(e => (long)e.Bytes.Length), result.Usage.OutputBytes);
    }

    public static async Task DeflateTail(string root, bool secondStream, bool large, string? fault = null)
    {
        FixtureEntry[] entries =
        [
            new("first.bin", ZipFixture.Payload(1024), true, ZipFixture.DeflateTail(secondStream, large)),
            new("after.bin", [19, 71, 203, 0, 255])
        ];
        var zip = ZipFixture.Build(entries);
        ZipFixture.VerifyTailLayout(zip, entries);
        if (fault is not null) { ZipFixture.CorruptTailDeclaration(zip, fault); }
        var area = new TestArea(root, zip);
        using var source = area.Open();
        var expectedOutput = entries.Sum(entry => (long)entry.Bytes.Length);
        var limits = TestArea.Limits with { MaxOutputBytes = expectedOutput + (fault == "length" ? 1 : 0) };
        var result = await Launch.Client().ExtractToNewFolderAsync(area.Job, source, area.Destination, limits, default);
        Console.WriteLine($"OBSERVE Deflate tail second={secondStream}, large={large}, fault={fault ?? "none"}, " +
            $"published={result.Publication.Published}, error={result.Publication.Error}, " +
            $"entries={result.VerifiedEntries}, output={result.Usage.OutputBytes}, " +
            $"exit={result.WorkerExitCode}, confirmed={result.WorkerExitConfirmed}");
        area.Preserved(source);
        if (fault is not null)
        {
            area.Rejected(result, fault == "overlap"
                ? [ArchiveError.IntegrityFailure]
                : [ArchiveError.EngineFailure, ArchiveError.IntegrityFailure]);
            if (fault == "overlap") { Check.That(result.WorkerExitCode is null, "항목 경계 침범 뒤 Worker 시작"); }
            return;
        }
        // PR #10의 명시된 첫 스트림 정책을 검사함. 원본 전체 인증을 뜻하지 않음.
        TestArea.Successful(result, entries.Length);
        area.VerifyTree(entries);
        Check.Equal(expectedOutput, result.Usage.OutputBytes);
        Check.That(result.Usage.ProcessedBytes >= 3L * zip.Length, "후행 바이트의 입력 복사/재읽기 예산 누락");
    }

    public static async Task OffsetSource(string root)
    {
        var zip = ZipFixture.Build(Item);
        var area = new TestArea(root, zip);
        using var source = new MemoryStream(new byte[7].Concat(zip).ToArray(), false);
        source.Position = 7;
        var result = await Launch.Client().ExtractToNewFolderAsync(area.Job, source, area.Destination, TestArea.Limits, default);
        TestArea.Successful(result, 1);
        area.Preserved(source, 7);
        area.VerifyTree([Item]);
    }

    public static async Task InvalidSource(string root, string mode)
    {
        var area = new TestArea(root, ZipFixture.Build(Item));
        using Stream source = mode == "writable" ? new MemoryStream(area.Original) : new ObservedSource(area.Original, mode);
        var result = await Launch.Client().ExtractToNewFolderAsync(area.Job, source, area.Destination, TestArea.Limits, default);
        var expected = mode switch
        {
            "writable" or "nonseek" => ArchiveError.UnsupportedOperation,
            "oversize" => ArchiveError.BudgetExceeded,
            _ => ArchiveError.IntegrityFailure
        };
        area.Rejected(result, expected);
        Check.That(result.WorkerExitCode is null, "원본 검사 실패 뒤 Worker 시작");
        if (source is ObservedSource observed) { Check.That(observed.MaximumRead <= 65536, "큰 원본 읽기 요청"); }
        area.Preserved(source);
    }

    public static async Task Budget(string root, string kind)
    {
        var entries = new[] { new FixtureEntry("a", [1, 2]), new FixtureEntry("b", [3, 4]) };
        var area = new TestArea(root, ZipFixture.Build(entries));
        using var source = area.Open();
        var limits = kind switch
        {
            "entries" => TestArea.Limits with { MaxEntries = 1 },
            "output" => TestArea.Limits with { MaxOutputBytes = 3 },
            _ => TestArea.Limits with { MaxProcessedBytes = area.Original.Length - 1 }
        };
        var result = await Launch.Client().ExtractToNewFolderAsync(area.Job, source, area.Destination, limits, default);
        area.Rejected(result, ArchiveError.BudgetExceeded);
        Check.That(result.WorkerExitCode is null, "사전 예산 초과 뒤 Worker 시작");
        area.Preserved(source);
    }

    public static async Task InvalidZip(string root, string kind)
    {
        var bytes = kind == "traversal" ? ZipFixture.Build(new FixtureEntry("../escape", [1])) : ZipFixture.Build(Item);
        if (kind == "crc") { bytes[30 + "payload.bin".Length] ^= 1; }
        if (kind == "truncated") { bytes = bytes[..^1]; }
        if (kind == "duplicate") { bytes = ZipFixture.Build(new("a", []), new("a", [])); }
        var area = new TestArea(root, bytes);
        using var source = area.Open();
        var result = await Launch.Client().ExtractToNewFolderAsync(area.Job, source, area.Destination, TestArea.Limits, default);
        area.Rejected(result, kind switch
        {
            "traversal" => [ArchiveError.UnsafePath],
            "duplicate" => [ArchiveError.OutputConflict],
            "truncated" => [ArchiveError.IntegrityFailure],
            _ => [ArchiveError.EngineFailure, ArchiveError.IntegrityFailure]
        });
        Check.That(!File.Exists(Path.Combine(area.Root, "escape")), "ZIP 경로 탈출");
        area.Preserved(source);
    }

    public static async Task Peer(string root, string mode)
    {
        var area = new TestArea(root, ZipFixture.Build(Item));
        area.Plan(mode);
        using var source = area.Open();
        var result = await Launch.Client(true).ExtractToNewFolderAsync(area.Job, source, area.Destination, TestArea.Limits, default);
        if (mode is "valid" or "range-valid")
        {
            TestArea.Successful(result, 1);
            area.VerifyTree([Item]);
        }
        else
        {
            area.Rejected(result, mode switch
            {
                "unsupported" => ArchiveError.UnsupportedOperation,
                "crc" => ArchiveError.IntegrityFailure,
                "range-budget" => ArchiveError.BudgetExceeded,
                _ => ArchiveError.EngineFailure
            });
        }
        area.Preserved(source);
        ExitedPeer(area);
    }

    public static async Task CancelSource(string root, bool before)
    {
        var area = new TestArea(root, ZipFixture.Build(Item));
        using var source = new ObservedSource(area.Original, before ? "" : "wait");
        using var cancellation = new CancellationTokenSource();
        if (before) { cancellation.Cancel(); }
        var pending = Launch.Client().ExtractToNewFolderAsync(area.Job, source, area.Destination,
            TestArea.Limits, cancellation.Token);
        try
        {
            if (!before)
            {
                await source.Entered.Task.WaitAsync(TimeSpan.FromSeconds(5));
                cancellation.Cancel();
            }
            var result = await pending;
            area.Rejected(result, ArchiveError.Cancelled);
            Check.That(result.WorkerExitCode is null, "스냅샷 취소 뒤 Worker 시작");
            area.Preserved(source);
        }
        finally
        {
            await cancellation.CancelAsync();
            await pending;
        }
    }

    public static async Task Waiting(string root, string mode, bool cancel)
    {
        var area = new TestArea(root, ZipFixture.Build(Item));
        area.Plan(mode);
        using var source = area.Open();
        using var cancellation = new CancellationTokenSource();
        var limits = TestArea.Limits with { MaxDuration = TimeSpan.FromSeconds(cancel ? 10 : 2) };
        var pending = Launch.Client(true).ExtractToNewFolderAsync(area.Job, source, area.Destination, limits, cancellation.Token);
        try
        {
            await area.Ready();
            Check.That(!pending.IsCompleted, "장벽 전에 작업 완료");
            if (mode == "pause-data")
            {
                await TestArea.Wait(() => OutputFiles(area).Any(path => new FileInfo(path).Length == 65536));
            }
            Check.That(!Directory.Exists(area.Destination), "Worker/EOF 확인 전에 게시");
            if (cancel) { cancellation.Cancel(); }
            var result = await pending;
            area.Rejected(result, cancel ? ArchiveError.Cancelled : ArchiveError.BudgetExceeded);
            area.Preserved(source);
            ExitedPeer(area);
        }
        finally
        {
            cancellation.Cancel();
            area.Release();
            await pending;
        }
    }

    public static async Task CleanupFailure(string root, bool input)
    {
        var area = new TestArea(root, ZipFixture.Build(Item));
        area.Plan(input ? "pause-before" : "pause-data");
        using var source = area.Open();
        using var cancellation = new CancellationTokenSource();
        var pending = Launch.Client(true).ExtractToNewFolderAsync(area.Job, source, area.Destination,
            TestArea.Limits, cancellation.Token);
        try
        {
            await area.Ready();
            if (!input) { await TestArea.Wait(() => OutputFiles(area).Any(path => new FileInfo(path).Length == 65536)); }
            var target = input
                ? Directory.GetDirectories(area.Parent, ".zipelo-*.tmp").Single(p => File.Exists(Path.Combine(p, "input.zip")))
                : Path.GetDirectoryName(OutputFiles(area).Single())!;
            var foreign = Path.Combine(target, "foreign.txt");
            File.WriteAllText(foreign, "미소유 항목 보존");
            if (input) { area.Release(); }
            else { cancellation.Cancel(); }
            var result = await pending;
            Check.That(!result.Publication.Published && result.Publication.Error != ArchiveError.None,
                "정리 실패를 게시 성공으로 처리");
            Check.That(!Directory.Exists(area.Destination), "정리 실패 뒤 게시");
            Check.That(result.WorkerExitConfirmed, "정리 전 Worker 종료 미확인");
            Check.Equal(input, result.InputCleanupFailed);
            Check.Equal(!input, result.Publication.CleanupFailed);
            Check.Equal(Path.GetFileName(target),
                input ? result.InputTemporaryName : result.Publication.TemporaryFileName);
            Check.Equal("미소유 항목 보존", File.ReadAllText(foreign));
            area.ExactParent("keep.txt", Path.GetFileName(target));
            area.Preserved(source);
            ExitedPeer(area);
        }
        finally
        {
            cancellation.Cancel();
            area.Release();
            await pending;
        }
    }

    public static async Task Conflict(string root, string kind)
    {
        var area = new TestArea(root, ZipFixture.Build(Item));
        var protectedPath = area.Destination;
        if (kind == "file") { File.WriteAllText(protectedPath, "기존 목적지"); }
        else
        {
            if (kind == "link")
            {
                protectedPath = Path.Combine(area.Root, "external");
                TestArea.CreateDirectory(protectedPath);
                TryLink(area.Destination, protectedPath);
            }
            else { TestArea.CreateDirectory(protectedPath); }
            File.WriteAllText(Path.Combine(protectedPath, "old.txt"), "기존 목적지");
        }
        using var source = area.Open();
        var result = await Launch.Client().ExtractToNewFolderAsync(area.Job, source, area.Destination, TestArea.Limits, default);
        Check.That(!result.Publication.Published, "기존 목적지를 덮어씀");
        Check.That(result.Publication.Error is ArchiveError.OutputConflict or ArchiveError.UnsafePath,
            "충돌 오류가 아님: " + result.Publication.Error);
        TestArea.CleanFlags(result);
        Check.Equal("기존 목적지", File.ReadAllText(kind == "file" ? protectedPath : Path.Combine(protectedPath, "old.txt")));
        if (kind != "file") { Check.Equal(1, Directory.GetFileSystemEntries(protectedPath).Length); }
        if (kind == "link") { Check.That(new DirectoryInfo(area.Destination).LinkTarget is not null, "목적지 링크 변경"); }
        area.ExactParent("keep.txt", "result");
        area.Preserved(source);
    }

    public static async Task ParentLink(string root)
    {
        var area = new TestArea(root, ZipFixture.Build(Item));
        var link = Path.Combine(area.Root, "linked-parent");
        TryLink(link, area.Parent);
        using var source = area.Open();
        var result = await Launch.Client().ExtractToNewFolderAsync(area.Job, source, Path.Combine(link, "result"),
            TestArea.Limits, default);
        area.Rejected(result, ArchiveError.UnsafePath, ArchiveError.OutputUnavailable);
        Check.That(result.WorkerExitCode is null, "링크 부모에 Worker 시작");
        area.Preserved(source);
    }

    public static async Task ParentReplacement(string root)
    {
        var area = new TestArea(root, ZipFixture.Build(Item));
        area.Plan("pause-before");
        using var source = area.Open();
        using var cancellation = new CancellationTokenSource();
        var pending = Launch.Client(true).ExtractToNewFolderAsync(area.Job, source, area.Destination, TestArea.Limits, cancellation.Token);
        var moved = Path.Combine(area.Root, "moved-parent");
        var replaced = false;
        try
        {
            await area.Ready();
            try { Directory.Move(area.Parent, moved); }
            catch (IOException) { throw new Blocked("OS가 열린 부모 이동을 거절하여 교체 후 상태 주입 미실행"); }
            catch (UnauthorizedAccessException) { throw new Blocked("부모 이동 권한 없음, 설정 변경 없이 보류"); }
            replaced = true;
            TestArea.CreateDirectory(area.Parent);
            File.WriteAllText(area.Marker, "교체된 부모 보존");
            area.Release();
            var result = await pending;
            Check.That(!result.Publication.Published, "교체된 부모 경로에서 성공을 반환함");
            Check.That(result.Publication.Error != ArchiveError.None, "경로 교체 오류 누락");
            Check.That(result.WorkerExitConfirmed, "Worker 종료 미확인");
            Check.That(!Directory.Exists(area.Destination) && !Directory.Exists(Path.Combine(moved, "result")),
                "경로 교체 후 원래/이동된 부모에 게시함");
            Check.Equal("교체된 부모 보존", File.ReadAllText(area.Marker));
            Check.Equal("기존 자료 보존", File.ReadAllText(Path.Combine(moved, "keep.txt")));
            Check.Bytes(area.Original, File.ReadAllBytes(area.SourcePath));
            Check.Equal(0L, source.Position);
        }
        finally
        {
            cancellation.Cancel();
            area.Release();
            await pending;
            if (!replaced) { area.Preserved(source); }
        }
    }

    private static string[] OutputFiles(TestArea area) =>
        Directory.GetDirectories(area.Parent, ".zipelo-*.tmp")
            .Select(p => Path.Combine(p, "payload.bin")).Where(File.Exists).ToArray();

    private static void TryLink(string path, string target)
    {
        try { Directory.CreateSymbolicLink(path, target); }
        catch (Exception failure) when (failure is UnauthorizedAccessException or PlatformNotSupportedException or IOException)
        {
            throw new Blocked("시험 링크 생성 불가, 권한 변경 없이 보류: " + failure.GetType().Name);
        }
    }

    private static void ExitedPeer(TestArea area)
    {
        var file = Path.Combine(area.Root, "peer.pid");
        if (!File.Exists(file)) { return; }
        var pid = int.Parse(File.ReadAllText(file));
        try
        {
            using var process = Process.GetProcessById(pid);
            Check.That(process.HasExited, "가짜 Worker 잔류");
        }
        catch (ArgumentException) { }
    }
}

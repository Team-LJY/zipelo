using System.Reflection;
using System.Runtime.ExceptionServices;
using Zipelo.Core;
using Zipelo.Runtime;

namespace Zipelo.StreamingExtractionTests;

internal static class PublicationCases
{
    private const BindingFlags Methods = BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic;

    public static async Task PublicV1(string root)
    {
        var fixture = new FixtureEntry("folder/payload.bin", ZipFixture.Payload(1024));
        var area = new TestArea(root, ZipFixture.Build(fixture));
        var item = Entry(fixture);
        var report = new EntryResult(item.Entry, EntryOutcome.Staged, VerificationState.Passed, ArchiveError.None);
        var model = new ZipWorkerResult(new JobResult(area.Job, JobState.Completed, [report], ArchiveError.None), null)
        {
            ExtractedEntries = [item]
        };
        var result = await NewFolderPublisher.PublishAsync(model, area.Destination, CaseLifetime.Token);
        Check.That(result.Published && result.Error == ArchiveError.None, "v1 공용 게시 정상 대조 실패");
        Check.That(!result.CleanupFailed && result.TemporaryFileName is null, "v1 정상 게시 정리 오류");
        area.VerifyTree([fixture]);
        area.Preserved();
        Check.Bytes(fixture.Bytes, item.Content);
    }

    public static async Task Common(string root, string mutation)
    {
        Check.That(mutation is "none" or "directory" or "missing" or "link", "알 수 없는 부모 교체 종류");
        var fixture = new FixtureEntry("folder/payload.bin", ZipFixture.Payload(1024));
        var area = new TestArea(root, ZipFixture.Build(fixture));
        var moved = Path.Combine(area.Root, "moved-parent");
        var foreign = Path.Combine(area.Root, "foreign");
        TestArea.CreateDirectory(foreign);
        var sentinel = Path.Combine(foreign, "sentinel.txt");
        File.WriteAllText(sentinel, "외부 자료 보존");
        var typeName = OperatingSystem.IsWindows()
            ? "Zipelo.Runtime.WindowsFolderPublication"
            : "Zipelo.Runtime.LinuxFolderPublication";
        var type = typeof(NewFolderPublisher).Assembly.GetType(typeName, throwOnError: true)!;
        var constructor = type.GetConstructor(Methods, null, [typeof(string)], null)
            ?? throw new MissingMethodException(typeName, ".ctor(string)");
        using var pending = (IDisposable)constructor.Invoke([area.Parent]);
        var published = false;
        Exception? rejection = null;
        try
        {
            Invoke(pending, "Initialize");
            await (Task)Invoke(pending, "WriteAsync", Entry(fixture), CaseLifetime.Token)!;
            Invoke(pending, "Verify", CaseLifetime.Token);
            // v1과 v2가 쓰는 실제 공용 게시 객체의 검증 완료 뒤에만 교체함.
            if (mutation != "none")
            {
                try { Directory.Move(area.Parent, moved); }
                catch (IOException) { throw new Blocked("OS가 열린 부모 이동을 거절하여 공용 게시 교체 주입 미실행"); }
                catch (UnauthorizedAccessException) { throw new Blocked("부모 이동 권한 없음, 설정 변경 없이 보류"); }
                if (mutation == "directory")
                {
                    TestArea.CreateDirectory(area.Parent);
                    File.WriteAllText(area.Marker, "교체된 부모 보존");
                }
                if (mutation == "link")
                {
                    try { Directory.CreateSymbolicLink(area.Parent, foreign); }
                    catch (Exception failure) when (failure is IOException or UnauthorizedAccessException or PlatformNotSupportedException)
                    {
                        throw new Blocked("부모 링크 주입 불가, 권한 변경 없이 보류: " + failure.GetType().Name);
                    }
                }
            }
            try
            {
                Invoke(pending, "Publish", "result", CaseLifetime.Token);
                published = true;
            }
            catch (Exception failure) when (failure is ArchiveException ||
                failure.GetType().FullName == "Zipelo.Runtime.PosixPublicationException")
            {
                rejection = failure;
            }
        }
        finally
        {
            if (!published)
            {
                Check.That(Invoke(pending, "Abort") is true, "공용 게시 실패 뒤 소유 임시 트리 정리 실패");
            }
        }

        Console.WriteLine($"OBSERVE common parent {mutation}, published={published}, rejection={rejection?.GetType().Name ?? "none"}");
        Check.Bytes(area.Original, File.ReadAllBytes(area.SourcePath));
        Check.Equal("외부 자료 보존", File.ReadAllText(sentinel));
        Check.That(Directory.GetFileSystemEntries(foreign).Select(Path.GetFileName).SequenceEqual(["sentinel.txt"]),
            "외부 트리에 추가 출력/변경 발생");
        if (mutation == "none")
        {
            Check.That(published && rejection is null, "공용 게시 정상 대조 실패");
            area.VerifyTree([fixture]);
            area.Preserved();
            return;
        }

        Check.That(!published && rejection is not null, "이미 교체된 부모에 공용 게시 성공");
        var error = rejection is ArchiveException archive ? archive.Code :
            rejection?.GetType().GetProperty("Code", Methods)?.GetValue(rejection) as ArchiveError?;
        Check.That(error is ArchiveError.UnsafePath or ArchiveError.OutputUnavailable, "부모 교체 오류 분류 불일치");
        Check.That(!Directory.Exists(area.Destination) && !File.Exists(area.Destination) &&
            !Directory.Exists(Path.Combine(moved, "result")) && !File.Exists(Path.Combine(moved, "result")),
            "원래/이동된 부모에 공용 결과 게시");
        Check.Equal("기존 자료 보존", File.ReadAllText(Path.Combine(moved, "keep.txt")));
        Check.That(Directory.GetFileSystemEntries(moved).Select(Path.GetFileName).SequenceEqual(["keep.txt"]),
            "이동된 부모의 기존 자료/소유 정리 결과 불일치");
        if (mutation == "directory")
        {
            Check.Equal("교체된 부모 보존", File.ReadAllText(area.Marker));
            area.ExactParent("keep.txt");
        }
        if (mutation == "missing")
        {
            Check.That(!Directory.Exists(area.Parent) && !File.Exists(area.Parent), "사라진 부모를 다시 생성함");
        }
        if (mutation == "link")
        {
            Check.Equal(foreign, new DirectoryInfo(area.Parent).LinkTarget);
        }
    }

    private static ZipExtractedEntry Entry(FixtureEntry fixture) => new(
        new ArchiveEntry("publication-fixture", fixture.Name, EntryKind.File, fixture.Bytes.LongLength),
        fixture.Bytes.ToArray(), ZipFixture.Crc(fixture.Bytes));

    private static object? Invoke(object pending, string name, params object[] arguments)
    {
        var method = pending.GetType().GetMethod(name, Methods)
            ?? throw new MissingMethodException(pending.GetType().FullName, name);
        try { return method.Invoke(pending, arguments); }
        catch (TargetInvocationException failure) when (failure.InnerException is not null)
        {
            ExceptionDispatchInfo.Capture(failure.InnerException).Throw();
            throw;
        }
    }
}

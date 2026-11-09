using Zipelo.Core;
using Zipelo.Runtime;

namespace Zipelo.StreamingExtractionTests;

internal static class Program
{
    private static readonly (string Name, Func<string, Task> Run)[] Cases =
    [
        ("real.empty", root => ExtractionCases.Real(root, "empty")),
        ("real.empty-items", root => ExtractionCases.Real(root, "empty-items")),
        ("real.stored", root => ExtractionCases.Real(root, "stored")),
        ("real.deflate", root => ExtractionCases.Real(root, "deflate")),
        ("real.nested", root => ExtractionCases.Real(root, "nested")),
        ("source.position", root => ExtractionCases.OffsetSource(root)),
        ("source.writable", root => ExtractionCases.InvalidSource(root, "writable")),
        ("source.nonseek", root => ExtractionCases.InvalidSource(root, "nonseek")),
        ("source.oversize", root => ExtractionCases.InvalidSource(root, "oversize")),
        ("source.content-change", root => ExtractionCases.InvalidSource(root, "content-change")),
        ("source.length-change", root => ExtractionCases.InvalidSource(root, "length-change")),
        ("budget.entries", root => ExtractionCases.Budget(root, "entries")),
        ("budget.output", root => ExtractionCases.Budget(root, "output")),
        ("budget.processed", root => ExtractionCases.Budget(root, "processed")),
        ("zip.traversal", root => ExtractionCases.InvalidZip(root, "traversal")),
        ("zip.duplicate", root => ExtractionCases.InvalidZip(root, "duplicate")),
        ("zip.crc", root => ExtractionCases.InvalidZip(root, "crc")),
        ("zip.truncated", root => ExtractionCases.InvalidZip(root, "truncated")),
        ("peer.valid", root => ExtractionCases.Peer(root, "valid")),
        ("peer.range-valid", root => ExtractionCases.Peer(root, "range-valid")),
        ("peer.version", root => ExtractionCases.Peer(root, "version")),
        ("peer.wrong-job", root => ExtractionCases.Peer(root, "wrong-job")),
        ("peer.wrong-item", root => ExtractionCases.Peer(root, "wrong-item")),
        ("peer.sequence", root => ExtractionCases.Peer(root, "sequence")),
        ("peer.wrong-length", root => ExtractionCases.Peer(root, "wrong-length")),
        ("peer.huge-chunk", root => ExtractionCases.Peer(root, "huge-chunk")),
        ("peer.truncated-header", root => ExtractionCases.Peer(root, "truncated-header")),
        ("peer.truncated-body", root => ExtractionCases.Peer(root, "truncated-body")),
        ("peer.crc", root => ExtractionCases.Peer(root, "crc")),
        ("peer.missing-item", root => ExtractionCases.Peer(root, "missing-item")),
        ("peer.extra-item", root => ExtractionCases.Peer(root, "extra-item")),
        ("peer.short-data", root => ExtractionCases.Peer(root, "short-data")),
        ("peer.duplicate-chunk", root => ExtractionCases.Peer(root, "duplicate-chunk")),
        ("peer.early-eof", root => ExtractionCases.Peer(root, "early-eof")),
        ("peer.failed-end", root => ExtractionCases.Peer(root, "failed-end")),
        ("peer.cancelled-end", root => ExtractionCases.Peer(root, "cancelled-end")),
        ("peer.trailing", root => ExtractionCases.Peer(root, "trailing")),
        ("peer.nonzero", root => ExtractionCases.Peer(root, "nonzero")),
        ("peer.unsupported", root => ExtractionCases.Peer(root, "unsupported")),
        ("peer.range-version", root => ExtractionCases.Peer(root, "range-version")),
        ("peer.range-id", root => ExtractionCases.Peer(root, "range-id")),
        ("peer.range-sequence", root => ExtractionCases.Peer(root, "range-sequence")),
        ("peer.range-offset", root => ExtractionCases.Peer(root, "range-offset")),
        ("peer.range-count", root => ExtractionCases.Peer(root, "range-count")),
        ("peer.range-reserved", root => ExtractionCases.Peer(root, "range-reserved")),
        ("peer.range-budget", root => ExtractionCases.Peer(root, "range-budget")),
        ("cancel.before_snapshot", root => ExtractionCases.CancelSource(root, true)),
        ("cancel.snapshot_read", root => ExtractionCases.CancelSource(root, false)),
        ("cancel.silent", root => ExtractionCases.Waiting(root, "silent", true)),
        ("cancel.pause-data", root => ExtractionCases.Waiting(root, "pause-data", true)),
        ("deadline.silent", root => ExtractionCases.Waiting(root, "silent", false)),
        ("deadline.no-eof", root => ExtractionCases.Waiting(root, "no-eof", false)),
        ("deadline.no-exit", root => ExtractionCases.Waiting(root, "no-exit", false)),
        ("cleanup.input_foreign", root => ExtractionCases.CleanupFailure(root, true)),
        ("cleanup.output_foreign", root => ExtractionCases.CleanupFailure(root, false)),
        ("destination.file", root => ExtractionCases.Conflict(root, "file")),
        ("destination.directory", root => ExtractionCases.Conflict(root, "directory")),
        ("destination.link", root => ExtractionCases.Conflict(root, "link")),
        ("path.parent_link", root => ExtractionCases.ParentLink(root)),
        ("path.parent_replacement", root => ExtractionCases.ParentReplacement(root)),
    ];

    public static async Task<int> Main(string[] args)
    {
        if (args is ["--extract-stream-v2"]) { return await FakeWorker.Run(); }
        if (args is ["--list"])
        {
            foreach (var item in Cases) { Console.WriteLine(item.Name); }
            Console.WriteLine($"등록 {Cases.Length}");
            return 0;
        }
        if (args is not ["--sandbox-parent", var parent] || !Path.IsPathFullyQualified(parent) || !Directory.Exists(parent))
        {
            Console.Error.WriteLine("사용법: --sandbox-parent <기존 시험 폴더의 절대 경로>");
            return 2;
        }
        for (DirectoryInfo? current = new(Path.GetFullPath(parent)); current is not null; current = current.Parent)
        {
            if ((current.Attributes & FileAttributes.ReparsePoint) != 0)
            {
                Console.Error.WriteLine("시험 부모의 링크를 허용하지 않음");
                return 2;
            }
        }
        var root = Path.Combine(parent, "streaming-" + Guid.NewGuid().ToString("N"));
        TestArea.CreateDirectory(root);
        Console.WriteLine("시험 생성 경로: " + root);
        var originalDirectory = Environment.CurrentDirectory;
        try
        {
            _ = Launch.Client();
            _ = Launch.Client(true);
            var probe = Path.Combine(root, "platform-probe");
            var model = new ZipWorkerResult(new(Guid.NewGuid(), JobState.Completed, [], ArchiveError.None), null)
            {
                ExtractedEntries = []
            };
            var platform = await NewFolderPublisher.PublishAsync(model, probe, default);
            if (!platform.Published || platform.CleanupFailed)
            {
                Console.Error.WriteLine($"BLOCKED 플랫폼 사전검사: {platform.Error}, POSIX={platform.PosixError}");
                Console.WriteLine($"등록 {Cases.Length}, 실행 0, 차단 {Cases.Length}");
                return 3;
            }
            var passed = 0;
            var failed = 0;
            var blocked = 0;
            foreach (var (name, run) in Cases)
            {
                try
                {
                    await run(root).WaitAsync(TimeSpan.FromSeconds(45));
                    Console.WriteLine("PASS " + name);
                    passed++;
                }
                catch (Blocked reason)
                {
                    Console.WriteLine("BLOCKED " + name + ": " + reason.Message);
                    blocked++;
                }
                catch (TimeoutException failure)
                {
                    Console.Error.WriteLine("FAIL " + name + ": " + failure.Message);
                    Console.WriteLine($"등록 {Cases.Length}, 통과 {passed}, 실패 {failed + 1}, 차단 {blocked}, 미실행 {Cases.Length - passed - failed - blocked - 1}");
                    return 124;
                }
                catch (Exception failure)
                {
                    Console.Error.WriteLine($"FAIL {name}: {failure.GetType().Name}: {failure.Message}");
                    failed++;
                }
                finally { Environment.CurrentDirectory = originalDirectory; }
            }
            Console.WriteLine($"등록 {Cases.Length}, 통과 {passed}, 실패 {failed}, 차단 {blocked}");
            return failed != 0 ? 1 : blocked != 0 ? 3 : 0;
        }
        catch (Exception failure)
        {
            Console.Error.WriteLine("SETUP_FAIL: " + failure.Message);
            return 2;
        }
        finally
        {
            Environment.CurrentDirectory = originalDirectory;
            Console.WriteLine("시험 경로는 검토를 위해 보존함: " + root);
        }
    }
}

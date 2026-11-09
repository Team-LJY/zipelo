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
        ("zip.deflate_tail_garbage_small", root => ExtractionCases.DeflateTail(root, false, false)),
        ("zip.deflate_tail_garbage_large", root => ExtractionCases.DeflateTail(root, false, true)),
        ("zip.deflate_tail_second_small", root => ExtractionCases.DeflateTail(root, true, false)),
        ("zip.deflate_tail_second_large", root => ExtractionCases.DeflateTail(root, true, true)),
        ("zip.deflate_tail_bad_crc", root => ExtractionCases.DeflateTail(root, true, false, "crc")),
        ("zip.deflate_tail_bad_length", root => ExtractionCases.DeflateTail(root, true, false, "length")),
        ("zip.deflate_tail_overlap", root => ExtractionCases.DeflateTail(root, false, false, "overlap")),
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
        ("publication.v1_normal", root => PublicationCases.PublicV1(root)),
        ("publication.common_normal", root => PublicationCases.Common(root, "none")),
        ("publication.common_parent_replaced", root => PublicationCases.Common(root, "directory")),
        ("publication.common_parent_missing", root => PublicationCases.Common(root, "missing")),
        ("publication.common_parent_link", root => PublicationCases.Common(root, "link")),
    ];

    public static async Task<int> Main(string[] args)
    {
        if (args is ["--extract-stream-v2"]) { return await FakeWorker.Run(); }
        var list = args.Length > 0 && args[0] == "--list";
        var parent = !list && args.Length >= 2 && args[0] == "--sandbox-parent" ? args[1] : null;
        if (!list && parent is null)
        {
            Console.Error.WriteLine("사용법: --list [--case <등록 이름>] 또는 --sandbox-parent <기존 시험 폴더의 절대 경로> [--case <등록 이름>]");
            return 2;
        }
        (string Name, Func<string, Task> Run)[] selectedCases;
        try { selectedCases = SelectCases(args, list ? 1 : 2); }
        catch (ArgumentException failure)
        {
            Console.Error.WriteLine("SETUP_FAIL: " + failure.Message);
            return 2;
        }
        var countLabel = selectedCases.Length == Cases.Length ? $"등록 {Cases.Length}" :
            $"등록 {Cases.Length}, 선택 {selectedCases.Length}, 제외 {Cases.Length - selectedCases.Length}";
        if (list)
        {
            foreach (var item in selectedCases) { Console.WriteLine(item.Name); }
            Console.WriteLine(countLabel);
            return 0;
        }
        if (parent is null || !Path.IsPathFullyQualified(parent) || !Directory.Exists(parent))
        {
            Console.Error.WriteLine("시험 부모는 기존 시험 폴더의 절대 경로여야 함");
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
        var restoreDirectory = true;
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
                Console.WriteLine($"{countLabel}, 실행 0, 차단 {selectedCases.Length}");
                return 3;
            }
            var passed = 0;
            var failed = 0;
            var blocked = 0;
            foreach (var (name, run) in selectedCases)
            {
                using var lifetime = new CaseLifetime();
                var pending = Task.Run(() => run(root));
                Exception? failure = null;
                try { await pending.WaitAsync(TimeSpan.FromSeconds(45)); }
                catch (Exception observed) { failure = observed; }

                var stopped = await lifetime.StopAsync(pending, name, failure is not null);
                if (!stopped || failure is TimeoutException)
                {
                    Console.Error.WriteLine($"FAIL {name}: {failure?.Message ?? "작업/Worker 종료 미확인"}");
                    Console.WriteLine($"{countLabel}, 통과 {passed}, 실패 {failed + 1}, 차단 {blocked}, 미실행 {selectedCases.Length - passed - failed - blocked - 1}");
                    restoreDirectory = stopped;
                    return 124;
                }
                Environment.CurrentDirectory = originalDirectory;
                if (failure is Blocked reason)
                {
                    Console.WriteLine("BLOCKED " + name + ": " + reason.Message);
                    blocked++;
                }
                else if (failure is not null)
                {
                    Console.Error.WriteLine($"FAIL {name}: {failure.GetType().Name}: {failure.Message}");
                    failed++;
                }
                else
                {
                    Console.WriteLine("PASS " + name);
                    passed++;
                }
            }
            Console.WriteLine($"{countLabel}, 통과 {passed}, 실패 {failed}, 차단 {blocked}");
            return failed != 0 ? 1 : blocked != 0 ? 3 : 0;
        }
        catch (Exception failure)
        {
            Console.Error.WriteLine("SETUP_FAIL: " + failure.Message);
            return 2;
        }
        finally
        {
            if (restoreDirectory) { Environment.CurrentDirectory = originalDirectory; }
            Console.WriteLine("시험 경로는 검토를 위해 보존함: " + root);
        }
    }

    private static (string Name, Func<string, Task> Run)[] SelectCases(string[] args, int start)
    {
        var names = new HashSet<string>(StringComparer.Ordinal);
        for (var index = start; index < args.Length; index += 2)
        {
            if (args[index] != "--case" || index + 1 >= args.Length)
            {
                throw new ArgumentException("--case <등록 이름> 쌍만 허용함");
            }
            var name = args[index + 1];
            if (!Cases.Any(item => string.Equals(item.Name, name, StringComparison.Ordinal)))
            {
                throw new ArgumentException("등록되지 않은 사례 이름임");
            }
            if (!names.Add(name)) { throw new ArgumentException("중복 사례 선택을 허용하지 않음"); }
        }
        return names.Count == 0 ? Cases : Cases.Where(item => names.Contains(item.Name)).ToArray();
    }
}

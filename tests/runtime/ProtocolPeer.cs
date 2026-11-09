using System.Buffers.Binary;
using System.Text.Json;
using Zipelo.Core;
using Zipelo.Runtime;

namespace Zipelo.RuntimeTests;

// 제품 작업자를 대체하지 않는 비정상 응답 시험용 자식 프로세스임.
internal static class ProtocolPeer
{
    public static async Task<int> Run()
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(30));
        var input = Console.OpenStandardInput();
        var output = Console.OpenStandardOutput();
        var header = new byte[4];
        await input.ReadExactlyAsync(header, timeout.Token);
        var size = BinaryPrimitives.ReadInt32LittleEndian(header);
        if (size <= 0 || size > 4096) { return 2; }
        var bytes = new byte[size];
        await input.ReadExactlyAsync(bytes, timeout.Token);
        using var document = JsonDocument.Parse(bytes);
        var request = document.RootElement;
        var id = request.GetProperty("JobId").GetGuid();
        var mode = request.GetProperty("Names")[0].GetString();
        if (mode == "hang")
        {
            var marker = System.Text.Encoding.UTF8.GetString(request.GetProperty("Inputs")[0].GetBytesFromBase64());
            var directory = Path.GetDirectoryName(marker)!;
            // 실행기가 검증한 sandbox 안의 표식만 작성함.
            if (!Path.IsPathFullyQualified(marker) || !Directory.Exists(directory) ||
                !Path.GetRelativePath(directory, marker).Equals("child.pid", StringComparison.Ordinal) ||
                !directory.StartsWith(Directory.GetCurrentDirectory() + Path.DirectorySeparatorChar,
                    StringComparison.Ordinal) ||
                !directory.Contains(Path.Combine("tests", "security", ".work") + Path.DirectorySeparatorChar,
                    StringComparison.Ordinal))
            {
                return 2;
            }
            await File.WriteAllTextAsync(marker + ".pending", Environment.ProcessId.ToString(), timeout.Token);
            File.Move(marker + ".pending", marker);
            await Task.Delay(Timeout.Infinite, timeout.Token);
            return 2;
        }
        if (mode == "exit") { return 9; }
        if (mode is "zero" or "negative" or "oversized")
        {
            var length = mode == "zero" ? 0 : mode == "negative" ? -1 : 32 * 1024 * 1024 + 1;
            BinaryPrimitives.WriteInt32LittleEndian(header, length);
            await output.WriteAsync(header, timeout.Token);
            await output.FlushAsync(timeout.Token);
            return 0;
        }
        if (mode is "truncated" or "bad-json")
        {
            BinaryPrimitives.WriteInt32LittleEndian(header, mode == "truncated" ? 10 : 1);
            await output.WriteAsync(header, timeout.Token);
            await output.WriteAsync(new byte[] { (byte)'{' }, timeout.Token);
            await output.FlushAsync(timeout.Token);
            return 0;
        }

        var state = mode == "nonterminal" ? JobState.Running : JobState.Completed;
        var error = mode == "success-error" ? ArchiveError.IntegrityFailure : ArchiveError.None;
        var result = new ZipWorkerResult(
            new(mode == "wrong-id" ? Guid.NewGuid() : id, state, Array.Empty<EntryResult>(), error),
            mode is "missing-zip" or "nonterminal" ? null : new byte[] { 80, 75 });
        var payload = JsonSerializer.SerializeToUtf8Bytes(result);
        BinaryPrimitives.WriteInt32LittleEndian(header, payload.Length);
        await output.WriteAsync(header, timeout.Token);
        await output.WriteAsync(payload, timeout.Token);
        await output.FlushAsync(timeout.Token);
        return 0;
    }
}

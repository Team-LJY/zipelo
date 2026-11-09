using System.Buffers.Binary;
using System.Text.Json;
using System.Runtime.InteropServices;

namespace Zipelo.StreamingExtractionTests;

// 제품 프레임 인코더를 쓰지 않는 제한된 시험 peer임.
internal static class FakeWorker
{
    public static async Task<int> Run()
    {
        try
        {
            using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(12));
            var token = timeout.Token;
            var input = Console.OpenStandardInput();
            var output = Console.OpenStandardOutput();
            var requests = Console.OpenStandardError();
            var config = new byte[64];
            await input.ReadExactlyAsync(config, token);
            Check.That(config.AsSpan(0, 4).SequenceEqual("ZXE2"u8), "설정 magic 오류");
            Check.Equal(2, BinaryPrimitives.ReadInt32LittleEndian(config.AsSpan(4)));
            var job = new Guid(config.AsSpan(8, 16));
            var planPath = Path.Combine(Environment.CurrentDirectory, "peer.json");
            Check.That(new FileInfo(planPath).Length < 2048 &&
                (File.GetAttributes(planPath) & FileAttributes.ReparsePoint) == 0, "시험 계획 오류");
            var plan = JsonSerializer.Deserialize<PeerPlan>(File.ReadAllText(planPath))!;
            Check.Equal(job, plan.Job);
            var mode = plan.Mode;
            File.WriteAllText("peer.pid", Environment.ProcessId.ToString());
            if (mode == "unsupported") { return 2; }
            if (mode == "silent") { await Pause(token); return 7; }
            if (mode.StartsWith("range-", StringComparison.Ordinal))
            {
                var count = mode == "range-budget" ? 65536 : 4;
                var rounds = mode == "range-budget" ? 769 : 1;
                for (var index = 0; index < rounds; index++)
                {
                    var request = new byte[48];
                    "ZXR2"u8.CopyTo(request);
                    BinaryPrimitives.WriteInt32LittleEndian(request.AsSpan(4), mode == "range-version" ? 9 : 2);
                    (mode == "range-id" ? Guid.Empty : job).TryWriteBytes(request.AsSpan(8, 16));
                    BinaryPrimitives.WriteUInt64LittleEndian(request.AsSpan(24), mode == "range-sequence" ? 1UL : (ulong)index);
                    BinaryPrimitives.WriteInt64LittleEndian(request.AsSpan(32), mode == "range-offset" ? long.MaxValue : 0);
                    BinaryPrimitives.WriteInt32LittleEndian(request.AsSpan(40), mode == "range-count" ? int.MaxValue : count);
                    BinaryPrimitives.WriteInt32LittleEndian(request.AsSpan(44), mode == "range-reserved" ? 1 : 0);
                    await requests.WriteAsync(request, token);
                    await requests.FlushAsync(token);
                    if (mode is not ("range-valid" or "range-budget")) { await Pause(token); return 7; }
                    var response = new byte[48 + count];
                    await input.ReadExactlyAsync(response, token);
                    Check.That(request.AsSpan().SequenceEqual(response.AsSpan(0, 48)), "범위 응답 헤더 불일치");
                    Check.That(response.AsSpan(48, 4).SequenceEqual(new byte[] { 0x50, 0x4B, 3, 4 }), "범위 응답 내용 불일치");
                }
                if (mode == "range-budget") { return 7; }
            }
            if (mode == "pause-before") { await Pause(token); }
            var itemBytes = new byte[16];
            BinaryPrimitives.WriteInt32LittleEndian(itemBytes, mode == "wrong-item" ? 2 : 1);
            var item = new Guid(itemBytes);
            ulong sequence = 0;

            async Task Send(ushort kind, Guid itemId, byte[] payload, int? declared = null)
            {
                var header = new byte[64];
                "ZIP2"u8.CopyTo(header);
                BinaryPrimitives.WriteUInt16LittleEndian(header.AsSpan(4), (ushort)(mode == "version" ? 9 : 2));
                BinaryPrimitives.WriteUInt16LittleEndian(header.AsSpan(6), kind);
                BinaryPrimitives.WriteUInt32LittleEndian(header.AsSpan(8), (uint)(declared ?? payload.Length));
                (mode == "wrong-job" ? Guid.Empty : job).TryWriteBytes(header.AsSpan(16, 16));
                itemId.TryWriteBytes(header.AsSpan(32, 16));
                BinaryPrimitives.WriteUInt64LittleEndian(header.AsSpan(48),
                    mode == "sequence" && sequence == 1 ? 2UL : sequence);
                sequence++;
                if (mode == "truncated-header") { await output.WriteAsync(header.AsMemory(0, 23), token); return; }
                await output.WriteAsync(header, token);
                await output.WriteAsync(payload, token);
                await output.FlushAsync(token);
            }
            static byte[] Length(ulong value)
            {
                var bytes = new byte[8];
                BinaryPrimitives.WriteUInt64LittleEndian(bytes, value);
                return bytes;
            }
            await Send(1, Guid.Empty, []);
            if (mode is "version" or "wrong-job" or "truncated-header") { return 0; }
            if (mode == "missing-item") { await Send(5, Guid.Empty, [1]); return 0; }
            await Send(2, item, Length(mode == "wrong-length" ? 65538UL : 65537UL));
            if (mode is "sequence" or "wrong-item" or "wrong-length") { return 0; }
            if (mode == "huge-chunk") { await Send(3, item, [], int.MaxValue); return 0; }
            if (mode == "truncated-body") { await Send(3, item, [1], 8); return 0; }
            var payload = ZipFixture.Payload();
            if (mode == "crc") { payload[0] ^= 1; }
            await Send(3, item, payload[..65536]);
            if (mode == "pause-data") { await Pause(token); }
            if (mode == "short-data") { await Send(4, item, Length(65537)); return 0; }
            if (mode == "duplicate-chunk") { sequence--; }
            await Send(3, item, payload[65536..]);
            if (mode == "duplicate-chunk") { return 0; }
            await Send(4, item, Length(65537));
            if (mode == "early-eof") { return 0; }
            if (mode == "extra-item") { await Send(2, new Guid("00000002-0000-0000-0000-000000000000"), Length(0)); return 0; }
            await Send(5, Guid.Empty, [mode == "failed-end" ? (byte)2 : mode == "cancelled-end" ? (byte)3 : (byte)1]);
            if (mode == "trailing") { await output.WriteAsync(new byte[] { 1 }, token); }
            await output.FlushAsync(token);
            if (mode == "no-eof") { await Pause(token); return 7; }
            if (mode == "no-exit")
            {
                output.Dispose();
                CloseOutput();
                await Pause(token);
                return 7;
            }
            return mode == "nonzero" ? 7 : 0;
        }
        catch { return 71; }
    }

    private static void CloseOutput()
    {
        // 시험 자식 자신의 stdout만 닫아 EOF와 프로세스 종료를 분리함.
        if (OperatingSystem.IsWindows()) { Check.That(CloseHandle(GetStdHandle(-11)), "stdout 닫기 실패"); }
        else { Check.Equal(0, Close(1)); }
    }

    [DllImport("kernel32.dll")]
    private static extern nint GetStdHandle(int kind);
    [DllImport("kernel32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseHandle(nint handle);
    [DllImport("libc", EntryPoint = "close")]
    private static extern int Close(int descriptor);

    private static async Task Pause(CancellationToken token)
    {
        File.WriteAllText("ready", "1");
        while (!File.Exists("release")) { await Task.Delay(10, token); }
    }
}

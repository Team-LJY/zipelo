using System.Diagnostics;
using System.Security.Cryptography;
using System.Text;
using System.Threading.Channels;

namespace Zipelo.Runtime.Streaming;

public static class StreamingExtractionDiagnostics
{
#if ZIPELO_STREAM_DIAGNOSTICS
    private sealed record Observation(long Ticks, Guid Job, string Stage, long Value, ulong Sequence, string Identity);

    private static readonly Channel<Observation> Queue = Channel.CreateBounded<Observation>(
        new BoundedChannelOptions(1024) { SingleReader = true, FullMode = BoundedChannelFullMode.Wait });
    private static readonly TaskCompletionSource Completion = new(TaskCreationOptions.RunContinuationsAsynchronously);
    private static int started;
    private static long accepted;
    private static long dropped;
    private static int status;
#endif

    public static bool IsEnabled
    {
        get
        {
#if ZIPELO_STREAM_DIAGNOSTICS
            return true;
#else
            return false;
#endif
        }
    }

    // 시험이 소유한 새 파일만 지정함. 반환값은 쓰기 성공이 아닌 시작 요청 수락 여부임.
    public static bool TryStart(string absoluteFilePath)
    {
#if ZIPELO_STREAM_DIAGNOSTICS
        try
        {
            if (absoluteFilePath.Length > 32768 || !Path.IsPathFullyQualified(absoluteFilePath) ||
                Interlocked.CompareExchange(ref started, 1, 0) != 0) { return false; }
            var writer = new Thread(() => Drain(absoluteFilePath)) { IsBackground = true, Name = "Zipelo stream diagnostics" };
            writer.Start();
            Record(Guid.Empty, "diagnostic.start", Stopwatch.Frequency);
            return true;
        }
        catch
        {
            Volatile.Write(ref status, 3);
            Queue.Writer.TryComplete();
            Completion.TrySetResult();
            return false;
        }
#else
        return false;
#endif
    }

    // 0은 미개방, 1은 기록 중, 2는 정상 닫힘, 3은 실패, 4는 용량 도달임.
    public static int Status
    {
        get
        {
#if ZIPELO_STREAM_DIAGNOSTICS
            return Volatile.Read(ref status);
#else
            return 0;
#endif
        }
    }

    public static long Dropped
    {
        get
        {
#if ZIPELO_STREAM_DIAGNOSTICS
            return Interlocked.Read(ref dropped);
#else
            return 0;
#endif
        }
    }

    // 제품은 완료를 기다리지 않음. 시험 호출자는 별도 제한 시간으로만 기다릴 것.
    public static Task StopAsync()
    {
#if ZIPELO_STREAM_DIAGNOSTICS
        if (Volatile.Read(ref started) == 0) { return Task.CompletedTask; }
        Queue.Writer.TryComplete();
        return Completion.Task;
#else
        return Task.CompletedTask;
#endif
    }

    [Conditional("ZIPELO_STREAM_DIAGNOSTICS")]
    internal static void Record(Guid job, string stage, long value = 0, ulong sequence = 0, string identity = "")
    {
#if ZIPELO_STREAM_DIAGNOSTICS
        try
        {
            if (Volatile.Read(ref started) == 0) { return; }
            if (Interlocked.Increment(ref accepted) > 16384 ||
                !Queue.Writer.TryWrite(new(Stopwatch.GetTimestamp(), job, stage, value, sequence, identity)))
            {
                Interlocked.Increment(ref dropped);
            }
        }
        catch { Interlocked.Increment(ref dropped); }
#endif
    }

    [Conditional("ZIPELO_STREAM_DIAGNOSTICS")]
    internal static void ProcessStarted(Guid job, int pid, string executable, string worker)
    {
#if ZIPELO_STREAM_DIAGNOSTICS
        try
        {
            // 전체 경로는 기록하지 않고 정확한 실행 입력을 해시로 대응함.
            Record(job, "process.started", pid, identity: PathIdentity(executable));
            Record(job, "process.executable_name", pid, identity: SafeName(executable));
            Record(job, "process.worker_identity", pid, identity: PathIdentity(worker));
            Record(job, "process.worker_name", pid, identity: SafeName(worker));
        }
        catch { Interlocked.Increment(ref dropped); }
#endif
    }

#if ZIPELO_STREAM_DIAGNOSTICS
    private static string PathIdentity(string path)
        => path.Length <= 32768 ? Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(path))) : "omitted";

    private static string SafeName(string path)
    {
        var name = Path.GetFileName(path);
        return name.Length <= 128 && name.All(c => char.IsAsciiLetterOrDigit(c) || c is '.' or '_' or '-')
            ? name : "omitted";
    }

    private static void Drain(string path)
    {
        try
        {
            using var file = new FileStream(path, FileMode.CreateNew, FileAccess.Write, FileShare.Read,
                4096, FileOptions.None);
            Volatile.Write(ref status, 1);
            long bytes = 0;
            while (Queue.Reader.WaitToReadAsync().AsTask().GetAwaiter().GetResult())
            {
                while (Queue.Reader.TryRead(out var item))
                {
                    var line = FormattableString.Invariant(
                        $"{item.Ticks}\t{item.Job:N}\t{Environment.ProcessId}\t{item.Stage}\t{item.Value}\t{item.Sequence}\t{item.Identity}\n");
                    var encoded = Encoding.UTF8.GetBytes(line);
                    if (encoded.Length > 512 || bytes + encoded.Length > 4 * 1024 * 1024)
                    {
                        Volatile.Write(ref status, 4);
                        return;
                    }
                    file.Write(encoded);
                    file.Flush();
                    bytes += encoded.Length;
                }
            }
            Volatile.Write(ref status, 2);
        }
        catch { Volatile.Write(ref status, 3); }
        finally
        {
            Queue.Writer.TryComplete();
            Completion.TrySetResult();
        }
    }
#endif
}

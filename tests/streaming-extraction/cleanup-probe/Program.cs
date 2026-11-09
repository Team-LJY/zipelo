#if ZIPELO_STREAM_DIAGNOSTICS
using System.Diagnostics;
using System.Reflection;
using System.Runtime.ExceptionServices;
using Microsoft.Win32.SafeHandles;
using Zipelo.Core;
using Zipelo.Runtime;
using Zipelo.Runtime.Streaming;
#endif

namespace Zipelo.CleanupProbe;

internal static class Program
{
#if ZIPELO_STREAM_DIAGNOSTICS
    public static async Task<int> Main(string[] args)
    {
        if (args is ["--list"])
        {
            Console.WriteLine("cleanup.empty\ncleanup.sync\ncleanup.async\ncleanup.sequence24");
            return 0;
        }
        if (args is not ["--sandbox-parent", var parent, "--case", var name] ||
            name is not ("cleanup.empty" or "cleanup.sync" or "cleanup.async" or "cleanup.sequence24") ||
            !Path.IsPathFullyQualified(parent))
        {
            Console.Error.WriteLine("사용법: --sandbox-parent <기존 시험 폴더의 절대 경로> --case <등록 이름>");
            return 2;
        }
        if (!StreamingExtractionDiagnostics.IsEnabled)
        {
            Console.Error.WriteLine("진단 Runtime 빌드가 필요함");
            return 2;
        }
        if (!OperatingSystem.IsWindows())
        {
            Console.Error.WriteLine("BLOCKED: Windows NTFS 정리 분리 검사임");
            return 3;
        }

        RuntimeAccess runtime;
        try { runtime = new RuntimeAccess(); }
        catch (Exception failure)
        {
            Console.Error.WriteLine("SETUP_FAIL: " + failure.GetType().Name);
            return 2;
        }
        if (name == "cleanup.sequence24") { return await ObserveSequenceAsync(runtime, parent); }

        var state = new ProbeState(runtime, name);
        var cancellation = new CancellationTokenSource();
        var registration = cancellation.Token.Register(() => state.Record("probe.token.cancelled"));
        cancellation.CancelAfter(TimeSpan.FromSeconds(15));
        var pending = Task.Run(() => RunAsync(runtime, state, parent, cancellation.Token));
        var result = 1;
        var timedOut = false;
        try { result = await pending.WaitAsync(TimeSpan.FromSeconds(45)); }
        catch (TimeoutException)
        {
            timedOut = true;
            Console.Error.WriteLine("FAIL: 사례 45초 제한 초과");
        }
        catch (Exception failure) { Console.Error.WriteLine("FAIL: " + failure.GetType().Name); }

        // 같은 작업의 종료만 관찰하며 별도 정리 작업이나 다음 사례를 시작하지 않음.
        var stop = Task.WhenAll(pending, cancellation.CancelAsync());
        try { await stop.WaitAsync(TimeSpan.FromSeconds(25)); }
        catch (Exception failure) { Console.Error.WriteLine("STOP_OBSERVATION: " + failure.GetType().Name); }
        var settled = stop.IsCompleted;
        if (timedOut || !settled || !state.CleanupConfirmed) { result = 124; }
        if (settled)
        {
            registration.Dispose();
            cancellation.Dispose();
        }
        state.Record("probe.observer.end", result);
        var logStopped = false;
        try
        {
            await StreamingExtractionDiagnostics.StopAsync().WaitAsync(TimeSpan.FromSeconds(1));
            logStopped = true;
        }
        catch (Exception failure) { Console.Error.WriteLine("DIAGNOSTICS_STOP_UNCONFIRMED: " + failure.GetType().Name); }
        var status = StreamingExtractionDiagnostics.Status;
        var dropped = StreamingExtractionDiagnostics.Dropped;
        var complete = state.DiagnosticStarted && logStopped && status == 2 && dropped == 0 && result != 124;
        Console.WriteLine($"PROBE case={name}, hostPid={Environment.ProcessId}, job={state.Job:N}, caseExit={result}, " +
            $"taskSettled={settled}, constructionAttempted={state.ConstructionAttempted}, cleanupConfirmed={state.CleanupConfirmed}, abortReturned={state.AbortReturned}, " +
            $"abortSucceeded={state.AbortSucceeded}, disposeReturned={state.DisposeReturned}, residueChecked={state.ResidueChecked}, stage={state.Stage}");
        Console.WriteLine($"DIAGNOSTICS stopped={logStopped}, status={status}, dropped={dropped}, logComplete={complete}");
        if (result == 124)
        {
            Console.Error.WriteLine("CLEANUP_UNCONFIRMED: 미정리 가능, 다음 실행 전 중앙의 프로세스 종료와 잔류 항목 확인 필요");
        }
        if (!complete) { Console.Error.WriteLine("DIAGNOSTICS_INCOMPLETE"); }
        return result == 0 && !complete ? 2 : result;
    }

    private static async Task<int> RunAsync(RuntimeAccess runtime, ProbeState state, string parent,
        CancellationToken token, bool startDiagnostics = true, int operationIndex = 0)
    {
        object? publication = null;
        string? destination = null;
        string? sentinel = null;
        var result = 0;
        var initialized = false;
        byte[] bytes = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
        try
        {
            token.ThrowIfCancellationRequested();
            RequireParent(parent);
            token.ThrowIfCancellationRequested();
            var runRoot = Path.Combine(parent, "cleanup-probe-" + Guid.NewGuid().ToString("N"));
            if (Directory.Exists(runRoot) || File.Exists(runRoot)) { throw new IOException("시험 경로 충돌"); }
            Directory.CreateDirectory(runRoot);
            token.ThrowIfCancellationRequested();
            if (startDiagnostics)
            {
                var trace = Path.Combine(runRoot, "cleanup-diagnostics.tsv");
                state.DiagnosticStarted = StreamingExtractionDiagnostics.TryStart(trace);
                if (!state.DiagnosticStarted) { throw new InvalidOperationException("진단 시작 거절"); }
            }
            Console.WriteLine("시험 생성 경로: " + runRoot);
            state.Record("probe.process", Environment.ProcessId);
            if (startDiagnostics) { state.Record("probe.deadline.scheduled_ms", 15000); }
            if (operationIndex != 0) { state.Record("probe.sequence.index", operationIndex); }
            state.Mark("probe.setup.begin");
            destination = Path.Combine(runRoot, "destination");
            Directory.CreateDirectory(destination);
            sentinel = Path.Combine(destination, "sentinel.bin");
            using (var file = new FileStream(sentinel, FileMode.CreateNew, FileAccess.Write, FileShare.None))
            {
                file.Write(bytes);
            }
            token.ThrowIfCancellationRequested();
            state.Mark("probe.construct.begin");
            state.ConstructionAttempted = true;
            publication = runtime.Construct(destination);
            runtime.SetJob(publication, state.Job);
            state.Mark("probe.initialize.begin");
            runtime.Initialize(publication);
            initialized = true;
            state.Mark("probe.initialize.end");
            token.ThrowIfCancellationRequested();

            if (state.Name != "cleanup.empty")
            {
                state.Mark("probe.child.create.begin");
                var entry = new ArchiveEntry("probe", "probe.bin", EntryKind.File, bytes.Length);
                var handle = runtime.CreateEntry(publication, entry, token);
                state.Mark("probe.child.create.end");
                // 반환 핸들은 Runtime 소유이며 시험에서 닫거나 복제하지 않음.
                state.Mark("probe.write.begin");
                if (state.Name == "cleanup.sync") { RandomAccess.Write(handle, bytes, 0); }
                else { await RandomAccess.WriteAsync(handle, bytes.AsMemory(), 0, token).ConfigureAwait(false); }
                state.Mark("probe.write.end");
                token.ThrowIfCancellationRequested();
                state.Mark("probe.flush.begin");
                RandomAccess.FlushToDisk(handle);
                state.Mark("probe.flush.end");
                Require(RandomAccess.GetLength(handle) == bytes.Length);
                var actual = new byte[bytes.Length];
                state.Mark("probe.read.begin");
                for (var offset = 0; offset < actual.Length;)
                {
                    token.ThrowIfCancellationRequested();
                    var read = state.Name == "cleanup.sync"
                        ? RandomAccess.Read(handle, actual.AsSpan(offset), offset)
                        : await RandomAccess.ReadAsync(handle, actual.AsMemory(offset), offset, token).ConfigureAwait(false);
                    Require(read > 0);
                    offset += read;
                }
                Require(bytes.SequenceEqual(actual));
                state.Mark("probe.read.end");
            }
            token.ThrowIfCancellationRequested();
        }
        catch (ArchiveException failure) when (!initialized &&
            failure.Code is ArchiveError.OutputUnavailable or ArchiveError.UnsupportedOperation or ArchiveError.UnsafePath)
        {
            result = 3;
            Console.Error.WriteLine("BLOCKED: " + failure.Code);
        }
        catch (Exception failure)
        {
            result = state.DiagnosticStarted ? 1 : 2;
            Console.Error.WriteLine("FAIL: " + failure.GetType().Name);
        }
        finally
        {
            if (publication is not null)
            {
                try
                {
                    state.Mark("probe.abort.begin");
                    state.AbortSucceeded = runtime.Abort(publication);
                    state.AbortReturned = true;
                    state.Mark("probe.abort.end");
                }
                catch (Exception failure)
                {
                    result = 1;
                    Console.Error.WriteLine("ABORT_FAIL: " + failure.GetType().Name);
                }
                finally
                {
                    if (state.AbortReturned)
                    {
                        try
                        {
                            state.Mark("probe.dispose.begin");
                            ((IDisposable)publication).Dispose();
                            state.DisposeReturned = true;
                            state.Mark("probe.dispose.end");
                        }
                        catch (Exception failure)
                        {
                            result = 1;
                            Console.Error.WriteLine("DISPOSE_FAIL: " + failure.GetType().Name);
                        }
                    }
                }
            }
        }
        if (publication is not null && state.AbortSucceeded && state.DisposeReturned)
        {
            try
            {
                state.Mark("probe.cleanup.verify.begin");
                Require(destination is not null && sentinel is not null);
                var entries = Directory.GetFileSystemEntries(destination!);
                Require(entries.Length == 1 && Path.GetFileName(entries[0]) == "sentinel.bin");
                Require(File.ReadAllBytes(sentinel!).SequenceEqual(bytes));
                state.ResidueChecked = true;
                state.CleanupConfirmed = true;
                state.Mark("probe.cleanup.verify.end");
            }
            catch (Exception failure)
            {
                result = 1;
                Console.Error.WriteLine("CLEANUP_VERIFY_FAIL: " + failure.GetType().Name);
            }
        }
        if (!state.ConstructionAttempted) { state.CleanupConfirmed = true; }
        if (token.IsCancellationRequested && result == 0) { result = 1; }
        return result;
    }

    private static async Task<int> ObserveSequenceAsync(RuntimeAccess runtime, string parent)
    {
        var sequence = new SequenceState();
        var cancellation = new CancellationTokenSource();
        cancellation.CancelAfter(TimeSpan.FromSeconds(15));
        var pending = Task.Run(() => RunSequenceAsync(runtime, sequence, parent, cancellation.Token));
        var result = 1;
        var timedOut = false;
        try { result = await pending.WaitAsync(TimeSpan.FromSeconds(45)); }
        catch (TimeoutException)
        {
            timedOut = true;
            Console.Error.WriteLine("FAIL: 연속 사례 전체 45초 제한 초과");
        }
        catch (Exception failure) { Console.Error.WriteLine("FAIL: " + failure.GetType().Name); }

        // 전체 예산은 한 번만 적용하며 현재 작업과 취소 콜백만 관찰함.
        var stop = Task.WhenAll(pending, cancellation.CancelAsync());
        try { await stop.WaitAsync(TimeSpan.FromSeconds(25)); }
        catch (Exception failure) { Console.Error.WriteLine("STOP_OBSERVATION: " + failure.GetType().Name); }
        var settled = stop.IsCompleted;
        var last = sequence.Last;
        var cleanupConfirmed = last is null ||
            (last.CleanupConfirmed && last.CancellationSettled && last.CancellationDisposed);
        if (timedOut || !settled || !cleanupConfirmed) { result = 124; }
        else if (!stop.IsCompletedSuccessfully && result == 0) { result = 1; }
        if (settled) { cancellation.Dispose(); }
        last?.Record("probe.sequence.observer.end", result);

        var logStopped = false;
        try
        {
            await StreamingExtractionDiagnostics.StopAsync().WaitAsync(TimeSpan.FromSeconds(1));
            logStopped = true;
        }
        catch (Exception failure) { Console.Error.WriteLine("DIAGNOSTICS_STOP_UNCONFIRMED: " + failure.GetType().Name); }
        var status = StreamingExtractionDiagnostics.Status;
        var dropped = StreamingExtractionDiagnostics.Dropped;
        var complete = sequence.First?.DiagnosticStarted == true && logStopped &&
            status == 2 && dropped == 0 && result != 124;
        Console.WriteLine($"PROBE case=cleanup.sequence24, hostPid={Environment.ProcessId}, planned=24, " +
            $"started={sequence.Started}, completed={sequence.Completed}, lastType={last?.Name ?? "none"}, " +
            $"job={last?.Job.ToString("N") ?? "none"}, caseExit={result}, taskSettled={settled}, " +
            $"cleanupConfirmed={cleanupConfirmed}, abortReturned={last?.AbortReturned == true}, " +
            $"abortSucceeded={last?.AbortSucceeded == true}, disposeReturned={last?.DisposeReturned == true}, " +
            $"residueChecked={last?.ResidueChecked == true}, cancellationSettled={last?.CancellationSettled == true}, " +
            $"ctsDisposed={last?.CancellationDisposed == true}, stage={last?.Stage ?? "probe.not_started"}");
        Console.WriteLine($"DIAGNOSTICS stopped={logStopped}, status={status}, dropped={dropped}, logComplete={complete}");
        if (result == 124)
        {
            Console.Error.WriteLine("CLEANUP_UNCONFIRMED: 미정리 가능, 다음 실행 전 중앙의 프로세스 종료와 잔류 항목 확인 필요");
        }
        if (!complete) { Console.Error.WriteLine("DIAGNOSTICS_INCOMPLETE"); }
        return result == 0 && !complete ? 2 : result;
    }

    private static async Task<int> RunSequenceAsync(RuntimeAccess runtime, SequenceState sequence,
        string parent, CancellationToken token)
    {
        string[] order = ["cleanup.empty", "cleanup.sync", "cleanup.async"];
        for (var index = 0; index < 24; index++)
        {
            if (token.IsCancellationRequested) { return 1; }
            var state = new ProbeState(runtime, order[index % order.Length]);
            state.DiagnosticStarted = sequence.First?.DiagnosticStarted == true;
            sequence.First ??= state;
            sequence.Last = state;
            sequence.Started = index + 1;
            var cancellation = new CancellationTokenSource();
            CancellationTokenRegistration propagation = default;
            CancellationTokenRegistration registration = default;
            var result = 1;
            try
            {
                propagation = token.Register(() => cancellation.Cancel());
                registration = cancellation.Token.Register(() => state.Record("probe.token.cancelled"));
                var pending = Task.Run(() => RunAsync(runtime, state, parent, cancellation.Token,
                    startDiagnostics: index == 0, operationIndex: index + 1));
                result = await pending.ConfigureAwait(false);
            }
            catch (Exception failure) { Console.Error.WriteLine("OPERATION_FAIL: " + failure.GetType().Name); }
            finally
            {
                try
                {
                    state.Mark("probe.cancellation.drain.begin");
                    // 부모 전파와 작업 콜백이 모두 끝난 뒤 CTS를 정리함.
                    await propagation.DisposeAsync().ConfigureAwait(false);
                    await cancellation.CancelAsync().ConfigureAwait(false);
                    await registration.DisposeAsync().ConfigureAwait(false);
                    state.CancellationSettled = true;
                    state.Mark("probe.cancellation.drain.end");
                    cancellation.Dispose();
                    state.CancellationDisposed = true;
                    state.Mark("probe.cts.dispose.end");
                }
                catch (Exception failure)
                {
                    result = 1;
                    Console.Error.WriteLine("CANCELLATION_UNCONFIRMED: " + failure.GetType().Name);
                }
            }

            if (!state.CleanupConfirmed || !state.CancellationSettled || !state.CancellationDisposed)
            {
                result = 124;
            }
            if (result == 0)
            {
                Require(state.AbortSucceeded && state.DisposeReturned && state.ResidueChecked);
                sequence.Completed++;
                state.Record("probe.sequence.completed", sequence.Completed);
                if (StreamingExtractionDiagnostics.Status is 3 or 4 || StreamingExtractionDiagnostics.Dropped != 0)
                {
                    result = 2;
                }
            }
            Console.WriteLine($"PROBE_OPERATION index={index + 1}, cycle={index / 3 + 1}, type={state.Name}, " +
                $"job={state.Job:N}, operationExit={result}, completed={sequence.Completed}, " +
                $"cleanupConfirmed={state.CleanupConfirmed}, residueChecked={state.ResidueChecked}, " +
                $"cancellationSettled={state.CancellationSettled}, ctsDisposed={state.CancellationDisposed}");
            if (result != 0) { return result; }
            if (token.IsCancellationRequested) { return 1; }
        }
        return 0;
    }

    private sealed class SequenceState
    {
        internal volatile ProbeState? First;
        internal volatile ProbeState? Last;
        internal volatile int Started;
        internal volatile int Completed;
    }

    private static void RequireParent(string parent)
    {
        if (!Directory.Exists(parent)) { throw new DirectoryNotFoundException(); }
        for (DirectoryInfo? current = new(Path.GetFullPath(parent)); current is not null; current = current.Parent)
        {
            if ((current.Attributes & FileAttributes.ReparsePoint) != 0)
            {
                throw new IOException("시험 부모 링크 거절");
            }
        }
    }

    private static void Require(bool condition)
    {
        if (!condition) { throw new InvalidOperationException("검사 불일치"); }
    }

    private sealed class ProbeState(RuntimeAccess runtime, string name)
    {
        internal readonly string Name = name;
        internal readonly Guid Job = Guid.NewGuid();
        internal volatile string Stage = "probe.not_started";
        internal volatile bool DiagnosticStarted;
        internal volatile bool ConstructionAttempted;
        internal volatile bool CleanupConfirmed;
        internal volatile bool AbortReturned;
        internal volatile bool AbortSucceeded;
        internal volatile bool DisposeReturned;
        internal volatile bool ResidueChecked;
        internal volatile bool CancellationSettled;
        internal volatile bool CancellationDisposed;

        internal void Record(string stage, long value = 0) => runtime.Record(Job, stage, value, 0, "");
        internal void Mark(string stage)
        {
            Stage = stage;
            Record(stage);
        }
    }

    private sealed class RuntimeAccess
    {
        private const BindingFlags Instance = BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic;
        private readonly ConstructorInfo constructor;
        private readonly MethodInfo initialize;
        private readonly MethodInfo createEntry;
        private readonly MethodInfo abort;
        private readonly PropertyInfo job;
        internal readonly Action<Guid, string, long, ulong, string> Record;

        internal RuntimeAccess()
        {
            var assembly = typeof(NewFolderPublisher).Assembly;
            var type = assembly.GetType("Zipelo.Runtime.WindowsFolderPublication", throwOnError: true)!;
            var pending = assembly.GetType("Zipelo.Runtime.PendingFolderPublication", throwOnError: true)!;
            constructor = type.GetConstructor(Instance, null, [typeof(string)], null)
                ?? throw new MissingMethodException(type.FullName, ".ctor(string)");
            initialize = Method(type, "Initialize");
            createEntry = Method(pending, "CreateStreamingEntry");
            abort = Method(type, "Abort");
            job = type.GetProperty("DiagnosticJobId", Instance)
                ?? throw new MissingMemberException(type.FullName, "DiagnosticJobId");
            var record = typeof(StreamingExtractionDiagnostics).GetMethod("Record", BindingFlags.Static | BindingFlags.NonPublic)
                ?? throw new MissingMethodException("StreamingExtractionDiagnostics.Record");
            Record = record.CreateDelegate<Action<Guid, string, long, ulong, string>>();
        }

        internal object Construct(string parent)
        {
            try { return constructor.Invoke([parent]); }
            catch (TargetInvocationException failure) when (failure.InnerException is not null)
            {
                ExceptionDispatchInfo.Capture(failure.InnerException).Throw();
                throw;
            }
        }

        internal void SetJob(object pending, Guid id) => job.SetValue(pending, id);
        internal void Initialize(object pending) => Invoke(initialize, pending);
        internal bool Abort(object pending) => Invoke(abort, pending) is true;
        internal SafeFileHandle CreateEntry(object pending, ArchiveEntry entry, CancellationToken token)
            => Invoke(createEntry, pending, entry, token) as SafeFileHandle
                ?? throw new InvalidOperationException("파일 핸들 없음");

        private static MethodInfo Method(Type type, string name) =>
            type.GetMethod(name, Instance) ?? throw new MissingMethodException(type.FullName, name);

        private static object? Invoke(MethodInfo method, object pending, params object[] arguments)
        {
            try { return method.Invoke(pending, arguments); }
            catch (TargetInvocationException failure) when (failure.InnerException is not null)
            {
                ExceptionDispatchInfo.Capture(failure.InnerException).Throw();
                throw;
            }
        }
    }
#else
    public static int Main(string[] args)
    {
        Console.Error.WriteLine("진단 빌드에서만 실행할 수 있음: ZIPELO_STREAM_DIAGNOSTICS");
        return 2;
    }
#endif
}

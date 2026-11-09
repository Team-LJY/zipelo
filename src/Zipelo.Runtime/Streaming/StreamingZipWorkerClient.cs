using System.Buffers.Binary;
using System.Diagnostics;
using System.Security.Cryptography;
using Microsoft.Win32.SafeHandles;
using Zipelo.Core;

namespace Zipelo.Runtime.Streaming;

public sealed record StreamingExtractionResult(
    Guid JobId,
    ZipPublicationResult Publication,
    bool WorkerExitConfirmed,
    int? WorkerExitCode,
    bool InputCleanupFailed,
    string? InputTemporaryName,
    int VerifiedEntries,
    BudgetUsage Usage)
{
    public PublicationNativeFailure? InputCleanupNativeFailure { get; init; }
    public int? InputCleanupPosixError { get; init; }
}

// 명시적으로 호출하는 별도 해제 경로임. 일반 capability를 변경하지 않음.
public sealed class StreamingZipWorkerClient
{
    private readonly string workerPath;
    private readonly string? dotnetHostPath;

    public StreamingZipWorkerClient(string workerPath, string? dotnetHostPath = null)
    {
        if (!Path.IsPathFullyQualified(workerPath) ||
            (dotnetHostPath is not null && !Path.IsPathFullyQualified(dotnetHostPath)))
        {
            throw new ArgumentException("신뢰하는 작업자와 호스트의 절대 경로가 필요함.");
        }
        this.workerPath = workerPath;
        this.dotnetHostPath = dotnetHostPath;
    }

    public async Task<StreamingExtractionResult> ExtractToNewFolderAsync(
        Guid jobId, Stream source, string destinationPath, JobLimits limits, CancellationToken token)
    {
        Process? process = null;
        Task? ranges = null;
        PendingFolderPublication? input = null;
        PendingFolderPublication? output = null;
        JobBudget? budget = null;
        var started = false;
        var exited = true;
        var ioStopped = true;
        var published = false;
        var inputCleanupAttempted = false;
        var inputCleanupFailed = false;
        var outputCleanupFailed = false;
        var verifiedEntries = 0;
        int? exitCode = null;
        int? posixError = null;
        PublicationNativeFailure? nativeFailure = null;
        var error = ArchiveError.None;
        using var deadline = CancellationTokenSource.CreateLinkedTokenSource(token);
#if ZIPELO_STREAM_DIAGNOSTICS
        var observation = deadline.Token.UnsafeRegister(_ =>
            StreamingExtractionDiagnostics.Record(jobId, "operation.cancelled", token.IsCancellationRequested ? 1 : 0), null);
#endif
        StreamingExtractionDiagnostics.Record(jobId, "operation.start");
        try
        {
            var effective = ExtractionWire.Limit(limits);
            deadline.CancelAfter(effective.MaxDuration);
            StreamingExtractionDiagnostics.Record(jobId, "deadline.scheduled_ms", (long)effective.MaxDuration.TotalMilliseconds);
            var operationToken = deadline.Token;
            budget = new JobBudget(effective);
            budget.Check(operationToken);
            if (jobId == Guid.Empty || source is null || !source.CanRead || !source.CanSeek || source.CanWrite)
            {
                throw new ArchiveException(ArchiveError.UnsupportedOperation);
            }
            if (!NewFolderPublisher.IsSupportedPlatform) { throw new ArchiveException(ArchiveError.OutputUnavailable); }
            var (parent, leaf) = Destination(destinationPath);
            StreamingExtractionDiagnostics.Record(jobId, "input.create.begin");
            input = Pending(parent);
            input.Initialize();
            StreamingExtractionDiagnostics.Record(jobId, "input.create.end");
            var snapshot = input.CreateStreamingEntry(new("snapshot", "input.zip", EntryKind.File, 0), operationToken)!;
            StreamingExtractionDiagnostics.Record(jobId, "snapshot.begin");
            var length = await SnapshotAsync(source, snapshot, budget, operationToken, jobId).ConfigureAwait(false);
            StreamingExtractionDiagnostics.Record(jobId, "snapshot.end");
            using var readSnapshot = new HandleReadStream(snapshot, length);
            StreamingExtractionDiagnostics.Record(jobId, "index.begin");
            var items = ZipStreamIndex.Read(readSnapshot, budget, operationToken);
            StreamingExtractionDiagnostics.Record(jobId, "index.end", items.Length);
            if (items.Sum(item => item.Entry.DeclaredSize!.Value) > effective.MaxOutputBytes)
            {
                throw new ArchiveException(ArchiveError.BudgetExceeded);
            }
            StreamingExtractionDiagnostics.Record(jobId, "output.create.begin");
            output = Pending(parent);
            output.Initialize();
            StreamingExtractionDiagnostics.Record(jobId, "output.create.end");
            var dll = workerPath.EndsWith(".dll", StringComparison.OrdinalIgnoreCase);
            if (!File.Exists(workerPath) || (dll && (dotnetHostPath is null || !File.Exists(dotnetHostPath))))
            {
                throw new ArchiveException(ArchiveError.EngineUnavailable);
            }
            var start = new ProcessStartInfo
            {
                FileName = dll ? dotnetHostPath! : workerPath,
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardInput = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true
            };
            if (dll) { start.ArgumentList.Add(workerPath); }
            start.ArgumentList.Add(ExtractionWire.Mode);
            process = new Process { StartInfo = start };
            StreamingExtractionDiagnostics.Record(jobId, "process.start.begin");
            if (!process.Start()) { throw new ArchiveException(ArchiveError.EngineUnavailable); }
            started = true;
            StreamingExtractionDiagnostics.ProcessStarted(jobId, process.Id, start.FileName, workerPath);
            exited = false;

            StreamingExtractionDiagnostics.Record(jobId, "config.write.begin");
            await process.StandardInput.BaseStream.WriteAsync(ExtractionWire.Config(jobId, length, effective),
                operationToken).ConfigureAwait(false);
            await process.StandardInput.BaseStream.FlushAsync(operationToken).ConfigureAwait(false);
            StreamingExtractionDiagnostics.Record(jobId, "config.write.end");
            async Task Serve()
            {
                try
                {
                    await ExtractionWire.ServeRangesAsync(process.StandardError.BaseStream,
                        process.StandardInput.BaseStream, snapshot, length, jobId, budget, operationToken).ConfigureAwait(false);
                }
                catch
                {
                    StreamingExtractionDiagnostics.Record(jobId, "range.fault");
                    deadline.Cancel();
                    throw;
                }
            }
            StreamingExtractionDiagnostics.Record(jobId, "range.service.begin");
            ranges = Serve();
            using (var reader = new WorkerStreamChannel(process.StandardOutput.BaseStream,
                WorkerStreamDirection.Read, jobId, ExtractionWire.ChannelLimits(effective)))
            {
                var begin = await RequiredAsync(reader, operationToken, jobId).ConfigureAwait(false);
                Require(begin.Header.Kind == WorkerStreamFrameKind.BeginJob);
                foreach (var item in items)
                {
                    var frame = await RequiredAsync(reader, operationToken, jobId).ConfigureAwait(false);
                    Require(frame.Header.Kind == WorkerStreamFrameKind.BeginItem && frame.Header.ItemId == item.Id &&
                        BinaryPrimitives.ReadUInt64LittleEndian(frame.Payload.Span) == (ulong)item.Entry.DeclaredSize!.Value);
                    StreamingExtractionDiagnostics.Record(jobId, "entry.create.begin", verifiedEntries);
                    var file = output.CreateStreamingEntry(item.Entry, operationToken);
                    StreamingExtractionDiagnostics.Record(jobId, "entry.create.end", verifiedEntries);
                    long actual = 0;
                    var crc = uint.MaxValue;
                    while (true)
                    {
                        frame = await RequiredAsync(reader, operationToken, jobId).ConfigureAwait(false);
                        if (frame.Header.Kind == WorkerStreamFrameKind.EndItem) { break; }
                        Require(frame.Header.Kind == WorkerStreamFrameKind.Data && file is not null);
                        var count = frame.Payload.Length;
                        if (count > item.Entry.DeclaredSize!.Value - actual)
                        {
                            throw new ArchiveException(ArchiveError.IntegrityFailure);
                        }
                        budget.ChargeProcessed(count, operationToken);
                        budget.ReserveOutput(count, operationToken);
                        crc = ZipExtraction.UpdateCrc(crc, frame.Payload.Span);
                        StreamingExtractionDiagnostics.Record(jobId, "output.write.begin", count);
                        await RandomAccess.WriteAsync(file!, frame.Payload, actual, operationToken).ConfigureAwait(false);
                        StreamingExtractionDiagnostics.Record(jobId, "output.write.end", count);
                        actual += count;
                    }
                    if (actual != item.Entry.DeclaredSize || ~crc != item.Crc)
                    {
                        throw new ArchiveException(ArchiveError.IntegrityFailure);
                    }
                    if (file is not null)
                    {
                        await PendingFolderPublication.VerifyStreamingFileAsync(file, actual, item.Crc, budget, operationToken, jobId)
                            .ConfigureAwait(false);
                    }
                    verifiedEntries++;
                }
                StreamingExtractionDiagnostics.Record(jobId, "endjob.read.begin");
                var end = await RequiredAsync(reader, operationToken, jobId).ConfigureAwait(false);
                Require(end.Header.Kind == WorkerStreamFrameKind.EndJob &&
                    reader.TerminalStatus == WorkerStreamTerminalStatus.Completed);
                StreamingExtractionDiagnostics.Record(jobId, "endjob.validated");
                StreamingExtractionDiagnostics.Record(jobId, "stdout.eof.begin");
                Require(await reader.ReadAsync(operationToken).ConfigureAwait(false) is null &&
                    reader.Phase == WorkerStreamPhase.InputCompleted);
                StreamingExtractionDiagnostics.Record(jobId, "stdout.eof.end");
            }
            StreamingExtractionDiagnostics.Record(jobId, "process.exit.begin");
            await process.WaitForExitAsync(operationToken).ConfigureAwait(false);
            exited = true;
            exitCode = process.ExitCode;
            StreamingExtractionDiagnostics.Record(jobId, "process.exit.end", exitCode.Value);
            StreamingExtractionDiagnostics.Record(jobId, "range.await.begin");
            await ranges.ConfigureAwait(false);
            StreamingExtractionDiagnostics.Record(jobId, "range.await.end");
            Require(exitCode == 0);
            budget.Check(operationToken);

            inputCleanupAttempted = true;
            try
            {
                StreamingExtractionDiagnostics.Record(jobId, "input.cleanup.begin");
                inputCleanupFailed = !input.Abort();
                StreamingExtractionDiagnostics.Record(jobId, "input.cleanup.end", inputCleanupFailed ? 1 : 0);
            }
            catch { inputCleanupFailed = true; throw; }
            if (inputCleanupFailed) { throw new ArchiveException(ArchiveError.OutputUnavailable); }
            StreamingExtractionDiagnostics.Record(jobId, "output.verify.begin");
            output.Verify(operationToken);
            StreamingExtractionDiagnostics.Record(jobId, "output.verify.end");
            budget.Check(operationToken);
            StreamingExtractionDiagnostics.Record(jobId, "output.publish.begin");
            output.Publish(leaf, operationToken);
            StreamingExtractionDiagnostics.Record(jobId, "output.publish.end");
            published = true;
        }
        catch (OperationCanceledException)
        {
            error = ranges?.Exception?.InnerException is Exception failure ? Error(failure) :
                token.IsCancellationRequested ? ArchiveError.Cancelled : ArchiveError.BudgetExceeded;
        }
        catch (PublicationNativeException failure) { error = failure.Code; nativeFailure = failure.Failure; }
        catch (PosixPublicationException failure) { error = failure.Code; posixError = failure.NativeError; }
        catch (Exception failure) { error = Error(failure); }
        finally
        {
            StreamingExtractionDiagnostics.Record(jobId, "cleanup.begin");
            StreamingExtractionDiagnostics.Record(jobId, "cleanup.cancel.begin");
            deadline.Cancel();
            StreamingExtractionDiagnostics.Record(jobId, "cleanup.cancel.end");
            if (started && process is not null)
            {
                try
                {
                    StreamingExtractionDiagnostics.Record(jobId, "cleanup.kill.begin");
                    if (!process.HasExited) { process.Kill(entireProcessTree: true); }
                    StreamingExtractionDiagnostics.Record(jobId, "cleanup.kill.end");
                    StreamingExtractionDiagnostics.Record(jobId, "cleanup.exit.begin");
                    await process.WaitForExitAsync(CancellationToken.None).WaitAsync(TimeSpan.FromSeconds(10)).ConfigureAwait(false);
                    StreamingExtractionDiagnostics.Record(jobId, "cleanup.exit.end");
                    exited = true;
                    exitCode = process.ExitCode;
                }
                catch { exited = false; }
            }
            if (ranges is not null)
            {
                try
                {
                    StreamingExtractionDiagnostics.Record(jobId, "cleanup.range.begin");
                    await ranges.WaitAsync(TimeSpan.FromSeconds(10)).ConfigureAwait(false);
                    StreamingExtractionDiagnostics.Record(jobId, "cleanup.range.end");
                }
                catch { ioStopped = ranges.IsCompleted; }
            }
            if (!published && !token.IsCancellationRequested &&
                ranges?.Exception?.InnerException is Exception rangeFailure &&
                rangeFailure is not OperationCanceledException)
            {
                error = Error(rangeFailure);
            }
            if (!published && (!exited || !ioStopped)) { error = ArchiveError.EngineFailure; }
            if (!published && exitCode == 2) { error = ArchiveError.UnsupportedOperation; }
            if (input is not null && !inputCleanupAttempted)
            {
                if (exited && ioStopped)
                {
                    try
                    {
                        StreamingExtractionDiagnostics.Record(jobId, "input.abort.begin");
                        inputCleanupFailed = !input.Abort();
                        StreamingExtractionDiagnostics.Record(jobId, "input.abort.end", inputCleanupFailed ? 1 : 0);
                    }
                    catch { inputCleanupFailed = true; }
                }
                else { inputCleanupFailed = true; }
            }
            if (output is not null && !published)
            {
                if (exited && ioStopped)
                {
                    try
                    {
                        StreamingExtractionDiagnostics.Record(jobId, "output.abort.begin");
                        outputCleanupFailed = !output.Abort();
                        StreamingExtractionDiagnostics.Record(jobId, "output.abort.end", outputCleanupFailed ? 1 : 0);
                    }
                    catch { outputCleanupFailed = true; }
                }
                else { outputCleanupFailed = true; }
            }
            if (ioStopped)
            {
                StreamingExtractionDiagnostics.Record(jobId, "owned.dispose.begin");
                input?.Dispose();
                output?.Dispose();
                StreamingExtractionDiagnostics.Record(jobId, "owned.dispose.end");
            }
            StreamingExtractionDiagnostics.Record(jobId, "process.dispose.begin");
            process?.Dispose();
            StreamingExtractionDiagnostics.Record(jobId, "process.dispose.end");
            StreamingExtractionDiagnostics.Record(jobId, "cleanup.end", (long)error);
#if ZIPELO_STREAM_DIAGNOSTICS
            observation.Unregister();
#endif
        }
        return new(jobId, new(published, error, outputCleanupFailed,
            outputCleanupFailed ? output?.TemporaryName : null)
        {
            NativeFailure = nativeFailure,
            PosixError = posixError,
            CleanupNativeFailure = output?.CleanupNativeFailure,
            CleanupPosixError = (output as LinuxFolderPublication)?.CleanupError
        }, exited, exitCode, inputCleanupFailed, inputCleanupFailed ? input?.TemporaryName : null,
            verifiedEntries, budget?.Usage ?? new(0, 0, 0))
        {
            InputCleanupNativeFailure = input?.CleanupNativeFailure,
            InputCleanupPosixError = (input as LinuxFolderPublication)?.CleanupError
        };
    }

    private static async Task<long> SnapshotAsync(Stream source, SafeFileHandle snapshot,
        JobBudget budget, CancellationToken token, Guid jobId)
    {
        var start = source.Position;
        var originalLength = source.Length;
        var length = originalLength - start;
        if (start < 0 || length is < 22 or > WorkerProtocol.MaxDataBytes)
        {
            throw new ArchiveException(ArchiveError.BudgetExceeded);
        }
        var buffer = new byte[ExtractionWire.ChunkBytes];
        var check = new byte[ExtractionWire.ChunkBytes];
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        try
        {
            long offset = 0;
            while (offset < length)
            {
                var count = (int)Math.Min(buffer.Length, length - offset);
                budget.ChargeProcessed(count, token);
                await source.ReadExactlyAsync(buffer.AsMemory(0, count), token).ConfigureAwait(false);
                hash.AppendData(buffer, 0, count);
                await RandomAccess.WriteAsync(snapshot, buffer.AsMemory(0, count), offset, token).ConfigureAwait(false);
                offset += count;
            }
            var expectedHash = hash.GetHashAndReset();
            StreamingExtractionDiagnostics.Record(jobId, "snapshot.flush.begin");
            RandomAccess.FlushToDisk(snapshot);
            StreamingExtractionDiagnostics.Record(jobId, "snapshot.flush.end");
            if (RandomAccess.GetLength(snapshot) != length || source.Length != originalLength)
            {
                throw new ArchiveException(ArchiveError.IntegrityFailure);
            }
            source.Position = start;
            offset = 0;
            while (offset < length)
            {
                var count = (int)Math.Min(buffer.Length, length - offset);
                budget.ChargeProcessed(count * 2L, token);
                await source.ReadExactlyAsync(buffer.AsMemory(0, count), token).ConfigureAwait(false);
                var filled = 0;
                while (filled < count)
                {
                    var read = await RandomAccess.ReadAsync(snapshot, check.AsMemory(filled, count - filled),
                        offset + filled, token).ConfigureAwait(false);
                    if (read == 0) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
                    filled += read;
                }
                if (!buffer.AsSpan(0, count).SequenceEqual(check.AsSpan(0, count)))
                {
                    throw new ArchiveException(ArchiveError.IntegrityFailure);
                }
                hash.AppendData(check, 0, count);
                offset += count;
            }
            if (source.Length != originalLength ||
                !CryptographicOperations.FixedTimeEquals(expectedHash, hash.GetHashAndReset()))
            {
                throw new ArchiveException(ArchiveError.IntegrityFailure);
            }
            budget.Check(token);
            return length;
        }
        finally { source.Position = start; }
    }

    private static PendingFolderPublication Pending(string parent)
        => OperatingSystem.IsWindows() ? new WindowsFolderPublication(parent) : new LinuxFolderPublication(parent);

    private static (string Parent, string Leaf) Destination(string path)
    {
        if (!Path.IsPathFullyQualified(path)) { throw new ArchiveException(ArchiveError.UnsafePath); }
        var fullPath = Path.GetFullPath(path);
        if (OperatingSystem.IsLinux()) { _ = OutputPathPolicy.Validate(fullPath[1..], FileNamePolicy.Linux); }
        var parent = Path.GetDirectoryName(fullPath);
        var leaf = Path.GetFileName(fullPath);
        if (parent is null || OutputPathPolicy.Validate(leaf, OutputPathPolicy.CurrentPolicy).Count != 1)
        {
            throw new ArchiveException(ArchiveError.UnsafePath);
        }
        return (parent, leaf);
    }

    private static async ValueTask<WorkerStreamFrame> RequiredAsync(WorkerStreamChannel reader, CancellationToken token, Guid jobId)
    {
        StreamingExtractionDiagnostics.Record(jobId, "frame.read.begin");
        var frame = await reader.ReadAsync(token).ConfigureAwait(false) ?? throw new ArchiveException(ArchiveError.EngineFailure);
        StreamingExtractionDiagnostics.Record(jobId, "frame.received", (long)frame.Header.Kind, frame.Header.Sequence);
        return frame;
    }

    private static void Require(bool valid)
    {
        if (!valid) { throw new ArchiveException(ArchiveError.EngineFailure); }
    }

    private static ArchiveError Error(Exception failure) => failure switch
    {
        ArchiveException archive => archive.Code,
        WorkerStreamException { Error: WorkerStreamError.BudgetExceeded } => ArchiveError.BudgetExceeded,
        WorkerStreamException => ArchiveError.EngineFailure,
        EndOfStreamException or InvalidDataException or OverflowException or System.Text.DecoderFallbackException
            => ArchiveError.IntegrityFailure,
        ArgumentException => ArchiveError.UnsupportedOperation,
        _ => ArchiveError.EngineFailure
    };
}

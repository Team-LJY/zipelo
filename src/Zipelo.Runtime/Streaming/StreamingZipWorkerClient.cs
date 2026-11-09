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
        try
        {
            var effective = ExtractionWire.Limit(limits);
            deadline.CancelAfter(effective.MaxDuration);
            var operationToken = deadline.Token;
            budget = new JobBudget(effective);
            budget.Check(operationToken);
            if (jobId == Guid.Empty || source is null || !source.CanRead || !source.CanSeek || source.CanWrite)
            {
                throw new ArchiveException(ArchiveError.UnsupportedOperation);
            }
            if (!NewFolderPublisher.IsSupportedPlatform) { throw new ArchiveException(ArchiveError.OutputUnavailable); }
            var (parent, leaf) = Destination(destinationPath);
            input = Pending(parent);
            input.Initialize();
            var snapshot = input.CreateStreamingEntry(new("snapshot", "input.zip", EntryKind.File, 0), operationToken)!;
            var length = await SnapshotAsync(source, snapshot, budget, operationToken).ConfigureAwait(false);
            using var readSnapshot = new HandleReadStream(snapshot, length);
            var items = ZipStreamIndex.Read(readSnapshot, budget, operationToken);
            if (items.Sum(item => item.Entry.DeclaredSize!.Value) > effective.MaxOutputBytes)
            {
                throw new ArchiveException(ArchiveError.BudgetExceeded);
            }
            output = Pending(parent);
            output.Initialize();
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
            if (!process.Start()) { throw new ArchiveException(ArchiveError.EngineUnavailable); }
            started = true;
            exited = false;

            await process.StandardInput.BaseStream.WriteAsync(ExtractionWire.Config(jobId, length, effective),
                operationToken).ConfigureAwait(false);
            await process.StandardInput.BaseStream.FlushAsync(operationToken).ConfigureAwait(false);
            async Task Serve()
            {
                try
                {
                    await ExtractionWire.ServeRangesAsync(process.StandardError.BaseStream,
                        process.StandardInput.BaseStream, snapshot, length, jobId, budget, operationToken).ConfigureAwait(false);
                }
                catch
                {
                    deadline.Cancel();
                    throw;
                }
            }
            ranges = Serve();
            using (var reader = new WorkerStreamChannel(process.StandardOutput.BaseStream,
                WorkerStreamDirection.Read, jobId, ExtractionWire.ChannelLimits(effective)))
            {
                var begin = await RequiredAsync(reader, operationToken).ConfigureAwait(false);
                Require(begin.Header.Kind == WorkerStreamFrameKind.BeginJob);
                foreach (var item in items)
                {
                    var frame = await RequiredAsync(reader, operationToken).ConfigureAwait(false);
                    Require(frame.Header.Kind == WorkerStreamFrameKind.BeginItem && frame.Header.ItemId == item.Id &&
                        BinaryPrimitives.ReadUInt64LittleEndian(frame.Payload.Span) == (ulong)item.Entry.DeclaredSize!.Value);
                    var file = output.CreateStreamingEntry(item.Entry, operationToken);
                    long actual = 0;
                    var crc = uint.MaxValue;
                    while (true)
                    {
                        frame = await RequiredAsync(reader, operationToken).ConfigureAwait(false);
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
                        await RandomAccess.WriteAsync(file!, frame.Payload, actual, operationToken).ConfigureAwait(false);
                        actual += count;
                    }
                    if (actual != item.Entry.DeclaredSize || ~crc != item.Crc)
                    {
                        throw new ArchiveException(ArchiveError.IntegrityFailure);
                    }
                    if (file is not null)
                    {
                        await PendingFolderPublication.VerifyStreamingFileAsync(file, actual, item.Crc, budget, operationToken)
                            .ConfigureAwait(false);
                    }
                    verifiedEntries++;
                }
                var end = await RequiredAsync(reader, operationToken).ConfigureAwait(false);
                Require(end.Header.Kind == WorkerStreamFrameKind.EndJob &&
                    reader.TerminalStatus == WorkerStreamTerminalStatus.Completed);
                Require(await reader.ReadAsync(operationToken).ConfigureAwait(false) is null &&
                    reader.Phase == WorkerStreamPhase.InputCompleted);
            }
            await process.WaitForExitAsync(operationToken).ConfigureAwait(false);
            exited = true;
            exitCode = process.ExitCode;
            await ranges.ConfigureAwait(false);
            Require(exitCode == 0);
            budget.Check(operationToken);

            inputCleanupAttempted = true;
            try { inputCleanupFailed = !input.Abort(); }
            catch { inputCleanupFailed = true; throw; }
            if (inputCleanupFailed) { throw new ArchiveException(ArchiveError.OutputUnavailable); }
            output.Verify(operationToken);
            budget.Check(operationToken);
            output.Publish(leaf, operationToken);
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
            deadline.Cancel();
            if (started && process is not null)
            {
                try
                {
                    if (!process.HasExited) { process.Kill(entireProcessTree: true); }
                    await process.WaitForExitAsync(CancellationToken.None).WaitAsync(TimeSpan.FromSeconds(10)).ConfigureAwait(false);
                    exited = true;
                    exitCode = process.ExitCode;
                }
                catch { exited = false; }
            }
            if (ranges is not null)
            {
                try { await ranges.WaitAsync(TimeSpan.FromSeconds(10)).ConfigureAwait(false); }
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
                    try { inputCleanupFailed = !input.Abort(); }
                    catch { inputCleanupFailed = true; }
                }
                else { inputCleanupFailed = true; }
            }
            if (output is not null && !published)
            {
                if (exited && ioStopped)
                {
                    try { outputCleanupFailed = !output.Abort(); }
                    catch { outputCleanupFailed = true; }
                }
                else { outputCleanupFailed = true; }
            }
            if (ioStopped)
            {
                input?.Dispose();
                output?.Dispose();
            }
            process?.Dispose();
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
        JobBudget budget, CancellationToken token)
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
            RandomAccess.FlushToDisk(snapshot);
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

    private static async ValueTask<WorkerStreamFrame> RequiredAsync(WorkerStreamChannel reader, CancellationToken token)
        => await reader.ReadAsync(token).ConfigureAwait(false) ?? throw new ArchiveException(ArchiveError.EngineFailure);

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

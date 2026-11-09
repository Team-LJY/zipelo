using System.IO.Compression;
using Zipelo.Core;
using Zipelo.Runtime;
using Zipelo.Runtime.Streaming;

namespace Zipelo.Worker;

internal static class StreamingZipExtractor
{
    internal static async Task<int> RunAsync()
    {
        AppContext.SetSwitch("System.IO.Compression.UseStrictValidation", true);
        try
        {
            var input = Console.OpenStandardInput();
            var config = ExtractionWire.ReadConfig(input);
            using var deadline = new CancellationTokenSource(config.Limits.MaxDuration);
            var token = deadline.Token;
            var budget = new JobBudget(config.Limits);
            using var snapshot = new RemoteSnapshotStream(input, Console.OpenStandardError(),
                config.Id, config.Length, budget, token);
            using var output = new WorkerStreamChannel(Console.OpenStandardOutput(), WorkerStreamDirection.Write,
                config.Id, ExtractionWire.ChannelLimits(config.Limits));
            ulong sequence = 0;
            async ValueTask Send(WorkerStreamFrameKind kind, Guid item, ReadOnlyMemory<byte> bytes)
            {
                await output.WriteAsync(new(kind, config.Id, item, sequence++, bytes.Length), bytes, token)
                    .ConfigureAwait(false);
            }

            await Send(WorkerStreamFrameKind.BeginJob, Guid.Empty, ReadOnlyMemory<byte>.Empty);
            var items = ZipStreamIndex.Read(snapshot, budget, token);
            var buffer = new byte[ExtractionWire.ChunkBytes];
            foreach (var item in items)
            {
                budget.Check(token);
                await Send(WorkerStreamFrameKind.BeginItem, item.Id, ExtractionWire.Length(item.Entry.DeclaredSize!.Value));
                using var packed = new PackedEntryStream(snapshot, item.Start, item.Packed);
                using Stream decoded = item.Method == 0 ? packed : new DeflateStream(packed, CompressionMode.Decompress);
                long actual = 0;
                var crc = uint.MaxValue;
                int count;
                while ((count = decoded.Read(buffer, 0, buffer.Length)) != 0)
                {
                    budget.ChargeProcessed(count, token);
                    budget.ReserveOutput(count, token);
                    if (count > item.Entry.DeclaredSize!.Value - actual)
                    {
                        throw new ArchiveException(ArchiveError.IntegrityFailure);
                    }
                    actual += count;
                    crc = ZipExtraction.UpdateCrc(crc, buffer.AsSpan(0, count));
                    await Send(WorkerStreamFrameKind.Data, item.Id, buffer.AsMemory(0, count));
                }
                if (actual != item.Entry.DeclaredSize || ~crc != item.Crc)
                {
                    throw new ArchiveException(ArchiveError.IntegrityFailure);
                }
                await Send(WorkerStreamFrameKind.EndItem, item.Id, ExtractionWire.Length(actual));
            }
            budget.Check(token);
            await Send(WorkerStreamFrameKind.EndJob, Guid.Empty, new byte[] { (byte)WorkerStreamTerminalStatus.Completed });
            return 0;
        }
        catch
        {
            // stderr는 범위 요청 전용임. 실패 시 성공 종료 프레임 없이 비정상 종료함.
            return 1;
        }
    }
}

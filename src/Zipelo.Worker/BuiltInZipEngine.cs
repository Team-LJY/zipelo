using System.IO.Compression;
using Zipelo.Core;
using Zipelo.Runtime;

namespace Zipelo.Worker;

// 압축 파싱은 작업자에서만 수행함. UI에서 이 구현을 직접 참조하지 않음.
internal sealed class BuiltInZipEngine(string[] names, long outputLimit) : IArchiveEngine
{
    public IReadOnlyCollection<EngineCapability> Capabilities { get; } = Array.AsReadOnly(new[]
    {
        new EngineCapability("zip", ArchiveAction.List),
        new EngineCapability("zip", ArchiveAction.Create)
    });

    public async Task ExecuteAsync(ArchiveAction action, IReadOnlyList<Stream> inputs,
        EngineContext context, CancellationToken token)
    {
        try
        {
            if (action == ArchiveAction.List)
            {
                if (inputs.Count != 1) { throw new ArchiveException(ArchiveError.UnsupportedOperation); }
                context.Budget.ChargeProcessed(inputs[0].Length, token);
                using var archive = new ZipArchive(inputs[0], ZipArchiveMode.Read, leaveOpen: true);
                if (archive.Entries.Count > WorkerProtocol.MaxEntries)
                {
                    throw new ArchiveException(ArchiveError.BudgetExceeded);
                }
                for (var index = 0; index < archive.Entries.Count; index++)
                {
                    context.Budget.Check(token);
                    var entry = archive.Entries[index];
                    if (entry.FullName.Length > WorkerProtocol.MaxNameLength)
                    {
                        throw new ArchiveException(ArchiveError.BudgetExceeded);
                    }
                    if (entry.IsEncrypted) { throw new ArchiveException(ArchiveError.UnsupportedOperation); }
                    var unixType = (entry.ExternalAttributes >> 16) & 0xF000;
                    var kind = unixType == 0xA000 ? EntryKind.SymbolicLink :
                        entry.FullName.EndsWith('/') || unixType == 0x4000 ? EntryKind.Directory :
                        unixType is 0 or 0x8000 ? EntryKind.File : EntryKind.Other;
                    // 목록은 내용의 CRC나 경로 안전성을 검증한 결과가 아님.
                    await context.ListEntryAsync(new($"entry-{index}", entry.FullName, kind, entry.Length))
                        .ConfigureAwait(false);
                }
                return;
            }
            if (action != ArchiveAction.Create || inputs.Count != names.Length)
            {
                throw new ArchiveException(ArchiveError.UnsupportedOperation);
            }

            var normalized = ValidateNames(names);
            using var zip = new BoundedMemoryStream(Math.Min(outputLimit, WorkerProtocol.MaxDataBytes));
            using (var archive = new ZipArchive(zip, ZipArchiveMode.Create, leaveOpen: true))
            {
                var buffer = new byte[64 * 1024];
                for (var index = 0; index < inputs.Count; index++)
                {
                    context.Budget.Check(token);
                    await context.ListEntryAsync(new($"source-{index}", normalized[index], EntryKind.File, inputs[index].Length))
                        .ConfigureAwait(false);
                    var entry = archive.CreateEntry(normalized[index], CompressionLevel.Optimal);
                    await using var destination = entry.Open();
                    int count;
                    while ((count = await inputs[index].ReadAsync(buffer, token).ConfigureAwait(false)) != 0)
                    {
                        context.Budget.ChargeProcessed(count, token);
                        await destination.WriteAsync(buffer.AsMemory(0, count), token).ConfigureAwait(false);
                    }
                }
            }
            context.Budget.Check(token);
            zip.Position = 0;
            await context.WriteEntryAsync(new("artifact", "archive.zip", EntryKind.File, zip.Length), zip,
                _ => ValueTask.FromResult(VerificationState.NotChecked)).ConfigureAwait(false);
        }
        catch (InvalidDataException) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
    }

    private static string[] ValidateNames(string[] names)
    {
        var normalized = new string[names.Length];
        var policy = OutputPathPolicy.CurrentPolicy;
        var comparison = OutputPathPolicy.Comparison(policy);
        var seen = new HashSet<string>(OutputPathPolicy.Comparer(policy));
        for (var index = 0; index < names.Length; index++)
        {
            if (names[index].Length > WorkerProtocol.MaxNameLength)
            {
                throw new ArchiveException(ArchiveError.BudgetExceeded);
            }
            var name = string.Join('/', OutputPathPolicy.Validate(names[index], policy));
            if (seen.Any(existing => existing.Equals(name, comparison) ||
                existing.StartsWith(name + "/", comparison) ||
                name.StartsWith(existing + "/", comparison)))
            {
                throw new ArchiveException(ArchiveError.OutputConflict);
            }
            seen.Add(name);
            normalized[index] = name;
        }
        return normalized;
    }
}

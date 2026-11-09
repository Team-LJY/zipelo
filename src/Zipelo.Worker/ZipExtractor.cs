using System.Buffers.Binary;
using System.IO.Compression;
using System.Text;
using Zipelo.Core;
using Zipelo.Runtime;

namespace Zipelo.Worker;

// 초기 해제는 단일 디스크 ZIP32의 저장/Deflate 방식만 허용함.
internal static class ZipExtractor
{
    private sealed record Item(ArchiveEntry Entry, uint Crc, int Start, int Packed, int Method);

    internal static ZipWorkerResult Extract(WorkerRequest request, CancellationToken token)
    {
        try
        {
            var budget = new JobBudget(request.Limits);
            var bytes = request.Inputs.Single();
            budget.ChargeProcessed(bytes.Length, token);
            var items = Parse(bytes, budget, token);
            ZipExtraction.ValidatePaths(items.Select(item => item.Entry).ToArray());
            var outputs = new List<ZipExtractedEntry>();
            long total = 0;
            var buffer = new byte[65536];
            foreach (var item in items)
            {
                budget.Check(token);
                using var packed = new MemoryStream(bytes, item.Start, item.Packed, writable: false);
                using Stream decoded = item.Method == 0 ? packed : new DeflateStream(packed, CompressionMode.Decompress);
                using var output = new MemoryStream();
                var crc = uint.MaxValue;
                int count;
                while ((count = decoded.Read(buffer, 0, buffer.Length)) != 0)
                {
                    budget.ChargeProcessed(count, token);
                    budget.ReserveOutput(count, token);
                    total += count;
                    if (total > WorkerProtocol.MaxDataBytes) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
                    if (output.Length + count > item.Entry.DeclaredSize)
                    {
                        throw new ArchiveException(ArchiveError.IntegrityFailure);
                    }
                    crc = ZipExtraction.UpdateCrc(crc, buffer.AsSpan(0, count));
                    output.Write(buffer, 0, count);
                }
                budget.Check(token);
                if (output.Length != item.Entry.DeclaredSize || ~crc != item.Crc)
                {
                    throw new ArchiveException(ArchiveError.IntegrityFailure);
                }
                outputs.Add(new(item.Entry, output.ToArray(), item.Crc));
            }
            var reports = outputs.Select(item => new EntryResult(item.Entry, EntryOutcome.Staged,
                VerificationState.Passed, ArchiveError.None)).ToArray();
            budget.Check(token);
            return new(new(request.JobId, JobState.Completed, reports, ArchiveError.None), null)
            {
                ExtractedEntries = outputs.ToArray()
            };
        }
        catch (InvalidDataException) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
        catch (DecoderFallbackException) { throw new ArchiveException(ArchiveError.UnsafePath); }
        catch (OverflowException) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
    }

    private static Item[] Parse(byte[] data, JobBudget budget, CancellationToken token)
    {
        int U16(int offset) => BinaryPrimitives.ReadUInt16LittleEndian(Slice(data, offset, 2));
        uint U32(int offset) => BinaryPrimitives.ReadUInt32LittleEndian(Slice(data, offset, 4));
        int Size(int offset) => checked((int)U32(offset));
        var end = data.Length - 22;
        while (end >= Math.Max(0, data.Length - 65557))
        {
            if (U32(end) == 0x06054B50 && end + 22 + U16(end + 20) == data.Length) { break; }
            end--;
        }
        if (end < Math.Max(0, data.Length - 65557)) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
        if (U16(end + 4) != 0 || U16(end + 6) != 0 || U16(end + 8) != U16(end + 10) || U16(end + 10) == 65535)
        {
            throw new ArchiveException(ArchiveError.UnsupportedOperation);
        }
        var count = U16(end + 10);
        if (count > WorkerProtocol.MaxEntries) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
        var central = Size(end + 16);
        if ((long)central + Size(end + 12) != end) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
        var cursor = central;
        var items = new List<Item>();
        var ranges = new List<(int Start, int End)>();
        for (var index = 0; index < count; index++)
        {
            budget.AddEntry(token);
            _ = Slice(data, cursor, 46);
            if (U32(cursor) != 0x02014B50) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
            var creator = U16(cursor + 4) >> 8;
            var flags = U16(cursor + 8);
            var method = U16(cursor + 10);
            if (creator is not (0 or 3) || U16(cursor + 6) > 20 || U16(cursor + 34) != 0 ||
                (flags & ~0x080E) != 0 || method is not (0 or 8))
            {
                throw new ArchiveException(ArchiveError.UnsupportedOperation);
            }
            var crc = U32(cursor + 16);
            var packed = Size(cursor + 20);
            var unpacked = Size(cursor + 24);
            if (method == 8 && packed < 2) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
            var nameLength = U16(cursor + 28);
            var extraLength = U16(cursor + 30);
            var nameBytes = Slice(data, cursor + 46, nameLength);
            if ((flags & 0x0800) == 0 && nameBytes.IndexOfAnyInRange((byte)128, (byte)255) >= 0)
            {
                throw new ArchiveException(ArchiveError.UnsupportedOperation);
            }
            var name = new UTF8Encoding(false, true).GetString(nameBytes);
            var attributes = U32(cursor + 38);
            var unixType = (attributes >> 16) & 0xF000;
            if (unixType is not (0 or 0x8000 or 0x4000) || (attributes & 0x400) != 0)
            {
                throw new ArchiveException(ArchiveError.UnsupportedEntry);
            }
            var directory = name.EndsWith('/');
            if ((unixType == 0x4000 || (attributes & 0x10) != 0) && !directory ||
                directory && (unixType == 0x8000 || unpacked != 0 || crc != 0))
            {
                throw new ArchiveException(ArchiveError.UnsupportedEntry);
            }
            var path = directory ? name[..^1] : name;
            var entry = new ArchiveEntry($"entry-{index}", path, directory ? EntryKind.Directory : EntryKind.File, unpacked);
            CheckExtra(Slice(data, cursor + 46 + nameLength, extraLength));
            var local = Size(cursor + 42);
            _ = Slice(data, local, 30);
            if (U32(local) != 0x04034B50 || U16(local + 4) != U16(cursor + 6) ||
                U16(local + 6) != flags || U16(local + 8) != method || U16(local + 26) != nameLength ||
                !Slice(data, local + 30, nameLength).SequenceEqual(nameBytes))
            {
                throw new ArchiveException(ArchiveError.IntegrityFailure);
            }
            if ((flags & 8) == 0 && (U32(local + 14) != crc || Size(local + 18) != packed || Size(local + 22) != unpacked))
            {
                throw new ArchiveException(ArchiveError.IntegrityFailure);
            }
            if ((flags & 8) != 0 && ((U32(local + 14) != 0 && U32(local + 14) != crc) ||
                (U32(local + 18) != 0 && Size(local + 18) != packed) ||
                (U32(local + 22) != 0 && Size(local + 22) != unpacked)))
            {
                throw new ArchiveException(ArchiveError.IntegrityFailure);
            }
            var localExtra = U16(local + 28);
            CheckExtra(Slice(data, local + 30 + nameLength, localExtra));
            var start = checked(local + 30 + nameLength + localExtra);
            var finish = checked(start + packed);
            _ = Slice(data, start, packed);
            if ((flags & 8) != 0)
            {
                var descriptor = finish;
                if (U32(descriptor) == 0x08074B50) { descriptor += 4; }
                if (U32(descriptor) != crc || Size(descriptor + 4) != packed || Size(descriptor + 8) != unpacked)
                {
                    throw new ArchiveException(ArchiveError.IntegrityFailure);
                }
                finish = checked(descriptor + 12);
            }
            if (finish > central) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
            ranges.Add((local, finish));
            items.Add(new(entry, crc, start, packed, method));
            cursor = checked(cursor + 46 + nameLength + extraLength + U16(cursor + 32));
        }
        if (cursor != end) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
        var next = 0;
        foreach (var range in ranges.OrderBy(range => range.Start))
        {
            if (range.Start != next) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
            next = range.End;
        }
        if (next != central) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
        return items.ToArray();
    }

    private static ReadOnlySpan<byte> Slice(byte[] bytes, int offset, int length)
    {
        if (offset < 0 || length < 0 || offset > bytes.Length - length)
        {
            throw new ArchiveException(ArchiveError.IntegrityFailure);
        }
        return bytes.AsSpan(offset, length);
    }

    private static void CheckExtra(ReadOnlySpan<byte> bytes)
    {
        // 링크 의미를 가진 Unix/ASi 등 미해석 확장 필드는 수용하지 않음.
        while (!bytes.IsEmpty)
        {
            if (bytes.Length < 4) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
            var id = BinaryPrimitives.ReadUInt16LittleEndian(bytes);
            var size = BinaryPrimitives.ReadUInt16LittleEndian(bytes[2..]);
            if (size > bytes.Length - 4) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
            if (id != 0x5455) { throw new ArchiveException(ArchiveError.UnsupportedEntry); }
            bytes = bytes[(4 + size)..];
        }
    }
}

using System.Buffers.Binary;
using System.Text;
using Zipelo.Core;

namespace Zipelo.Runtime.Streaming;

internal sealed record ZipStreamItem(ArchiveEntry Entry, Guid Id, uint Crc, long Start, int Packed, int Method);

// 호스트와 작업자가 같은 제한 목록을 독립적으로 검증함.
internal static class ZipStreamIndex
{
    internal const int MaxMetadataBytes = 2 * 1024 * 1024;

    internal static ZipStreamItem[] Read(Stream source, JobBudget budget, CancellationToken token)
    {
        var length = source.Length;
        if (!source.CanSeek || length is < 22 or > WorkerProtocol.MaxDataBytes)
        {
            throw new ArchiveException(ArchiveError.IntegrityFailure);
        }
        long metadata = 0;
        void Range(long offset, long size)
        {
            if (offset < 0 || size < 0 || offset > length - size)
            {
                throw new ArchiveException(ArchiveError.IntegrityFailure);
            }
        }
        void ChargeMetadata(int size)
        {
            if (size > MaxMetadataBytes - metadata) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
            metadata += size;
        }
        byte[] ReadAt(long offset, int size)
        {
            budget.Check(token);
            Range(offset, size);
            if (size > 65557) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
            ChargeMetadata(size);
            budget.ChargeProcessed(size, token);
            var data = new byte[size];
            source.Position = offset;
            source.ReadExactly(data);
            return data;
        }

        var tailOffset = Math.Max(0, length - 65557);
        var tail = ReadAt(tailOffset, (int)(length - tailOffset));
        var endIndex = tail.Length - 22;
        while (endIndex >= 0)
        {
            if (U32(tail, endIndex) == 0x06054B50 && endIndex + 22 + U16(tail, endIndex + 20) == tail.Length) { break; }
            endIndex--;
        }
        if (endIndex < 0) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
        var end = tailOffset + endIndex;
        if (U16(tail, endIndex + 4) != 0 || U16(tail, endIndex + 6) != 0 ||
            U16(tail, endIndex + 8) != U16(tail, endIndex + 10) || U16(tail, endIndex + 10) == 65535)
        {
            throw new ArchiveException(ArchiveError.UnsupportedOperation);
        }
        var count = U16(tail, endIndex + 10);
        if (count > WorkerProtocol.MaxEntries) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
        var central = (long)U32(tail, endIndex + 16);
        if (central + U32(tail, endIndex + 12) != end)
        {
            throw new ArchiveException(ArchiveError.IntegrityFailure);
        }
        var cursor = central;
        var items = new List<ZipStreamItem>();
        var ranges = new List<(long Start, long End)>();
        long declaredTotal = 0;
        for (var index = 0; index < count; index++)
        {
            budget.AddEntry(token);
            var header = ReadAt(cursor, 46);
            if (U32(header, 0) != 0x02014B50) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
            var creator = U16(header, 4) >> 8;
            var flags = U16(header, 8);
            var method = U16(header, 10);
            if (creator is not (0 or 3) || U16(header, 6) > 20 || U16(header, 34) != 0 ||
                (flags & ~0x080E) != 0 || method is not (0 or 8))
            {
                throw new ArchiveException(ArchiveError.UnsupportedOperation);
            }
            var crc = U32(header, 16);
            var packed = Size(header, 20);
            var unpacked = Size(header, 24);
            if (unpacked > WorkerProtocol.MaxDataBytes - declaredTotal)
            {
                throw new ArchiveException(ArchiveError.BudgetExceeded);
            }
            declaredTotal += unpacked;
            if ((method == 8 && packed < 2) || (method == 0 && packed != unpacked))
            {
                throw new ArchiveException(ArchiveError.IntegrityFailure);
            }
            var nameLength = U16(header, 28);
            var extraLength = U16(header, 30);
            if (nameLength is 0 or > 4096) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
            var nameBytes = ReadAt(cursor + 46, nameLength);
            if ((flags & 0x0800) == 0 && nameBytes.AsSpan().IndexOfAnyInRange((byte)128, (byte)255) >= 0)
            {
                throw new ArchiveException(ArchiveError.UnsupportedOperation);
            }
            var name = new UTF8Encoding(false, true).GetString(nameBytes);
            var attributes = U32(header, 38);
            var unixType = (attributes >> 16) & 0xF000;
            if (unixType is not (0 or 0x8000 or 0x4000) || (attributes & 0x400) != 0)
            {
                throw new ArchiveException(ArchiveError.UnsupportedEntry);
            }
            var directory = name.EndsWith('/');
            if (((unixType == 0x4000 || (attributes & 0x10) != 0) && !directory) ||
                (directory && (unixType == 0x8000 || unpacked != 0 || crc != 0)))
            {
                throw new ArchiveException(ArchiveError.UnsupportedEntry);
            }
            var entry = new ArchiveEntry($"entry-{index}", directory ? name[..^1] : name,
                directory ? EntryKind.Directory : EntryKind.File, unpacked);
            CheckExtra(ReadAt(cursor + 46 + nameLength, extraLength));
            var local = (long)U32(header, 42);
            var localHeader = ReadAt(local, 30);
            if (U32(localHeader, 0) != 0x04034B50 || U16(localHeader, 4) != U16(header, 6) ||
                U16(localHeader, 6) != flags || U16(localHeader, 8) != method ||
                U16(localHeader, 26) != nameLength ||
                !ReadAt(local + 30, nameLength).AsSpan().SequenceEqual(nameBytes))
            {
                throw new ArchiveException(ArchiveError.IntegrityFailure);
            }
            if ((flags & 8) == 0 &&
                (U32(localHeader, 14) != crc || Size(localHeader, 18) != packed || Size(localHeader, 22) != unpacked))
            {
                throw new ArchiveException(ArchiveError.IntegrityFailure);
            }
            if ((flags & 8) != 0 && ((U32(localHeader, 14) != 0 && U32(localHeader, 14) != crc) ||
                (U32(localHeader, 18) != 0 && Size(localHeader, 18) != packed) ||
                (U32(localHeader, 22) != 0 && Size(localHeader, 22) != unpacked)))
            {
                throw new ArchiveException(ArchiveError.IntegrityFailure);
            }
            var localExtra = U16(localHeader, 28);
            CheckExtra(ReadAt(local + 30 + nameLength, localExtra));
            var start = local + 30 + nameLength + localExtra;
            var finish = start + packed;
            Range(start, packed);
            if ((flags & 8) != 0)
            {
                var descriptor = ReadAt(finish, 4);
                var descriptorStart = U32(descriptor, 0) == 0x08074B50 ? finish + 4 : finish;
                descriptor = ReadAt(descriptorStart, 12);
                if (U32(descriptor, 0) != crc || Size(descriptor, 4) != packed || Size(descriptor, 8) != unpacked)
                {
                    throw new ArchiveException(ArchiveError.IntegrityFailure);
                }
                finish = descriptorStart + 12;
            }
            if (finish > central) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
            ranges.Add((local, finish));
            var idBytes = new byte[16];
            BinaryPrimitives.WriteInt32LittleEndian(idBytes, index + 1);
            items.Add(new(entry, new Guid(idBytes), crc, start, packed, method));
            var commentLength = U16(header, 32);
            ChargeMetadata(commentLength);
            cursor += 46 + nameLength + extraLength + commentLength;
            if (cursor > end) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
        }
        if (cursor != end) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
        long next = 0;
        foreach (var range in ranges.OrderBy(range => range.Start))
        {
            if (range.Start != next) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
            next = range.End;
        }
        if (next != central) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
        ZipExtraction.ValidatePaths(items.Select(item => item.Entry).ToArray());
        return items.ToArray();
    }

    private static ushort U16(byte[] data, int offset) => BinaryPrimitives.ReadUInt16LittleEndian(data.AsSpan(offset, 2));
    private static uint U32(byte[] data, int offset) => BinaryPrimitives.ReadUInt32LittleEndian(data.AsSpan(offset, 4));
    private static int Size(byte[] data, int offset)
    {
        var value = U32(data, offset);
        if (value > WorkerProtocol.MaxDataBytes) { throw new ArchiveException(ArchiveError.BudgetExceeded); }
        return (int)value;
    }

    private static void CheckExtra(ReadOnlySpan<byte> data)
    {
        while (!data.IsEmpty)
        {
            if (data.Length < 4) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
            var id = BinaryPrimitives.ReadUInt16LittleEndian(data);
            var size = BinaryPrimitives.ReadUInt16LittleEndian(data[2..]);
            if (size > data.Length - 4) { throw new ArchiveException(ArchiveError.IntegrityFailure); }
            if (id != 0x5455) { throw new ArchiveException(ArchiveError.UnsupportedEntry); }
            data = data[(4 + size)..];
        }
    }
}

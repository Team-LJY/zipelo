using System.Buffers.Binary;
using System.IO.Compression;
using System.Text;

namespace Zipelo.StreamingExtractionTests;

internal sealed record FixtureEntry(string Name, byte[] Bytes, bool Deflate = false, byte[]? PackedSuffix = null);

internal static class ZipFixture
{
    public static byte[] Payload(int size = 65537) =>
        Enumerable.Range(0, size).Select(i => (byte)(i % 251)).ToArray();

    public static uint Crc(ReadOnlySpan<byte> bytes)
    {
        uint crc = uint.MaxValue;
        foreach (var value in bytes)
        {
            crc ^= value;
            for (var bit = 0; bit < 8; bit++) { crc = (crc >> 1) ^ ((crc & 1) == 0 ? 0 : 0xEDB88320u); }
        }
        return ~crc;
    }

    // 제품 ZIP 생성기와 독립된 작은 ZIP32 픽스처임.
    public static byte[] Build(params FixtureEntry[] entries)
    {
        using var output = new MemoryStream();
        using var writer = new BinaryWriter(output, Encoding.UTF8, true);
        var records = new List<(FixtureEntry Entry, byte[] Name, byte[] Packed, uint Offset, uint Crc)>();
        foreach (var entry in entries)
        {
            Check.That(entry.Bytes.Length <= 131073 && records.Count < 8, "픽스처 크기 제한 초과");
            var name = Encoding.UTF8.GetBytes(entry.Name);
            var packed = entry.Bytes;
            if (entry.Deflate)
            {
                using var compressed = new MemoryStream();
                using (var deflate = new DeflateStream(compressed, CompressionLevel.Fastest, true))
                {
                    deflate.Write(entry.Bytes);
                }
                packed = compressed.ToArray();
            }
            if (entry.PackedSuffix is { } suffix)
            {
                Check.That(entry.Deflate && suffix.Length is > 0 and <= 65540, "후행 픽스처 크기/방식 제한 초과");
                packed = packed.Concat(suffix).ToArray();
            }
            var offset = (uint)output.Position;
            var crc = Crc(entry.Bytes);
            writer.Write(0x04034B50u);
            writer.Write((ushort)20); writer.Write((ushort)0x800);
            writer.Write((ushort)(entry.Deflate ? 8 : 0));
            writer.Write(0u); writer.Write(crc);
            writer.Write((uint)packed.Length); writer.Write((uint)entry.Bytes.Length);
            writer.Write((ushort)name.Length); writer.Write((ushort)0);
            writer.Write(name); writer.Write(packed);
            records.Add((entry, name, packed, offset, crc));
        }
        var central = (uint)output.Position;
        foreach (var (entry, name, packed, offset, crc) in records)
        {
            writer.Write(0x02014B50u);
            writer.Write((ushort)20); writer.Write((ushort)20); writer.Write((ushort)0x800);
            writer.Write((ushort)(entry.Deflate ? 8 : 0));
            writer.Write(0u); writer.Write(crc);
            writer.Write((uint)packed.Length); writer.Write((uint)entry.Bytes.Length);
            writer.Write((ushort)name.Length);
            writer.Write((ushort)0); writer.Write((ushort)0); writer.Write((ushort)0); writer.Write((ushort)0);
            writer.Write(entry.Name.EndsWith('/') ? 0x10u : 0u); writer.Write(offset); writer.Write(name);
        }
        var centralSize = (uint)output.Position - central;
        writer.Write(0x06054B50u); writer.Write((ushort)0); writer.Write((ushort)0);
        writer.Write((ushort)records.Count); writer.Write((ushort)records.Count);
        writer.Write(centralSize); writer.Write(central); writer.Write((ushort)0);
        return output.ToArray();
    }

    public static byte[] DeflateTail(bool secondStream, bool large)
    {
        if (!secondStream) { return Enumerable.Repeat((byte)0xA5, large ? 65537 : 7).ToArray(); }
        // 독립된 최종 raw Deflate Stored 블록임. LEN/NLEN 뒤의 내용은 첫 출력과 다름.
        var length = (ushort)(large ? 65535 : 9);
        var stream = new byte[5 + length];
        stream[0] = 1;
        BinaryPrimitives.WriteUInt16LittleEndian(stream.AsSpan(1), length);
        BinaryPrimitives.WriteUInt16LittleEndian(stream.AsSpan(3), (ushort)(length ^ ushort.MaxValue));
        stream.AsSpan(5).Fill(0xE7);
        return stream;
    }

    public static void VerifyTailLayout(byte[] zip, FixtureEntry[] entries)
    {
        Check.Equal(2, entries.Length);
        var suffix = entries[0].PackedSuffix ?? throw new InvalidOperationException("후행 바이트 없음");
        var central = checked((int)Read32(zip, zip.Length - 6));
        var firstName = Encoding.UTF8.GetBytes(entries[0].Name);
        var secondName = Encoding.UTF8.GetBytes(entries[1].Name);
        var packedStart = 30 + firstName.Length;
        var packedLength = checked((int)Read32(zip, 18));
        var secondLocal = checked(packedStart + packedLength);
        var secondCentral = central + 46 + firstName.Length;
        Check.Equal(0x04034B50u, Read32(zip, 0));
        Check.Equal(0x02014B50u, Read32(zip, central));
        Check.Equal(0x02014B50u, Read32(zip, secondCentral));
        Check.That(packedLength > suffix.Length, "첫 Deflate 스트림 누락");
        Check.Equal((uint)packedLength, Read32(zip, central + 20));
        Check.Bytes(suffix, zip.AsSpan(secondLocal - suffix.Length, suffix.Length).ToArray());
        Check.Equal(0x04034B50u, Read32(zip, secondLocal));
        Check.Equal((uint)secondLocal, Read32(zip, secondCentral + 42));
        Check.Equal((uint)central, (uint)(secondLocal + 30 + secondName.Length + entries[1].Bytes.Length));
        Check.Equal((uint)entries[0].Bytes.Length, Read32(zip, 22));
        Check.Equal((uint)entries[0].Bytes.Length, Read32(zip, central + 24));
        Check.Equal(Crc(entries[0].Bytes), Read32(zip, 14));
        Check.Equal(Crc(entries[0].Bytes), Read32(zip, central + 16));
        Check.Bytes(firstName, zip.AsSpan(30, firstName.Length).ToArray());
        Check.Bytes(secondName, zip.AsSpan(secondLocal + 30, secondName.Length).ToArray());
        Check.Bytes(entries[1].Bytes, zip.AsSpan(secondLocal + 30 + secondName.Length, entries[1].Bytes.Length).ToArray());
    }

    public static void CorruptTailDeclaration(byte[] zip, string fault)
    {
        var central = checked((int)Read32(zip, zip.Length - 6));
        var (localField, centralField) = fault switch
        {
            "crc" => (14, 16),
            "length" => (22, 24),
            "overlap" => (18, 20),
            _ => throw new ArgumentException("알 수 없는 후행 데이터 오류", nameof(fault))
        };
        var value = Read32(zip, localField);
        value = fault == "crc" ? value ^ 1u : checked(value + 1);
        BinaryPrimitives.WriteUInt32LittleEndian(zip.AsSpan(localField), value);
        BinaryPrimitives.WriteUInt32LittleEndian(zip.AsSpan(central + centralField), value);
    }

    private static uint Read32(byte[] bytes, int offset) =>
        BinaryPrimitives.ReadUInt32LittleEndian(bytes.AsSpan(offset, 4));
}

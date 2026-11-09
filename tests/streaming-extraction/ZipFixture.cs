using System.IO.Compression;
using System.Text;

namespace Zipelo.StreamingExtractionTests;

internal sealed record FixtureEntry(string Name, byte[] Bytes, bool Deflate = false);

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
}

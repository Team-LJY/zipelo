using System.Buffers.Binary;
using System.Security.Cryptography;

internal sealed record Pattern(int Length, Func<int, byte> At)
{
    public string Sha256()
    {
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        var chunk = new byte[1024];
        for (var offset = 0; offset < Length; offset += chunk.Length)
        {
            var count = Math.Min(chunk.Length, Length - offset);
            for (var i = 0; i < count; i++) { chunk[i] = At(offset + i); }
            hash.AppendData(chunk, 0, count);
        }
        return Convert.ToHexString(hash.GetHashAndReset());
    }
}

internal enum Envelope { Raw, Zlib, GzipBody }
internal sealed record Sample(string Name, byte[] Raw, int EndBits, Pattern Output, byte[]? Dictionary = null)
{
    public byte[] Wire(Envelope envelope)
    {
        if (envelope == Envelope.Raw) { return (byte[])Raw.Clone(); }
        if (envelope == Envelope.Zlib)
        {
            var prefix = Dictionary == null ? 2 : 6;
            var bytes = new byte[prefix + Raw.Length + 4];
            bytes[0] = 0x78;
            bytes[1] = Dictionary == null ? (byte)0x01 : (byte)0x20;
            if (Dictionary != null)
            {
                BinaryPrimitives.WriteUInt32BigEndian(bytes.AsSpan(2), Fixtures.Adler(Dictionary));
            }
            Raw.CopyTo(bytes, prefix);
            BinaryPrimitives.WriteUInt32BigEndian(bytes.AsSpan(prefix + Raw.Length), Fixtures.Adler(Output));
            return bytes;
        }
        var gzip = new byte[10 + Raw.Length + 8];
        new byte[] { 0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255 }.CopyTo(gzip, 0);
        Raw.CopyTo(gzip, 10);
        BinaryPrimitives.WriteUInt32LittleEndian(gzip.AsSpan(10 + Raw.Length), Fixtures.Crc(Output));
        BinaryPrimitives.WriteUInt32LittleEndian(gzip.AsSpan(14 + Raw.Length), (uint)Output.Length);
        return gzip;
    }
}

internal static class Fixtures
{
    public static readonly byte[] Sentinel = [0xde, 0xad, 0xbe, 0xef, 0x5a, 0xa5, 0xc3, 0x3c];
    public static Pattern Repeated(int count, byte value) => new(count, _ => value);
    public static Pattern Sequence(int count) => new(count, i => (byte)((i * 73 + 19) & 255));

    public static Sample Stored(int count)
    {
        var output = Sequence(count);
        var bits = new Bits();
        bits.Stored(output, true);
        return new($"stored-{count}", bits.Bytes(), bits.Count, output);
    }

    public static Sample EmptyFixed() => new("empty-fixed", [3, 0], 10, Repeated(0, 0));

    public static Sample Overlap()
    {
        var bits = new Bits();
        bits.Put(3, 3);
        bits.Literal(65);
        bits.Literal(285);
        bits.Code(0, 5);
        bits.Literal(256);
        return new("fixed-overlap-259", bits.Bytes(), 31, Repeated(259, 65));
    }

    public static Sample StoredFixed()
    {
        var bits = new Bits();
        bits.Stored(new(2, i => (byte)(88 + i)), false);
        AddXyReference(bits);
        return new("stored-fixed-xy", bits.Bytes(), 78, new(7, i => (byte)(88 + i % 2)));
    }

    public static Sample DictionaryXy()
    {
        var bits = new Bits();
        AddXyReference(bits);
        return new("dictionary-xy", bits.Bytes(), 22, new(5, i => (byte)(88 + i % 2)), [88, 89]);
    }

    public static Sample DictionaryWindow()
    {
        var dictionary = new byte[32769];
        for (var i = 0; i < dictionary.Length; i++) { dictionary[i] = SequenceByte(i); }
        var bits = new Bits();
        bits.Put(3, 3);
        bits.Literal(257);
        bits.Code(29, 5);
        bits.Put(8191, 13);
        bits.Literal(256);
        return new("dictionary-window-32769", bits.Bytes(), 35,
            new(3, i => SequenceByte(i + 1)), dictionary);
    }

    public static Sample InvalidFirstReference() =>
        new("first-reference-distance-1", [3, 2, 0], 22, Repeated(3, 0));

    private static byte SequenceByte(int i) => (byte)((i * 73 + 19) & 255);

    private static void AddXyReference(Bits bits)
    {
        bits.Put(3, 3);
        bits.Literal(259);
        bits.Code(1, 5);
        bits.Literal(256);
    }

    public static uint Adler(byte[] bytes) => Adler(new Pattern(bytes.Length, i => bytes[i]));

    public static uint Adler(Pattern pattern)
    {
        uint a = 1;
        uint b = 0;
        for (var i = 0; i < pattern.Length; i++)
        {
            a = (a + pattern.At(i)) % 65521;
            b = (b + a) % 65521;
        }
        return (b << 16) | a;
    }

    public static uint Crc(Pattern pattern)
    {
        uint crc = 0xffffffff;
        for (var i = 0; i < pattern.Length; i++)
        {
            crc ^= pattern.At(i);
            for (var bit = 0; bit < 8; bit++)
            {
                crc = (crc >> 1) ^ ((crc & 1) == 0 ? 0u : 0xedb88320u);
            }
        }
        return ~crc;
    }

    // 고정 토큰과 stored 표본만 작성함. 디코더나 범용 압축기가 아님.
    private sealed class Bits
    {
        private readonly List<byte> bytes = [];
        public int Count { get; private set; }

        public void Put(int value, int count)
        {
            for (var i = 0; i < count; i++)
            {
                if (Count % 8 == 0) { bytes.Add(0); }
                bytes[^1] |= (byte)(((value >> i) & 1) << (Count % 8));
                Count++;
            }
        }

        public void Code(int value, int count)
        {
            for (var i = count - 1; i >= 0; i--) { Put((value >> i) & 1, 1); }
        }

        public void Literal(int symbol)
        {
            if (symbol <= 143) { Code(48 + symbol, 8); }
            else if (symbol <= 255) { Code(400 + symbol - 144, 9); }
            else if (symbol <= 279) { Code(symbol - 256, 7); }
            else { Code(192 + symbol - 280, 8); }
        }

        public void Stored(Pattern data, bool final)
        {
            Put(final ? 1 : 0, 3);
            while (Count % 8 != 0) { Put(0, 1); }
            Put(data.Length, 16);
            Put(data.Length ^ 65535, 16);
            for (var i = 0; i < data.Length; i++) { Put(data.At(i), 8); }
        }

        public byte[] Bytes() => bytes.ToArray();
    }
}

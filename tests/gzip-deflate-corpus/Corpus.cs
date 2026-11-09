using System.Security.Cryptography;

namespace Zipelo.DeflateCorpus;

internal sealed record Token(int StartBit, int BitLength, string Kind, string Value);

internal sealed record Digest(int Length, string Sha256, string Crc32);

internal sealed record Pattern(string Kind, int Length, int Prefix = 0, int Distance = 0)
{
    public byte At(int index)
    {
        if ((uint)index >= (uint)Length)
        {
            throw new ArgumentOutOfRangeException(nameof(index));
        }

        return Kind switch
        {
            "zero" => 0,
            "a" => (byte)'A',
            "xy" => (byte)(index % 2 == 0 ? 'X' : 'Y'),
            "window" => Seed(index < Prefix ? index : index - Distance),
            "tail" => Seed(index < Prefix ? index : Prefix - 1),
            _ => throw new InvalidOperationException("알 수 없는 출력 패턴임.")
        };
    }

    public static byte Seed(int index)
    {
        return (byte)((index * 37 + (index >> 8) * 13 + (index >> 15) * 71) & 255);
    }

    public Digest Hash()
    {
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        var buffer = new byte[4096];
        uint crc = uint.MaxValue;
        for (var offset = 0; offset < Length;)
        {
            var count = Math.Min(buffer.Length, Length - offset);
            for (var index = 0; index < count; index++)
            {
                var value = At(offset + index);
                buffer[index] = value;
                crc ^= value;
                for (var bit = 0; bit < 8; bit++)
                {
                    crc = (crc >> 1) ^ ((crc & 1) != 0 ? 0xedb88320u : 0u);
                }
            }

            hash.AppendData(buffer, 0, count);
            offset += count;
        }

        return new Digest(Length, Convert.ToHexString(hash.GetHashAndReset()), (~crc).ToString("X8"));
    }
}

internal sealed record Fixture(
    string Id,
    byte[] Raw,
    int EndBitExclusive,
    Token[] Tokens,
    Pattern? Output,
    string Rationale,
    int? FaultBitOffset = null,
    int? HistoryBeforeFault = null,
    int? MatchDistance = null,
    Pattern? InvalidWrapperPattern = null)
{
    public bool Valid => Output is not null;
}

internal static class Corpus
{
    public const int MaxRawBytes = 70000;
    public const int MaxOutputBytes = 66000;

    public static IEnumerable<Fixture> Create()
    {
        yield return new Fixture(
            "empty.fixed", [0x03, 0x00], 10,
            [
                new Token(0, 1, "BFINAL", "1"),
                new Token(1, 2, "BTYPE", "01 fixed"),
                new Token(3, 7, "EOB", "symbol 256, code 0000000")
            ],
            new Pattern("zero", 0), "최종 fixed 블록의 EOB만 존재함.");
        yield return new Fixture(
            "empty.stored", [0x01, 0x00, 0x00, 0xff, 0xff], 40,
            [
                new Token(0, 1, "BFINAL", "1"),
                new Token(1, 2, "BTYPE", "00 stored"),
                new Token(3, 5, "alignment", "ignored zero bits"),
                new Token(8, 16, "LEN", "0"),
                new Token(24, 16, "NLEN", "65535")
            ],
            new Pattern("zero", 0), "길이 0인 최종 stored 블록임.");

        yield return FixedMatch("history.before-first.invalid", [], 3, 1, null, new Pattern("zero", 3));
        yield return FixedMatch("history.distance-one.valid", [0], 3, 1, new Pattern("zero", 4));
        yield return FixedMatch("history.one-distance-two.invalid", [0], 3, 2, null, new Pattern("zero", 4));
        yield return FixedMatch("history.distance-two.valid", [0, 0], 3, 2, new Pattern("zero", 5));
        yield return StoredMatch("history.32767-distance32768.invalid", 32767, 3, 32768, true, null,
            new Pattern("zero", 32770));
        yield return StoredMatch("history.32768-distance32768.valid", 32768, 3, 32768, true,
            new Pattern("zero", 32771));
        yield return FixedMatch("overlap.distance-one.length258", [(byte)'A'], 258, 1, new Pattern("a", 259));
        yield return FixedMatch("overlap.xy.length5.distance2", [(byte)'X', (byte)'Y'], 5, 2, new Pattern("xy", 7));

        foreach (var prefix in new[] { 32768, 32769, 65535, 65536, 65537 })
        {
            yield return StoredMatch($"window.prefix-{prefix}", prefix, 258, 32768, false,
                new Pattern("window", prefix + 258, prefix, 32768));
        }

        foreach (var length in new[] { 32767, 32768, 32769, 65535, 65536, 65537 })
        {
            yield return StoredMatch($"output.length-{length}", length - 3, 3, 1, false,
                new Pattern("tail", length, length - 3));
        }

        yield return DynamicLiteralOnly();
    }

    private static Fixture FixedMatch(
        string id, byte[] literals, int length, int distance, Pattern? output, Pattern? invalidPattern = null)
    {
        var writer = new FixtureWriter();
        writer.FixedHeader();
        writer.Literals(literals);
        var fault = writer.Position;
        writer.Match(length, distance);
        writer.EndFixed();
        return new Fixture(id, writer.Bytes(), writer.Position, writer.Tokens.ToArray(), output,
            output is null
                ? "거리 값이 참조 직전 생성된 이력보다 큼. CRC 오류와 무관하게 Deflate 거부가 필요함."
                : "거리 이하의 이력이 존재하며 겹침 복사도 허용됨.",
            output is null ? fault : null, literals.Length, distance, invalidPattern);
    }

    private static Fixture StoredMatch(
        string id, int prefix, int length, int distance, bool zeros, Pattern? output,
        Pattern? invalidPattern = null)
    {
        var writer = new FixtureWriter();
        for (var offset = 0; offset < prefix;)
        {
            var count = Math.Min(65535, prefix - offset);
            writer.Stored(offset, count, zeros);
            offset += count;
        }

        writer.FixedHeader();
        var fault = writer.Position;
        writer.Match(length, distance);
        writer.EndFixed();
        return new Fixture(id, writer.Bytes(), writer.Position, writer.Tokens.ToArray(), output,
            output is null
                ? "stored 출력 32767바이트 뒤 거리 32768을 사용하므로 한 바이트만큼 이력이 부족함."
                : "stored 블록에서 만든 이력을 최종 fixed 블록이 참조함. 블록 경계는 이력을 초기화하지 않음.",
            output is null ? fault : null, prefix, distance, invalidPattern);
    }

    private static Fixture DynamicLiteralOnly()
    {
        // 범용 동적 트리 작성기 없이 검토된 정적 표본만 사용함.
        return new Fixture(
            "dynamic.literal-only.a",
            Convert.FromHexString("05C001040000000010000000000000000001000000000000000000000000000000000000000000008004"),
            331,
            [
                new Token(0, 1, "BFINAL", "1"),
                new Token(1, 2, "BTYPE", "10 dynamic"),
                new Token(3, 5, "HLIT", "0: 257 literal/length symbols"),
                new Token(8, 5, "HDIST", "0: one distance length"),
                new Token(13, 4, "HCLEN", "14: 18 code-length lengths"),
                new Token(17, 54, "code-length alphabet", "RFC order: lengths 1 for symbols 0 and 1, otherwise 0"),
                new Token(71, 258, "literal/distance lengths", "257 literal/length lengths: 1 at 65 and 256; distance length 0; all others 0; one bit per length"),
                new Token(329, 1, "literal", "65: canonical code 0"),
                new Token(330, 1, "EOB", "256: canonical code 1")
            ],
            new Pattern("a", 1),
            "A와 EOB만 길이 1인 literal 트리를 사용함. 거리 트리의 단일 길이 0은 거리 참조 없음이며 RFC 1951 3.2.7에서 허용됨.");
    }
}

internal sealed class FixtureWriter
{
    private readonly List<byte> bytes = [];
    public List<Token> Tokens { get; } = [];
    public int Position { get; private set; }

    public void FixedHeader()
    {
        Field(1, 1, "BFINAL", "1");
        Field(1, 2, "BTYPE", "01 fixed");
    }

    public void Stored(int offset, int count, bool zeros)
    {
        if (count is < 1 or > 65535)
        {
            throw new ArgumentOutOfRangeException(nameof(count));
        }

        Field(0, 1, "BFINAL", "0");
        Field(0, 2, "BTYPE", "00 stored");
        var padding = (8 - Position % 8) % 8;
        Field(0, padding, "alignment", "ignored zero bits");
        Field((uint)count, 16, "LEN", count.ToString());
        Field((uint)(count ^ 65535), 16, "NLEN", (count ^ 65535).ToString());
        var start = Position;
        for (var index = 0; index < count; index++)
        {
            Bits(zeros ? 0u : Pattern.Seed(offset + index), 8);
        }

        Tokens.Add(new Token(start, Position - start, "stored bytes",
            zeros ? $"{count} zero bytes" : $"seed(i), i=[{offset},{offset + count}); seed(i)=(37*i+13*(i>>8)+71*(i>>15))&255"));
    }

    public void Literals(byte[] values)
    {
        foreach (var value in values)
        {
            var start = Position;
            Symbol(value);
            Tokens.Add(new Token(start, Position - start, "literal", value.ToString()));
        }
    }

    public void Match(int length, int distance)
    {
        var start = Position;
        var symbol = length switch
        {
            3 => 257,
            5 => 259,
            258 => 285,
            _ => throw new ArgumentOutOfRangeException(nameof(length))
        };
        Symbol(symbol);
        Tokens.Add(new Token(start, Position - start, "length", $"symbol={symbol}, length={length}, extraBits=0"));

        start = Position;
        var code = distance switch
        {
            1 => 0,
            2 => 1,
            32768 => 29,
            _ => throw new ArgumentOutOfRangeException(nameof(distance))
        };
        Huffman((uint)code, 5);
        Tokens.Add(new Token(start, 5, "distance code", code.ToString()));
        if (distance == 32768)
        {
            Field(8191, 13, "distance extra", "24577 + 8191 = 32768");
        }
    }

    public void EndFixed()
    {
        var start = Position;
        Symbol(256);
        Tokens.Add(new Token(start, Position - start, "EOB", "symbol 256, code 0000000"));
    }

    public byte[] Bytes() => bytes.ToArray();

    private void Symbol(int symbol)
    {
        var (code, width) = symbol switch
        {
            < 144 => (0x30 + symbol, 8),
            < 256 => (0x190 + symbol - 144, 9),
            < 280 => (symbol - 256, 7),
            _ => (0xc0 + symbol - 280, 8)
        };
        Huffman((uint)code, width);
    }

    private void Huffman(uint value, int width)
    {
        // Huffman 코드는 최상위 비트부터 전송함.
        for (var bit = width - 1; bit >= 0; bit--)
        {
            Bits((value >> bit) & 1, 1);
        }
    }

    private void Field(uint value, int width, string kind, string description)
    {
        var start = Position;
        Bits(value, width);
        Tokens.Add(new Token(start, width, kind, description));
    }

    private void Bits(uint value, int width)
    {
        if (width < 0 || width > 16 || value >> width != 0)
        {
            throw new ArgumentOutOfRangeException(nameof(width));
        }

        for (var bit = 0; bit < width; bit++)
        {
            if (Position / 8 == bytes.Count)
            {
                if (bytes.Count >= Corpus.MaxRawBytes)
                {
                    throw new InvalidOperationException("표본 크기 상한을 초과함.");
                }
                bytes.Add(0);
            }

            bytes[Position / 8] |= (byte)(((value >> bit) & 1) << (Position % 8));
            Position++;
        }
    }
}

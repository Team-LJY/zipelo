namespace Zipelo.DynamicDeflateCorpus;

internal sealed record Token(int StartBit, int BitLength, string Kind, string Value);
internal sealed record Codeword(int Symbol, int Length, string Bits);
internal sealed record TreeDescription(string Name, int[] Lengths, string Shape, Codeword[]? Canonical);
internal sealed record LengthCode(int Symbol, int Extra = 0);
internal sealed record HeaderLayout(int DataStart, int[] LengthStarts, int[] MetaStarts);

// 합성 표본의 코드 작성만 담당함. 입력 해제나 일반 압축 기능은 없음.
internal sealed class Codebook
{
    public int[] Lengths { get; }
    public int[] Codes { get; }
    public string Shape { get; }
    public bool OverSubscribed => Shape == "over-subscribed";

    public Codebook(int[] lengths)
    {
        Lengths = (int[])lengths.Clone();
        Codes = new int[lengths.Length];
        if (lengths.Any(length => length is < 0 or > 15))
        {
            throw new ArgumentOutOfRangeException(nameof(lengths));
        }
        var counts = new int[16];
        foreach (var length in lengths)
        {
            if (length != 0) { counts[length]++; }
        }
        var left = 1;
        for (var bits = 1; bits <= 15; bits++)
        {
            left = left * 2 - counts[bits];
            if (left < 0)
            {
                Shape = "over-subscribed";
                return;
            }
        }
        Shape = counts.Sum() == 0 ? "empty" : left == 0 ? "complete"
            : counts[1] == 1 && counts.Sum() == 1 ? "single-one-bit" : "incomplete";
        var next = new int[16];
        var code = 0;
        for (var bits = 1; bits <= 15; bits++)
        {
            code = (code + counts[bits - 1]) << 1;
            next[bits] = code;
        }
        for (var symbol = 0; symbol < lengths.Length; symbol++)
        {
            if (lengths[symbol] != 0) { Codes[symbol] = next[lengths[symbol]]++; }
        }
    }

    public TreeDescription Describe(string name) => new(name, Lengths, Shape,
        OverSubscribed ? null : Enumerable.Range(0, Lengths.Length)
            .Where(symbol => Lengths[symbol] != 0)
            .Select(symbol => new Codeword(symbol, Lengths[symbol],
                Convert.ToString(Codes[symbol], 2).PadLeft(Lengths[symbol], '0'))).ToArray());
}

internal sealed class FixtureWriter
{
    private static readonly int[] MetaOrder = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    private readonly List<byte> bytes = [];
    public List<Token> Tokens { get; } = [];
    public List<TreeDescription> Trees { get; } = [];
    public int Position { get; private set; }

    public byte[] Bytes() => bytes.ToArray();

    public HeaderLayout Dynamic(int[] literalLengths, int[] distanceLengths, int[] metaLengths,
        LengthCode[]? encodedLengths = null, int[]? suffixEncodingMeta = null)
    {
        if (literalLengths.Length is < 257 or > 286 || distanceLengths.Length is < 1 or > 32 ||
            metaLengths.Length != 19 || metaLengths.Any(length => length is < 0 or > 7))
        {
            throw new ArgumentException("표본 헤더의 선언 범위를 벗어남.");
        }
        var ll = new Codebook(literalLengths);
        var dd = new Codebook(distanceLengths);
        var meta = new Codebook(metaLengths);
        Trees.Add(ll.Describe("literal-length"));
        Trees.Add(dd.Describe("distance"));
        Trees.Add(meta.Describe("code-length"));
        var encoding = suffixEncodingMeta is null ? meta : new Codebook(suffixEncodingMeta);
        if (suffixEncodingMeta is not null)
        {
            Trees.Add(encoding.Describe("reference-only-suffix-encoding"));
        }
        var metaCount = Math.Max(4, Array.FindLastIndex(MetaOrder, symbol => metaLengths[symbol] != 0) + 1);
        Field(1, 1, "BFINAL", "1");
        Field(2, 2, "BTYPE", "10 dynamic");
        Field((uint)(literalLengths.Length - 257), 5, "HLIT", literalLengths.Length.ToString());
        Field((uint)(distanceLengths.Length - 1), 5, "HDIST", distanceLengths.Length.ToString());
        Field((uint)(metaCount - 4), 4, "HCLEN", metaCount.ToString());
        var metaStarts = new int[metaCount];
        for (var index = 0; index < metaCount; index++)
        {
            metaStarts[index] = Position;
            var symbol = MetaOrder[index];
            Field((uint)metaLengths[symbol], 3, "meta-length", $"symbol={symbol}, length={metaLengths[symbol]}");
        }
        var sequence = encodedLengths ?? literalLengths.Concat(distanceLengths)
            .Select(length => new LengthCode(length)).ToArray();
        var starts = new int[sequence.Length];
        var expandedIndex = 0;
        for (var index = 0; index < sequence.Length; index++)
        {
            var item = sequence[index];
            starts[index] = Position;
            Huffman(encoding, item.Symbol, "code-length", $"symbol={item.Symbol}, targetIndex={expandedIndex}");
            var extraBits = item.Symbol switch { 16 => 2, 17 => 3, 18 => 7, _ => 0 };
            if (extraBits != 0)
            {
                Field((uint)item.Extra, extraBits, "repeat-extra", item.Extra.ToString());
            }
            expandedIndex += item.Symbol switch { 16 => 3 + item.Extra, 17 => 3 + item.Extra, 18 => 11 + item.Extra, _ => 1 };
        }
        return new HeaderLayout(Position, starts, metaStarts);
    }

    public void StoredXY()
    {
        Field(0, 1, "BFINAL", "0");
        Field(0, 2, "BTYPE", "00 stored");
        Field(0, (8 - Position % 8) % 8, "alignment", "ignored zero bits");
        Field(2, 16, "LEN", "2");
        Field(65533, 16, "NLEN", "65533");
        Field(88, 8, "stored-literal", "X");
        Field(89, 8, "stored-literal", "Y");
    }

    public (Codebook Literal, Codebook Distance) Fixed()
    {
        Field(1, 1, "BFINAL", "1");
        Field(1, 2, "BTYPE", "01 fixed");
        var ll = new Codebook(Enumerable.Range(0, 288)
            .Select(symbol => symbol <= 143 ? 8 : symbol <= 255 ? 9 : symbol <= 279 ? 7 : 8).ToArray());
        var dd = new Codebook(Enumerable.Repeat(5, 32).ToArray());
        Trees.Add(ll.Describe("fixed-literal-length"));
        Trees.Add(dd.Describe("fixed-distance-including-reserved"));
        return (ll, dd);
    }

    public void Huffman(Codebook book, int symbol, string kind, string meaning)
    {
        if (book.OverSubscribed || (uint)symbol >= (uint)book.Lengths.Length || book.Lengths[symbol] == 0)
        {
            throw new ArgumentException("정의되지 않은 canonical 코드를 작성하려 함.");
        }
        var start = Position;
        var length = book.Lengths[symbol];
        var code = book.Codes[symbol];
        for (var bit = length - 1; bit >= 0; bit--) { Bit((code >> bit) & 1); }
        Tokens.Add(new Token(start, length, kind,
            $"{meaning}; symbol={symbol}, canonical={Convert.ToString(code, 2).PadLeft(length, '0')}, MSB-first"));
    }

    public void OpaqueSuffix()
    {
        // 과다 할당 트리 뒤의 비교용 비트열이며 유효 토큰이라고 주장하지 않음.
        var start = Position;
        foreach (var bit in new[] { 0, 1, 0, 1, 1 }) { Bit(bit); }
        Tokens.Add(new Token(start, 5, "opaque-suffix", "01011; valid AB control body only, no meaning under malformed tree"));
    }

    public void Field(uint value, int count, string kind, string meaning)
    {
        if (count is < 0 or > 16 || value >= (1u << count))
        {
            throw new ArgumentOutOfRangeException(nameof(value));
        }
        var start = Position;
        for (var bit = 0; bit < count; bit++) { Bit((int)((value >> bit) & 1)); }
        Tokens.Add(new Token(start, count, kind, $"{meaning}; value={value}, LSB-first"));
    }

    private void Bit(int value)
    {
        if (Position >= 4096 * 8) { throw new InvalidOperationException("표본 입력 상한을 초과함."); }
        if (Position % 8 == 0) { bytes.Add(0); }
        if (value != 0) { bytes[Position / 8] |= (byte)(1 << (Position % 8)); }
        Position++;
    }
}

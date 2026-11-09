namespace Zipelo.IncompleteFixtures;

// 합성 비트열 작성만 담당하며 decoder를 포함하지 않음.
internal sealed class FixtureWriter
{
    private static readonly int[] MetaOrder = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    private readonly List<byte> bytes = [];
    public readonly List<BitToken> Tokens = [];
    public readonly List<TreeSpec> Trees = [];
    public int Position { get; private set; }
    public byte[] Bytes => bytes.ToArray();

    public void Header(int[] literal, int[] distance, int[] meta, bool emitLengths = true)
    {
        Specification.Require(literal.Length is >= 257 and <= 286 && distance.Length is >= 1 and <= 32
            && meta.Length == 19, "표본 헤더 범위가 잘못됨.");
        if (emitLengths)
        {
            Trees.Add(Describe("literal-length", literal));
            Trees.Add(Describe("distance", distance));
        }
        Trees.Add(Describe("code-length", meta));
        Field(1, 1, "BFINAL");
        Field(2, 2, "BTYPE");
        Field(literal.Length - 257, 5, "HLIT");
        Field(distance.Length - 1, 5, "HDIST");
        Field(14, 4, "HCLEN");
        foreach (var symbol in MetaOrder.Take(18))
        {
            Field(meta[symbol], 3, $"meta-length-{symbol}");
        }
        if (!emitLengths) { return; }
        foreach (var length in literal.Concat(distance))
        {
            Code(meta, length, "code-length");
        }
    }

    public void Code(int[] lengths, int symbol, string meaning)
    {
        Specification.Require(symbol >= 0 && symbol < lengths.Length && lengths[symbol] > 0,
            "정의되지 않은 코드를 작성하려 함.");
        Raw(Canonical(lengths)[symbol], "huffman", meaning);
    }

    public void Raw(string bits, string kind, string meaning)
    {
        var start = Position;
        foreach (var bit in bits)
        {
            Specification.Require(bit is '0' or '1', "이진 문자열이 아님.");
            Specification.Require(Position < 4096, "표본 비트 상한을 초과함.");
            if (Position % 8 == 0) { bytes.Add(0); }
            if (bit == '1') { bytes[Position / 8] |= (byte)(1 << (Position % 8)); }
            Position++;
        }
        Tokens.Add(new BitToken(start, bits.Length, kind, bits, meaning));
    }

    private void Field(int value, int width, string name)
    {
        Specification.Require(value >= 0 && value < (1 << width), "필드 범위를 벗어남.");
        var bits = new string(Enumerable.Range(0, width).Select(bit => ((value >> bit) & 1) == 0 ? '0' : '1').ToArray());
        Raw(bits, "field-lsb-first", name);
    }

    private static TreeSpec Describe(string name, int[] lengths) => new(name, lengths, Canonical(lengths));

    private static string[] Canonical(int[] lengths)
    {
        var counts = new int[16];
        foreach (var length in lengths)
        {
            Specification.Require(length is >= 0 and <= 15, "코드 길이 범위를 벗어남.");
            if (length > 0) { counts[length]++; }
        }
        var left = 1;
        for (var bits = 1; bits <= 15; bits++)
        {
            left = left * 2 - counts[bits];
            Specification.Require(left >= 0, "과다 할당 트리를 작성하려 함.");
        }
        var next = new int[16];
        var code = 0;
        for (var bits = 1; bits <= 15; bits++)
        {
            code = (code + counts[bits - 1]) << 1;
            next[bits] = code;
        }
        var result = new string[lengths.Length];
        for (var symbol = 0; symbol < lengths.Length; symbol++)
        {
            var length = lengths[symbol];
            result[symbol] = length == 0 ? "" : Convert.ToString(next[length]++, 2).PadLeft(length, '0');
        }
        return result;
    }
}

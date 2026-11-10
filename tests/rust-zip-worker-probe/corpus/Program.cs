using System.Buffers.Binary;
using System.Text;
using Zipelo.StreamingExtractionTests;

if (args is not ["--gate", var gate, "--output", var output]) { return 2; }
await Data.Gate(gate);
var samples = new List<Sample>();
void Add(string id, FixtureEntry[] entries, byte[]? bytes = null)
{
    var zip = bytes ?? ZipFixture.Build(entries);
    samples.Add(new(id, Convert.ToBase64String(zip), Data.Hash(zip),
        entries.Select(e => new ExpectedEntry(e.Name, e.Name.EndsWith('/') ? "Directory" : "File",
            e.Bytes.LongLength, Data.Hash(e.Bytes), ZipFixture.Crc(e.Bytes))).ToArray()));
}
Add("empty", []);
Add("empty-items", [new("empty", []), new("folder/", [])]);
Add("stored-chunk", [new("payload.bin", ZipFixture.Payload())]);
Add("deflate-chunk", [new("compressed.bin", ZipFixture.Payload(131073), true)]);
Add("utf8", [new("한글/", []), new("한글/내용.txt", Encoding.UTF8.GetBytes("합성 표본\n"), true)]);
Add("budget-pair", [new("a", [1, 2]), new("b", [3, 4])]);
foreach (var second in new[] { false, true })
{
    foreach (var large in new[] { false, true })
    {
        FixtureEntry[] entries =
        [
            new("first.bin", ZipFixture.Payload(1024), true, ZipFixture.DeflateTail(second, large)),
            new("after.bin", [19, 71, 203, 0, 255])
        ];
        var zip = ZipFixture.Build(entries);
        ZipFixture.VerifyTailLayout(zip, entries);
        Add($"tail-{(second ? "second" : "garbage")}-{(large ? "large" : "small")}", entries, zip);
        if (!second && !large)
        {
            foreach (var fault in new[] { "crc", "length", "overlap" })
            {
                var changed = (byte[])zip.Clone();
                ZipFixture.CorruptTailDeclaration(changed, fault);
                Add("tail-" + fault, entries, changed);
            }
        }
    }
}
// ZIP 컨테이너를 유지하고 packed 본문만 바꿔 decoder 입력을 구분함.
var entry = new FixtureEntry("raw.bin", ZipFixture.Payload(32), true);
var original = ZipFixture.Build(entry);
var nameBytes = Encoding.UTF8.GetByteCount(entry.Name);
var start = 30 + nameBytes;
var central = checked((int)BinaryPrimitives.ReadUInt32LittleEndian(original.AsSpan(original.Length - 6)));
byte[] ReplacePacked(byte[] packed)
{
    var result = original[..start].Concat(packed).Concat(original[central..]).ToArray();
    var cd = start + packed.Length;
    BinaryPrimitives.WriteUInt32LittleEndian(result.AsSpan(18), (uint)packed.Length);
    BinaryPrimitives.WriteUInt32LittleEndian(result.AsSpan(cd + 20), (uint)packed.Length);
    BinaryPrimitives.WriteUInt32LittleEndian(result.AsSpan(result.Length - 6), (uint)cd);
    return result;
}
Add("raw-format", [entry], ReplacePacked([7, 0]));
Add("raw-eof", [entry], ReplacePacked([1, 32, 0, 223, 255, 0, 1]));
Data.WriteNew(output, samples);
return 0;

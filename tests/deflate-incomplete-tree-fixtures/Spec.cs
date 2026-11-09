using System.Security.Cryptography;
using System.Text;
using System.Text.Json;

namespace Zipelo.IncompleteFixtures;

public sealed record Checkpoint(int Offered, string Expected, int OutputLength, int Consumed, int Remaining);
public sealed record FixtureSpec(
    string Id, bool Valid, int DataStartBit, int EndBitExclusive, int RawLength,
    string OutputText, int? FaultStartBit, int? DecisiveEndBit,
    int ErrorConsumedMin, int ErrorConsumedMax, string ErrorPrefix,
    string Reason, Checkpoint[] Checkpoints);
public sealed record BitToken(int StartBit, int BitLength, string Kind, string Bits, string Meaning);
public sealed record TreeSpec(string Name, int[] Lengths, string[] Canonical);
public sealed record EmittedFixture(
    FixtureSpec Spec, string RawHex, string RawSha256, string? ExpectedOutputSha256,
    TreeSpec[] Trees, BitToken[] Tokens);

public static class Specification
{
    public static readonly JsonSerializerOptions Json = new() { PropertyNamingPolicy = JsonNamingPolicy.CamelCase };
    public static byte[] Bytes(string text) => Encoding.ASCII.GetBytes(text);
    public static string Hash(byte[] bytes) => Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant();

    public static FixtureSpec[] Load()
    {
        var path = Path.Combine(AppContext.BaseDirectory, "policy-expectations.json");
        return JsonSerializer.Deserialize<FixtureSpec[]>(File.ReadAllText(path), Json)
            ?? throw new InvalidDataException("기대값을 읽지 못함.");
    }

    public static void Require(bool condition, string message)
    {
        if (!condition) { throw new InvalidDataException(message); }
    }
}

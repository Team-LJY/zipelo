using System.Security.Cryptography;

namespace Zipelo.StreamingExtractionTests;

internal static class Check
{
    internal static void That(bool condition, string message)
    {
        if (!condition) { throw new InvalidDataException(message); }
    }

    internal static void Equal<T>(T expected, T actual) =>
        That(EqualityComparer<T>.Default.Equals(expected, actual), "기대값 불일치");

    internal static void Bytes(byte[] expected, byte[] actual) =>
        That(SHA256.HashData(expected).SequenceEqual(SHA256.HashData(actual)), "바이트 해시 불일치");
}

internal sealed record PeerPlan(Guid Job, string Mode);

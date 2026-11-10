using System.Buffers.Binary;
using Zipelo.Core;

namespace Zipelo.StreamingExtractionTests;

internal static class NativeExitPeer
{
    internal const int WireBytes = 65938;
    internal const int ProofBytes = 96;
    internal static readonly JobLimits Limits = new(1024, 128L * 1024 * 1024,
        16L * 1024 * 1024, TimeSpan.FromSeconds(2), 1);

    internal static bool Case(CasePlan plan) => Evidence.NativeExitCase(plan);

    internal static bool Selected(CasePlan plan) => Evidence.NativeExitSelected(plan);

    // 기존 합성 payload의 여섯 프레임만 작성함.
    internal static byte[] Wire(Guid job, long inputLength)
    {
        Check.That(job != Guid.Empty && inputLength is >= 22 and <= 1024 * 1024, "native 계획 범위 오류");
        using var output = new MemoryStream();
        var config = new byte[64];
        "ZXE2"u8.CopyTo(config);
        BinaryPrimitives.WriteInt32LittleEndian(config.AsSpan(4), 2);
        job.TryWriteBytes(config.AsSpan(8, 16));
        BinaryPrimitives.WriteInt64LittleEndian(config.AsSpan(24), inputLength);
        BinaryPrimitives.WriteInt64LittleEndian(config.AsSpan(32), Limits.MaxProcessedBytes);
        BinaryPrimitives.WriteInt64LittleEndian(config.AsSpan(40), Limits.MaxOutputBytes);
        BinaryPrimitives.WriteInt64LittleEndian(config.AsSpan(48), Limits.MaxEntries);
        BinaryPrimitives.WriteInt64LittleEndian(config.AsSpan(56), Limits.MaxDuration.Ticks);
        output.Write(config);
        ulong sequence = 0;
        void Frame(ushort kind, Guid frameItem, ReadOnlySpan<byte> body)
        {
            var header = new byte[64];
            "ZIP2"u8.CopyTo(header);
            BinaryPrimitives.WriteUInt16LittleEndian(header.AsSpan(4), 2);
            BinaryPrimitives.WriteUInt16LittleEndian(header.AsSpan(6), kind);
            BinaryPrimitives.WriteUInt32LittleEndian(header.AsSpan(8), (uint)body.Length);
            job.TryWriteBytes(header.AsSpan(16, 16));
            frameItem.TryWriteBytes(header.AsSpan(32, 16));
            BinaryPrimitives.WriteUInt64LittleEndian(header.AsSpan(48), sequence++);
            output.Write(header);
            output.Write(body);
        }
        var item = new Guid("00000001-0000-0000-0000-000000000000");
        var payload = ZipFixture.Payload();
        Check.That(payload.Length == 65537, "고정 payload 길이 불일치");
        var length = new byte[8];
        BinaryPrimitives.WriteInt64LittleEndian(length, payload.Length);
        Frame(1, Guid.Empty, []);
        Frame(2, item, length);
        Frame(3, item, payload.AsSpan(0, 65536));
        Frame(3, item, payload.AsSpan(65536));
        Frame(4, item, length);
        Frame(5, Guid.Empty, [1]);
        Check.That(output.Length == 64 + WireBytes, "고정 wire 길이 불일치");
        return output.ToArray();
    }

    internal static void Prepare(CasePlan plan, Guid job, long inputLength, JobLimits limits)
    {
        Check.That(Selected(plan) && limits == Limits, "Linux native no-exit 계약 불일치");
        var payload = ZipFixture.Payload();
        Check.That(plan.Sample.Id == "stored-chunk" && plan.Sample.Entries.Length == 1 &&
            plan.Sample.Entries[0] == new ExpectedEntry("payload.bin", "File", payload.Length,
                Data.Hash(payload), ZipFixture.Crc(payload)), "native fixture 기대값 불일치");
        var options = new FileStreamOptions { Mode = FileMode.CreateNew, Access = FileAccess.Write };
        if (OperatingSystem.IsLinux()) { options.UnixCreateMode = UnixFileMode.UserRead | UnixFileMode.UserWrite; }
        using var file = new FileStream(Path.Combine(plan.RunDirectory, "native-wire.bin"), options);
        file.Write(Wire(job, inputLength));
    }

    internal static bool ProofMatches(byte[] bytes, Guid job, int pid, int parentPid)
    {
        if (bytes.Length != ProofBytes || job == Guid.Empty || pid <= 0 || parentPid <= 0) { return false; }
        uint U32(int offset) => BinaryPrimitives.ReadUInt32LittleEndian(bytes.AsSpan(offset, 4));
        ulong U64(int offset) => BinaryPrimitives.ReadUInt64LittleEndian(bytes.AsSpan(offset, 8));
        var closed = U64(64);
        var alive = U64(72);
        return bytes.AsSpan(0, 4).SequenceEqual("NXP1"u8) && U32(4) == 1 &&
            new Guid(bytes.AsSpan(8, 16)) == job && U32(24) == (uint)pid && U32(28) == (uint)parentPid &&
            U64(40) != 0 && U32(48) == 1 && U32(52) == 0 && U32(56) == WireBytes && U32(60) == 1 &&
            closed > 0 && alive > closed && alive - closed >= 50_000_000 &&
            alive - closed < 12_000_000_000 && U64(80) == 0 && U64(88) == 0;
    }

    private static byte[]? ReadExactFile(string path, int length)
    {
        if (!File.Exists(path)) { return null; }
        var info = new FileInfo(path);
        if (info.Length != length || (info.Attributes & FileAttributes.ReparsePoint) != 0) { return null; }
        var bytes = File.ReadAllBytes(path);
        return bytes.Length == length ? bytes : null;
    }

    internal static void Confirm(CasePlan plan, Observation report, int? pid)
    {
        if (!Case(plan)) { return; }
        var job = Guid.Parse(report.JobId);
        var proof = ReadExactFile(Path.Combine(plan.RunDirectory, "native-proof.bin"), ProofBytes);
        var wire = ReadExactFile(Path.Combine(plan.RunDirectory, "native-wire.bin"), 64 + WireBytes);
        var expected = Wire(job, Convert.FromBase64String(plan.Sample.Base64).LongLength);
        var confirmed = Selected(plan) && report.InjectionConfirmed && pid is not null && proof is not null &&
            ProofMatches(proof, job, pid.Value, Environment.ProcessId) && wire is not null &&
            wire.AsSpan().SequenceEqual(expected) && report.Error == "BudgetExceeded" &&
            report.WorkerExitConfirmed == true && report.WorkerExitCode is not (null or 0 or 71) &&
            !report.Published && report.CleanupConfirmed && report.IdentityConfirmed;
        report.InjectionConfirmed &= confirmed;
        if (!confirmed && !report.StopRequired)
        {
            report.Verdict = "blocked";
            report.Detail = OperatingSystem.IsWindows() ? "native_no_exit_windows_unverified" :
                "native_no_exit_proof_unconfirmed";
        }
        Data.WriteNew(Path.Combine(plan.RunDirectory, "native-evidence.json"), new
        {
            job, hostPid = Environment.ProcessId, workerPid = pid, invocation = plan.Invocation,
            confirmed, proofSha256 = proof is null ? null : Data.Hash(proof),
            wireSha256 = wire is null ? null : Data.Hash(wire),
            scope = "Linux helper self-observation plus frozen host EOF/exit/deadline/cleanup trace"
        });
    }
}

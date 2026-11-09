using Zipelo.Core;

namespace Zipelo.SecurityTests;

internal sealed partial class CoreTests
{
    private static void PlatformNames()
    {
        foreach (var name in new[] { "CON", "name:stream", "name.", "name ", "e\u0301.txt", "line\nfeed", "a?b" })
        {
            Check.Equal(ArchiveError.UnsafePath,
                Check.Throws<ArchiveException>(() => OutputPathPolicy.Validate(name, FileNamePolicy.Windows)).Code);
            Check.Equal(name, string.Join('/', OutputPathPolicy.Validate(name, FileNamePolicy.Linux)));
        }
        Check.Equal(1, OutputPathPolicy.Validate("back\\slash", FileNamePolicy.Linux).Count);
        Check.Equal(2, OutputPathPolicy.Validate("back\\slash", FileNamePolicy.Windows).Count);
        foreach (var policy in new[] { FileNamePolicy.Windows, FileNamePolicy.Linux })
        {
            foreach (var name in new[] { "../escape", "/absolute", "a//b", "a/./b", "bad\0name" })
            {
                Check.Equal(ArchiveError.UnsafePath,
                    Check.Throws<ArchiveException>(() => OutputPathPolicy.Validate(name, policy)).Code);
            }
        }
        Check.Equal(255, OutputPathPolicy.Validate(new string('a', 255), FileNamePolicy.Linux)[0].Length);
        Check.Throws<ArchiveException>(() => OutputPathPolicy.Validate(new string('가', 86), FileNamePolicy.Linux));
        Check.Throws<ArchiveException>(() => OutputPathPolicy.Validate("\ud800", FileNamePolicy.Linux));
        Check.That(OutputPathPolicy.Comparer(FileNamePolicy.Windows).Equals("a", "A"), "Windows 충돌을 구분하지 못함");
        Check.That(!OutputPathPolicy.Comparer(FileNamePolicy.Linux).Equals("a", "A"), "Linux 이름을 합침");
    }

    private static async Task PlatformCaseOutputs()
    {
        var run = await Execute(Stub(async context =>
        {
            await Write(context, "Readme.txt", [1]);
            await Write(context, "README.TXT", [2]);
        }));
        if (OperatingSystem.IsWindows())
        {
            Failed(run, ArchiveError.OutputConflict);
            Check.Equal(1, run.Output.CreateCalls);
            return;
        }
        Check.Equal(JobState.Completed, run.Result.State);
        Check.Equal(ArchiveError.None, run.Result.Error);
        Check.Equal(2, run.Output.Published.Count);
        Check.That(run.Output.Published["Readme.txt"].SequenceEqual(new byte[] { 1 }), "첫 항목이 변경됨");
        Check.That(run.Output.Published["README.TXT"].SequenceEqual(new byte[] { 2 }), "둘째 항목이 변경됨");
    }
}

using Zipelo.SecurityTests;

namespace Zipelo.RuntimeTests;

internal static class Program
{
    public static async Task<int> Main(string[] arguments)
    {
        // 인수 없는 자식 실행은 프로토콜 실패 주입 전용임.
        if (arguments.Length == 0) { return await ProtocolPeer.Run(); }
        if (arguments.Length != 2) { return 2; }
        var root = Path.GetFullPath(arguments[0]);
        var sandbox = Path.GetFullPath(arguments[1]);
        var work = Path.Combine(root, "tests", "security", ".work");
        if (!File.Exists(Path.Combine(root, "src", "Zipelo.Runtime", "Zipelo.Runtime.csproj")) ||
            !sandbox.StartsWith(work + Path.DirectorySeparatorChar, StringComparison.Ordinal) ||
            !Directory.Exists(sandbox))
        {
            return 2;
        }
        for (DirectoryInfo? directory = new(sandbox); directory is not null; directory = directory.Parent)
        {
            if ((directory.Attributes & FileAttributes.ReparsePoint) != 0) { return 2; }
        }
        var tests = new RuntimeTests(root, sandbox);
        var passed = 0;
        var failed = 0;
        var skipped = 0;
        foreach (var (name, run) in tests.Cases())
        {
            try
            {
                await run();
                Console.WriteLine($"PASS {name}");
                passed++;
            }
            catch (CaseSkippedException exception)
            {
                Console.WriteLine($"SKIP {name}: {exception.Message}");
                skipped++;
            }
            catch (Exception exception)
            {
                Console.Error.WriteLine($"FAIL {name}: {exception.GetType().Name}");
                failed++;
            }
        }
        Console.WriteLine($"런타임 회귀 검사: {passed} 통과, {failed} 실패, {skipped} 미실행");
        Console.WriteLine($"실행 환경: {System.Runtime.InteropServices.RuntimeInformation.OSDescription}, " +
            System.Runtime.InteropServices.RuntimeInformation.ProcessArchitecture);
        return failed == 0 ? 0 : 1;
    }
}

internal sealed class CaseSkippedException(string message) : Exception(message) { }

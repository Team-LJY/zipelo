namespace Zipelo.SecurityTests;

internal static class Program
{
    public static async Task<int> Main(string[] arguments)
    {
        if (arguments.Length != 2)
        {
            Console.Error.WriteLine("tests/run_checks.py --core로 실행해야 함");
            return 2;
        }

        var root = Path.GetFullPath(arguments[0]);
        var sandbox = Path.GetFullPath(arguments[1]);
        var work = Path.Combine(root, "tests", "security", ".work");
        if (!File.Exists(Path.Combine(root, "src", "Zipelo.Core", "Zipelo.Core.csproj")) ||
            !sandbox.StartsWith(work + Path.DirectorySeparatorChar, StringComparison.Ordinal) ||
            !Directory.Exists(sandbox))
        {
            Console.Error.WriteLine("프로젝트 안의 테스트 임시 영역이 필요함");
            return 2;
        }

        for (DirectoryInfo? directory = new DirectoryInfo(sandbox); directory is not null; directory = directory.Parent)
        {
            if ((directory.Attributes & FileAttributes.ReparsePoint) != 0)
            {
                Console.Error.WriteLine("테스트 경로의 링크를 허용하지 않음");
                return 2;
            }
        }

        var tests = new CoreTests(root, sandbox);
        var passed = 0;
        var failed = 0;
        foreach (var (name, run) in tests.Cases())
        {
            try
            {
                await run();
                Console.WriteLine($"PASS {name}");
                passed++;
            }
            catch (Exception exception)
            {
                // 예외 원문과 테스트 암호를 출력하지 않음.
                Console.Error.WriteLine($"FAIL {name}: {exception.GetType().Name}");
                failed++;
            }
        }

        Console.WriteLine($"코어 회귀 검사: {passed} 통과, {failed} 실패");
        Console.WriteLine("미실행: 실제 엔진, Windows 링크/junction, 저장 계층 경합, 미리보기 실행 및 통신 관찰");
        return failed == 0 ? 0 : 1;
    }
}

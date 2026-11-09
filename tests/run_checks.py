import argparse
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
TESTS = ROOT / "tests"
SECURITY = TESTS / "security"
BUILD_TIMEOUT = 120
CORE_TIMEOUT = 60


def python_checks():
    suite = unittest.TestSuite()
    for index, path in enumerate(sorted(TESTS.rglob("check_*.py"))):
        if any(part in (".work", "bin", "obj", "__pycache__") for part in path.relative_to(TESTS).parts):
            continue
        spec = importlib.util.spec_from_file_location(f"zipelo_checks_{index}", path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        suite.addTests(unittest.defaultTestLoader.loadTestsFromModule(module))
    return unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful()


def check_work_path(path):
    work = SECURITY / ".work"
    if ".." in path.parts or (path != work and not path.is_relative_to(work)):
        raise ValueError("프로젝트 안의 테스트 임시 영역이 필요함")
    for ancestor in (path, *path.parents):
        if ancestor.is_symlink() or (hasattr(ancestor, "is_junction") and ancestor.is_junction()):
            raise ValueError("테스트 작업 경로의 링크를 허용하지 않음")


def core_checks():
    return csharp_checks("security", "Zipelo.SecurityTests", CORE_TIMEOUT)


def runtime_checks():
    return csharp_checks("runtime", "Zipelo.RuntimeTests", 120)


def csharp_checks(folder, assembly_name, timeout):
    dotnet = shutil.which("dotnet")
    if dotnet is None:
        print("미실행: .NET 10 SDK가 PATH에 없어 C# 검사를 빌드하거나 실행하지 못함", flush=True)
        return 2

    work = SECURITY / ".work"
    check_work_path(work)
    work.mkdir(exist_ok=True)

    # 빌드 결과, SDK 캐시와 실행 입력을 이번 테스트 소유 폴더에만 둠.
    with tempfile.TemporaryDirectory(prefix="run-", dir=work) as temporary:
        sandbox = Path(temporary)
        check_work_path(sandbox)
        environment = os.environ.copy()
        for variable, child in (
            ("DOTNET_CLI_HOME", "dotnet-home"),
            ("NUGET_PACKAGES", "nuget-packages"),
            ("NUGET_HTTP_CACHE_PATH", "nuget-http-cache"),
            ("TMPDIR", "temp"),
            ("TMP", "temp"),
            ("TEMP", "temp"),
        ):
            location = sandbox / child
            location.mkdir(exist_ok=True)
            environment[variable] = str(location)
        environment["DOTNET_CLI_TELEMETRY_OPTOUT"] = "1"
        environment["DOTNET_SKIP_FIRST_TIME_EXPERIENCE"] = "1"
        environment["DOTNET_CLI_WORKLOAD_UPDATE_NOTIFY_DISABLE"] = "true"
        binary_directory = sandbox / "bin"
        build = [
            dotnet,
            "build",
            str(TESTS / folder / f"{assembly_name}.csproj"),
            "--configuration", "Release",
            "--artifacts-path", str(sandbox / "artifacts"),
            "--output", str(binary_directory),
            "--configfile", str(SECURITY / "NuGet.Config"),
            "--disable-build-servers",
            "-p:UseAppHost=false",
        ]
        print(f"C# 테스트 빌드: tests/{folder}/{assembly_name}.csproj", flush=True)
        try:
            built = subprocess.run(
                build, cwd=sandbox, env=environment, check=False, timeout=BUILD_TIMEOUT
            )
            if built.returncode != 0:
                return built.returncode

            assembly = binary_directory / f"{assembly_name}.dll"
            check_work_path(assembly)
            if not assembly.is_file():
                print("실행 실패: 빌드된 테스트 DLL이 없음", flush=True)
                return 2

            if folder == "runtime":
                for name in ("Zipelo.Worker.dll", "Zipelo.Worker.deps.json", "Zipelo.Worker.runtimeconfig.json"):
                    artifact = binary_directory / name
                    check_work_path(artifact)
                    if not artifact.is_file():
                        print(f"실행 실패: 작업자 빌드 파일이 없음: {name}", flush=True)
                        return 2

            # 빌드 옵션을 앱 인수로 넘기지 않고 루트와 임시 경로만 전달함.
            command = [dotnet, str(assembly), str(ROOT), str(sandbox)]
            print(f"{assembly_name} DLL 실행", flush=True)
            completed = subprocess.run(
                command, cwd=sandbox, env=environment, check=False, timeout=timeout
            )
            return completed.returncode
        except subprocess.TimeoutExpired:
            print("실행 실패: C# 빌드 또는 테스트 제한 시간을 초과함", flush=True)
            return 124


def main():
    parser = argparse.ArgumentParser(description="프로젝트 루트 tests/의 검사를 실행함")
    parser.add_argument("--core", action="store_true", help=".NET 10 코어 회귀 검사도 실행함")
    parser.add_argument("--runtime", action="store_true", help="실제 ZIP 작업자와 프로토콜 회귀 검사도 실행함")
    options = parser.parse_args()
    sys.dont_write_bytecode = True
    if not python_checks():
        return 1
    for enabled, run in ((options.core, core_checks), (options.runtime, runtime_checks)):
        if enabled:
            code = run()
            if code != 0:
                return code
    if not options.core:
        print("미실행: 코어 회귀 검사. .NET 10 SDK 환경에서 --core로 실행해야 함", flush=True)
    if not options.runtime:
        print("미실행: 런타임 회귀 검사. .NET 10 SDK 환경에서 --runtime으로 실행해야 함", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

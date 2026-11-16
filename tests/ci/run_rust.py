import argparse
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[2]
TOOLCHAIN = "+1.99.1"
WINDOWS_TARGET = "x86_64-pc-windows-gnu"
LINUX_ONLY = {"rust-ui-system-font-regressions"}
JOB_NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9_-]{0,63}\Z")


def work_directory(root, name):
    if not JOB_NAME.fullmatch(name):
        raise ValueError("작업명은 영숫자로 시작하는 영숫자, 밑줄, 하이픈만 허용함")
    common = Path(subprocess.check_output(
        ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
        cwd=root, text=True,
    ).strip()).resolve()
    project = common.parent if common.name == ".git" else root.resolve()
    tests = project / "tests"
    base = tests / "work"
    if tests.is_symlink() or not tests.is_dir():
        raise ValueError(f"안전한 작업 디렉터리가 필요함: {tests}")
    if base.is_symlink() or (base.exists() and not base.is_dir()):
        raise ValueError(f"안전한 작업 디렉터리가 필요함: {base}")
    base.mkdir(mode=0o700, exist_ok=True)
    if base.resolve() != base:
        raise ValueError(f"안전한 작업 디렉터리가 필요함: {base}")
    path = base / name
    if path.is_symlink() or (path.exists() and not path.is_dir()):
        raise ValueError(f"안전한 작업 디렉터리가 필요함: {path}")
    path.mkdir(mode=0o700, exist_ok=True)
    if path.resolve() != path:
        raise ValueError(f"안전한 작업 디렉터리가 필요함: {path}")
    return path


def output_directory(work, name, create=False):
    path = work / name
    if not path.resolve().is_relative_to(work) or (path.exists() and not path.is_dir()):
        raise ValueError(f"작업 폴더 밖의 출력 경로는 허용하지 않음: {path}")
    if create:
        path.mkdir(mode=0o700, exist_ok=True)
    return path


def test_work_directory(root):
    configured = os.environ.get("ZIPELO_TEST_WORK_DIR")
    if configured:
        return Path(configured)
    return work_directory(root, os.environ.get("ZIPELO_CI_JOB", "rust-local"))




def manifests(root):
    paths = sorted(root.glob("experiments/*/Cargo.toml"))
    if not paths:
        raise ValueError("experiments/ Rust crate를 찾지 못함")
    paths += sorted(root.glob("tests/*/Cargo.toml"))
    for path in paths:
        if not path.with_name("Cargo.lock").is_file():
            raise ValueError(f"고정 Cargo.lock이 없음: {path}")
    return paths


def command(manifest, phase):
    arguments = [
        "cargo",
        TOOLCHAIN,
        "build" if phase == "cross-build" else phase,
        "--manifest-path",
        str(manifest),
        "--locked",
        "--all-targets",
        "--all-features",
    ]
    if phase == "cross-build":
        arguments += ["--target", WINDOWS_TARGET]
    if phase == "test":
        arguments += ["--", "--test-threads=1"]
    return arguments


def prepare_fixtures(root):
    skipped = {}
    work = test_work_directory(root)
    if work.resolve() != work:
        raise ValueError(f"안전한 작업 디렉터리가 필요함: {work}")
    work.mkdir(mode=0o700, parents=True, exist_ok=True)
    output_directory(work, "fixtures", create=True)
    return skipped


def run(root, phase):
    paths = manifests(root)
    skipped = {}
    if phase == "test" and sys.platform == "linux":
        skipped = prepare_fixtures(root)
    results = []
    for manifest in paths:
        relative = manifest.relative_to(root).as_posix()
        if manifest.parent.parent.name == "tests" and manifest.parent.name in skipped:
            reason = skipped[manifest.parent.name]
            results.append({
                "manifest": relative,
                "status": "not-applicable",
                "reason": reason,
            })
            print(f"NOT APPLICABLE: {relative} ({reason})", flush=True)
            continue
        if (
            phase == "test"
            and sys.platform != "linux"
            and manifest.parent.parent.name == "tests"
            and manifest.parent.name in LINUX_ONLY
        ):
            results.append({
                "manifest": relative,
                "status": "not-applicable",
                "reason": "Linux-only cfg",
            })
            print(f"NOT APPLICABLE: {relative} (Linux-only cfg)", flush=True)
            continue
        print(f"::group::{phase}: {relative}", flush=True)
        completed = subprocess.run(command(manifest, phase), cwd=root, check=False)
        results.append({
            "manifest": relative,
            "status": "passed" if completed.returncode == 0 else "failed",
            "exit_code": completed.returncode,
        })
        print("::endgroup::", flush=True)
    return results


def prepare_tools(root):
    configured = os.environ.get("ZIPELO_CI_TOOLS")
    if configured:
        directories = [Path(configured).resolve()]
    else:
        directories = [test_work_directory(root) / "tools"]
        common = Path(subprocess.check_output(
            ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
            cwd=root, text=True,
        ).strip())
        # 같은 저장소의 기본 worktree에 준비한 도구를 재사용함.
        if common.name == ".git":
            directories.append(common.parent / "tests/ci/tools")
    for tools in directories:
        if (tools / "rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu").is_dir():
            os.environ.setdefault("RUSTUP_HOME", str(tools / "rustup"))
    bins = [str(tools / "mingw/usr/bin") for tools in directories
            if (tools / "mingw/usr/bin").is_dir()]
    if bins:
        os.environ["PATH"] = os.pathsep.join(bins + [os.environ.get("PATH", "")])


def prepare_cross_build():
    for variable, names in (
        ("CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER", [
            "x86_64-w64-mingw32-gcc", "x86_64-w64-mingw32-gcc-posix",
        ]),
        ("CC_x86_64_pc_windows_gnu", [
            "x86_64-w64-mingw32-gcc", "x86_64-w64-mingw32-gcc-posix",
        ]),
        ("CXX_x86_64_pc_windows_gnu", [
            "x86_64-w64-mingw32-g++", "x86_64-w64-mingw32-g++-posix",
        ]),
    ):
        candidate = os.environ.get(variable)
        if not candidate:
            candidate = next((shutil.which(name) for name in names if shutil.which(name)), None)
        if not candidate or not shutil.which(candidate):
            raise ValueError(
                f"Windows 교차 컴파일 도구가 없음: {variable}. "
                "bash tests/ci/setup_windows.sh 실행 또는 ZIPELO_CI_TOOLS 지정이 필요함"
            )
        os.environ[variable] = candidate


def prepare_local_tools(root):
    try:
        prepare_cross_build()
    except ValueError:
        print("Windows 교차 도구를 프로젝트 내부에 준비함: tests/ci/setup_windows.sh", flush=True)
        environment = dict(os.environ, ZIPELO_CI_TOOLS=str(test_work_directory(root) / "tools"))
        subprocess.run(["bash", str(root / "tests/ci/setup_windows.sh")],
                       cwd=root, env=environment, check=True)
        os.environ["ZIPELO_CI_TOOLS"] = environment["ZIPELO_CI_TOOLS"]
        prepare_tools(root)
        prepare_cross_build()


def execute_phases(root, phases):
    results = []
    for phase in phases:
        try:
            if phase == "cross-build":
                prepare_cross_build()
            rows = run(root, phase)
        except (OSError, ValueError, subprocess.CalledProcessError) as error:
            rows = [{"manifest": "preflight", "status": "failed", "reason": str(error)}]
        for row in rows:
            results.append({"phase": phase, **row})
    return results


def main():
    parser = argparse.ArgumentParser(description="로컬 Linux 시험과 Windows 교차 빌드")
    parser.add_argument("phase", choices=["local", "build", "test", "cross-build"])
    parser.add_argument("--job", default=os.environ.get("ZIPELO_CI_JOB", "rust-local"))
    args = parser.parse_args()
    if args.phase == "local" and sys.platform != "linux":
        parser.error("local 모드는 Linux에서 실행해야 함")
    work = work_directory(ROOT, args.job)
    temporary = output_directory(work, "tmp", create=True)
    target = output_directory(work, "target")
    os.environ["ZIPELO_CI_JOB"] = args.job
    os.environ["ZIPELO_TEST_WORK_DIR"] = str(work)
    os.environ["TMPDIR"] = str(temporary)
    os.environ["TMP"] = str(temporary)
    os.environ["TEMP"] = str(temporary)
    os.environ["PYTHONDONTWRITEBYTECODE"] = "1"
    os.environ.setdefault("CARGO_BUILD_JOBS", "2")
    os.environ["CARGO_TARGET_DIR"] = str(target)
    prepare_tools(ROOT)
    phases = ["build", "test", "cross-build"] if args.phase == "local" else [args.phase]
    checks = subprocess.run(
        [sys.executable, "-m", "unittest", "discover", "-s", "tests/ci", "-p", "test_*.py"],
        cwd=ROOT, check=False,
    )
    if checks.returncode != 0:
        results = [{
            "phase": "self-check",
            "manifest": "tests/ci",
            "status": "failed",
            "exit_code": checks.returncode,
        }]
    else:
        try:
            if args.phase == "local":
                prepare_local_tools(ROOT)
        except (OSError, ValueError, subprocess.CalledProcessError) as error:
            results = [{
                "phase": "tool-setup", "manifest": "preflight",
                "status": "failed", "reason": str(error),
            }]
        else:
            results = execute_phases(ROOT, phases)
    report = {
        "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "platform": sys.platform,
        "windows_cross_target": WINDOWS_TARGET if "cross-build" in phases else None,
        "windows_runtime_tested": checks.returncode == 0 and sys.platform == "win32" and "test" in phases,
        "results": results,
    }
    (work / f"{args.phase}-results.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=4) + "\n", encoding="utf-8"
    )
    lines = []
    for phase in phases:
        selected = [row for row in results if row["phase"] == phase]
        counts = {status: sum(row["status"] == status for row in selected)
                  for status in ("passed", "failed", "not-applicable")}
        lines.append(f"{phase}: " + ", ".join(f"{key}={value}" for key, value in counts.items()))
    for row in results:
        if row["status"] == "failed":
            lines.append(f"FAIL {row['phase']} {row['manifest']}: {row.get('reason', row.get('exit_code'))}")
    if "cross-build" in phases:
        lines.append("Windows: cross-build only; runtime tests not executed")
    print("\n".join(lines), flush=True)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as output:
            output.write("\n" + "\n\n".join(lines) + "\n")
    return checks.returncode or int(any(result["status"] == "failed" for result in results))


if __name__ == "__main__":
    raise SystemExit(main())

import argparse
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[2]
TOOLCHAIN = "+1.99.1"
WINDOWS_TARGET = "x86_64-pc-windows-gnu"
LINUX_ONLY = {
    "rust-file-publication",
    "rust-file-snapshot",
    "rust-gzip-file-job",
    "rust-ui-system-font-regressions",
}
FILE_FIXTURES = LINUX_ONLY - {"rust-ui-system-font-regressions"}
PUBLICATION_FIXTURES = {"rust-file-publication", "rust-gzip-file-job"}


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
    for name in sorted(FILE_FIXTURES):
        path = root / "tests" / name / "runs"
        if not path.exists() and not path.is_symlink():
            path.mkdir(mode=0o700)
        info = path.lstat()
        if (
            not stat.S_ISDIR(info.st_mode)
            or stat.S_IMODE(info.st_mode) != 0o700
            or info.st_uid != os.geteuid()
            or path.resolve() != path
            or any(path.iterdir())
        ):
            raise ValueError(f"비어 있는 소유 시험 디렉터리가 필요함: {path}")
        if name in PUBLICATION_FIXTURES:
            filesystem = subprocess.check_output(
                ["stat", "-f", "-c", "%t", "--", str(path)], text=True
            ).strip().lower()
            if filesystem not in {"ef53", "58465342"}:
                skipped[name] = (
                    f"파일 게시 시험은 ext4/XFS가 필요함: filesystem=0x{filesystem}"
                    " (tmpfs 포함 다른 파일시스템은 제품 코드에서 지원하지 않음)"
                )
                continue
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
        directories = [root / "tests/ci/tools"]
        common = Path(subprocess.check_output(
            ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
            cwd=root, text=True,
        ).strip())
        # 같은 저장소의 기본 worktree에 준비한 도구만 재사용함.
        if common.name == ".git" and common.parent != root:
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
        subprocess.run(["bash", str(root / "tests/ci/setup_windows.sh")], cwd=root, check=True)
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
    args = parser.parse_args()
    if args.phase == "local" and sys.platform != "linux":
        parser.error("local 모드는 Linux에서 실행해야 함")
    prepare_tools(ROOT)
    os.environ.setdefault("CARGO_BUILD_JOBS", "2")
    os.environ.setdefault("CARGO_TARGET_DIR", str(ROOT / "tests/ci/target"))
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
    (ROOT / f"tests/ci/{args.phase}-results.json").write_text(
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

import argparse
import json
import os
from pathlib import Path
import stat
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[2]
TOOLCHAIN = "+1.99.1"
LINUX_ONLY = {
    "rust-file-publication",
    "rust-file-snapshot",
    "rust-gzip-file-job",
    "rust-ui-system-font-regressions",
}
FILE_FIXTURES = LINUX_ONLY - {"rust-ui-system-font-regressions"}


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
        phase,
        "--manifest-path",
        str(manifest),
        "--locked",
        "--all-targets",
        "--all-features",
    ]
    if phase == "test":
        arguments += ["--", "--test-threads=1"]
    return arguments


def prepare_fixtures(root):
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


def run(root, phase):
    paths = manifests(root)
    if phase == "test" and sys.platform == "linux":
        prepare_fixtures(root)
    results = []
    for manifest in paths:
        relative = manifest.relative_to(root).as_posix()
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


def main():
    parser = argparse.ArgumentParser(description="Rust crate 전체 빌드와 tests/ 회귀 실행")
    parser.add_argument("phase", choices=["build", "test"])
    args = parser.parse_args()
    os.environ.setdefault("CARGO_BUILD_JOBS", "1")
    os.environ.setdefault("CARGO_TARGET_DIR", str(ROOT / "tests/ci/target"))
    results = run(ROOT, args.phase)
    (ROOT / f"tests/ci/{args.phase}-results.json").write_text(
        json.dumps(results, ensure_ascii=False, indent=4) + "\n", encoding="utf-8"
    )
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as output:
            output.write(f"\nRust {args.phase} ({sys.platform})\n\n")
            for result in results:
                output.write(f"- {result['manifest']}: {result['status']}\n")
            output.write("\nGUI harness는 컴파일만 확인하며 실제 화면 시나리오는 실행하지 않음.\n")
    return int(any(result["status"] == "failed" for result in results))


if __name__ == "__main__":
    raise SystemExit(main())

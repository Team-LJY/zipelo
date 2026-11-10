import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
BASE_HEAD = "63b80e3158c08b71ec70666a2a39216cd36752b1"
BASE_DIGEST = "7316761d72c18e3dc6f89135ca15272f44aa814e493b2083bc278081dd5006f8"
BASE_MANIFEST = "tests/rust-ui-probe/SHA256SUMS"
REDUCER_MANIFEST = "tests/rust-ui-operation-reducer/SHA256SUMS"
BASE_PATHS = frozenset({'experiments/rust-ui-probe/src/lib.rs', 'tests/rust-ui-probe/dependency-inventory.json', 'tests/rust-ui-probe/app.rs', 'tests/rust-ui-probe/dependency_audit.py', 'tests/rust-ui-probe/license-supplement.json', 'experiments/rust-ui-probe/Cargo.lock', 'tests/rust-ui-probe/contract.json', 'experiments/rust-ui-probe/Cargo.toml', 'tests/rust-ui-probe/model.rs'})
REDUCER_PATHS = frozenset({'experiments/rust-ui-operation-reducer/LICENSE', 'experiments/rust-ui-operation-reducer/src/lib.rs', 'experiments/rust-ui-operation-reducer/src/listing.rs', 'tests/rust-ui-operation-reducer/strict_tests.py', 'tests/rust-ui-operation-reducer/overflow.rs', 'tests/rust-ui-operation-reducer/contract.json', 'tests/rust-ui-operation-reducer/reducer.rs', 'experiments/rust-ui-operation-reducer/Cargo.lock', 'experiments/rust-ui-operation-reducer/Cargo.toml', 'tests/rust-ui-operation-reducer/validate.py'})


def digest(data):
    return hashlib.sha256(data).hexdigest()


def check_hex(value, size):
    if re.fullmatch(r"[0-9a-f]{" + str(size) + r"}", value) is None:
        raise ValueError("잘못된 고정 hash")


def checked_path(root, name):
    path = PurePosixPath(name)
    if path.is_absolute() or str(path) != name or any(x in ("", ".", "..") for x in name.split("/")) or "\\" in name:
        raise ValueError("잘못된 상대 경로")
    current = root
    for part in path.parts:
        current = current / part
        if current.is_symlink():
            raise ValueError("symlink 경로 거부")
    if not current.is_file():
        raise ValueError("필수 파일 누락")
    return current


def parse_manifest(data, expected_paths):
    text = data.decode("ascii")
    if not text.endswith("\n"):
        raise ValueError("manifest 마지막 개행 누락")
    result = {}
    for line in text.splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_./-]+)", line)
        if match is None:
            raise ValueError("잘못된 manifest 행")
        value, name = match.groups()
        path = PurePosixPath(name)
        if path.is_absolute() or str(path) != name or any(p in ("", ".", "..") for p in name.split("/")):
            raise ValueError("잘못된 manifest 경로")
        if name in result:
            raise ValueError("manifest 경로 중복")
        result[name] = value
    if set(result) != set(expected_paths):
        raise ValueError("manifest 경로 누락 또는 추가")
    return result


def verify_manifest(root, relative, expected_paths, expected_digest):
    check_hex(expected_digest, 64)
    path = checked_path(root, relative)
    data = path.read_bytes()
    entries = parse_manifest(data, expected_paths)
    if digest(data) != expected_digest:
        raise ValueError("고정 manifest digest 불일치")
    for name, expected in entries.items():
        if digest(checked_path(root, name).read_bytes()) != expected:
            raise ValueError("파일 digest 불일치: " + name)
    return entries


def preflight(root, reducer_digest):
    baseline = verify_manifest(root, BASE_MANIFEST, BASE_PATHS, BASE_DIGEST)
    reducer = verify_manifest(root, REDUCER_MANIFEST, REDUCER_PATHS, reducer_digest)
    paths = set(baseline) | set(reducer) | {BASE_MANIFEST, REDUCER_MANIFEST}
    return {name: digest(checked_path(root, name).read_bytes()) for name in sorted(paths)}


def run(root, artifacts, name, args):
    env = dict(os.environ, CARGO_TARGET_DIR=str(artifacts / "target"))
    result = subprocess.run(args, cwd=root, env=env, text=True, capture_output=True)
    (artifacts / (name + ".stdout")).write_text(result.stdout)
    (artifacts / (name + ".stderr")).write_text(result.stderr)
    evidence = {"command": args, "exit": result.returncode}
    if result.returncode:
        raise RuntimeError(json.dumps(evidence) + "\n" + result.stdout + result.stderr)
    return result.stdout, evidence


def main(argv=None, root=ROOT):
    if sys.flags.optimize:
        raise RuntimeError("최적화 모드에서는 검증을 실행할 수 없음")
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-head", required=True)
    parser.add_argument("--reducer-manifest-sha256", required=True)
    args = parser.parse_args(argv)
    check_hex(args.source_head, 40)
    # Cargo 및 산출물 생성 전에 고정 기대값과 모든 파일을 확인함.
    before = preflight(root, args.reducer_manifest_sha256)
    here = root / "tests/rust-ui-operation-reducer"
    artifacts = here / "artifacts"
    artifacts.mkdir(exist_ok=True)
    unit_names = re.findall(r"#\[test\]\s+fn (\w+)\(", (here / "overflow.rs").read_text())
    api_names = re.findall(r"#\[test\]\s+fn (\w+)\(", (here / "reducer.rs").read_text())
    expected = sorted(["overflow::" + name for name in unit_names] + api_names)
    assert len(expected) == len(set(expected)) == 67
    base = ["cargo", "+1.99.1"]
    flags = ["--locked", "--offline", "--manifest-path", "experiments/rust-ui-operation-reducer/Cargo.toml"]
    _, build = run(root, artifacts, "build", base + ["build", "--lib"] + flags)
    listed, discovery = run(root, artifacts, "discovery", base + ["test", "--lib", "--test", "operation_reducer"] + flags + ["--", "--list"])
    actual = sorted(re.findall(r"^([^\s]+): test$", listed, re.M))
    assert actual == expected
    output, execution = run(root, artifacts, "execution", base + ["test", "--lib", "--test", "operation_reducer"] + flags + ["--", "--test-threads=1"])
    passed = sorted(re.findall(r"^test ([^\s]+) \.\.\. ok$", output, re.M))
    assert passed == expected
    summaries = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out", output)
    assert sorted(summaries) == sorted([("7", "0", "0", "0", "0"), ("60", "0", "0", "0", "0")])
    after = preflight(root, args.reducer_manifest_sha256)
    assert before == after
    report = {"scope": "작성자 측 순수 모델 검증임", "source_head_caller_supplied": args.source_head,
        "source_head_limit": "고정 commit과 manifest digest는 호출자가 별도 신뢰 경로에서 확인해야 함. helper는 Git tree provenance를 독립 입증하지 않음",
        "helper_sha256": digest(Path(__file__).read_bytes()),
        "manifest_trust": {"baseline_head": BASE_HEAD, "baseline_expected_sha256": BASE_DIGEST, "reducer_external_expected_sha256": args.reducer_manifest_sha256},
        "build": build, "discovery": discovery, "execution": execution,
        "discovered": len(actual), "executed": len(passed), "passed": len(passed), "failed": 0,
        "ignored": 0, "filtered": 0, "existing_tests": 58, "added_tests": 9,
        "names": actual, "hashes_before": before, "hashes_after": after}
    (artifacts / "validation-report.json").write_text(json.dumps(report, ensure_ascii=False, indent=4) + "\n")
    print(json.dumps({"discovered": len(actual), "executed": len(passed), "passed": len(passed), "summaries": summaries}))


if __name__ == "__main__":
    main()

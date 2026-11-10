import argparse
import hashlib
import json
import pathlib
import re
import stat
import sys
import tomllib

BASE = pathlib.Path(__file__).resolve().parent
ROOT = BASE.parents[1]
SOURCE = ROOT / "experiments/rust-gzip-file-job"
HEAD = "4c6ae4b3a7ab12a992598e94e7b02f3a167fbbd4"
TREE = "9a01a37623b163c35ca4ff25963b2e11c61a0c35"
SOURCE_MANIFEST = "ef32ad5306d2d98751836846f631f66c562e4b55caf5d3422e120305aa78b37f"
MODULES = ("success", "invalid", "limits", "control", "namespace", "concurrency")
FILES = {
    "Cargo.toml", "Cargo.lock", "main.rs", "support.rs", "fixtures.rs",
    "success.rs", "invalid.rs", "limits.rs", "control.rs", "namespace.rs", "concurrency.rs",
    "source-pins.json", "contract.json", "run-plan.json", "static_check.py",
    "static_check_helper_tests.py",
}
GUARD = "최적화 모드에서는 정적 검사를 실행할 수 없음"


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def parse_manifest(text, expected_paths):
    rows = {}
    for line in text.splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_./-]+)", line)
        require(match is not None, "잘못된 manifest 행")
        value, name = match.groups()
        path = pathlib.PurePosixPath(name)
        require(not path.is_absolute() and ".." not in path.parts
                and str(path) == name and name not in rows, "중복 또는 잘못된 경로")
        rows[name] = value
    require(set(rows) == set(expected_paths), "manifest 경로 불일치")
    return rows


def hashes():
    result = {}
    for name in sorted(FILES | {"SHA256SUMS"}):
        path = BASE / name
        require(path.resolve() == path and stat.S_ISREG(path.lstat().st_mode), "일반 시험 파일 필요")
        require(not path.stat().st_mode & 0o111, "시험 파일 mode 불일치")
        result[name] = digest(path.read_bytes())
    return result


def verify_sources():
    pins = json.loads((BASE / "source-pins.json").read_text())
    require(pins["head"] == HEAD and pins["tree"] == TREE and len(pins["files"]) == 39, "고정 source 기준 불일치")
    require(len({row["path"] for row in pins["files"]}) == 39, "source 경로 중복")
    roots = ("experiments/rust-gzip-file-job/", "experiments/rust-file-snapshot/",
             "experiments/rust-file-publication/", "experiments/rust-memory-gzip-reader/")
    result = {}
    for row in pins["files"]:
        path = ROOT / row["path"]
        require(row["path"].startswith(roots) and path.resolve() == path, "source 경로 이탈")
        info = path.lstat()
        require(stat.S_ISREG(info.st_mode) and not info.st_mode & 0o111, "source mode 불일치")
        raw = path.read_bytes()
        require(digest(raw) == row["sha256"], "source SHA256 불일치")
        blob = hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest()
        require(blob == row["git_blob"] and row["mode"] == "100644", "source blob 불일치")
        result[row["path"]] = digest(raw)
    require(digest((SOURCE / "SHA256SUMS").read_bytes()) == SOURCE_MANIFEST, "source manifest 불일치")
    return result


def main():
    if sys.flags.optimize:
        raise RuntimeError(GUARD)
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest-sha256", required=True)
    parser.add_argument("--metadata", type=pathlib.Path)
    args = parser.parse_args()
    before = hashes()
    require(before["SHA256SUMS"] == args.manifest_sha256, "시험 manifest 불일치")
    rows = parse_manifest((BASE / "SHA256SUMS").read_text(), FILES)
    require(all(before[name] == value for name, value in rows.items()), "시험 파일 변경")
    sources = verify_sources()
    contract = json.loads((BASE / "contract.json").read_text())
    require(contract["source_head"] == HEAD and contract["source_tree"] == TREE, "계약 head 불일치")
    require(contract["source_manifest_sha256"] == SOURCE_MANIFEST, "계약 manifest 불일치")
    require(set(contract["deliverables"]) == FILES | {"SHA256SUMS"}, "배포 파일 집합 불일치")
    names = []
    for module in MODULES:
        text = (BASE / (module + ".rs")).read_text()
        found = re.findall(r"#\[test\]\s*fn ([a-z0-9_]+)\(", text)
        require(len(found) == text.count("#[test]"), "시험 선언 분석 실패")
        names.extend(module + "::" + name for name in found)
    require(len(names) == len(set(names)) == contract["declared_test_count"] == 62, "선언 수 불일치")
    require(sorted(names) == contract["test_names"] == sorted(contract["evidence_classes"]), "이름 집합 불일치")
    for name in FILES:
        raw = (BASE / name).read_bytes()
        require(b"\xc2\xb7" not in raw, "가운뎃점 금지")
    for path in BASE.glob("*.rs"):
        text = path.read_text()
        require("#[ignore" not in text and "#[should_panic" not in text, "시험 제외 금지")
        require(not re.search(r"include!|#\[path|Command::|execveat|\bchmod\b|\bchown\b|\bmount\b", text), "범위 밖 Rust 경계")
    support = (BASE / "support.rs").read_text()
    require("impl Drop for Sandbox" not in support, "실패 증거 자동 정리 금지")
    require("owned_descriptors(&self.root).is_empty()" in support, "소유 fd 점검 누락")
    main = (BASE / "main.rs").read_text()
    require(main.startswith('#![cfg(target_os = "linux")]'), "native Linux 경계 누락")
    require(set(re.findall(r"mod ([a-z_]+);", main)) == set(MODULES) | {"support", "fixtures"}, "모듈 집합 불일치")
    plan = json.loads((BASE / "run-plan.json").read_text())
    require(plan["target"]["exact_names"] == sorted(names) and plan["target"]["count"] == 62, "실행 계획 이름 불일치")
    require(plan["target"]["filtered_per_case"] == 61, "filtered 수 불일치")
    require(plan["bounds"]["per_case_seconds"] == 30 and plan["bounds"]["channel_wait_seconds"] == 5, "시간 상한 변경")
    require(plan["bounds"]["compile_seconds"] == 120 and plan["bounds"]["per_command_log_bytes"] == 8388608, "실행 상한 변경")
    manifest = tomllib.loads((BASE / "Cargo.toml").read_text())
    require(manifest["package"]["autotests"] is False, "자동 시험 발견 금지")
    require(manifest["test"] == [{"name": "file_job_public_api", "path": "main.rs"}], "target 불일치")
    require(manifest["dependencies"] == {
        "job": {"package": "zipelo-gzip-file-job", "path": "../../experiments/rust-gzip-file-job"},
        "libc": {"version": "=0.2.191", "default-features": False, "features": ["std"]},
        "sha2": {"version": "=0.10.9", "default-features": False, "features": ["std", "force-soft"]},
    }, "직접 의존성 변경")
    normal = lambda rows: sorted(rows, key=lambda row: (row["name"], row["version"]))
    original = tomllib.loads((SOURCE / "Cargo.lock").read_text())["package"]
    locked = tomllib.loads((BASE / "Cargo.lock").read_text())["package"]
    package = {"name": "zipelo-gzip-file-job-regressions", "version": "0.1.0",
               "dependencies": ["libc", "sha2", "zipelo-gzip-file-job"]}
    require(normal(locked) == normal(original + [package]), "기존 lock graph 변경")
    require(len([p for p in locked if "source" in p]) == 11, "registry 수 변경")
    if args.metadata:
        require(args.metadata.resolve().is_relative_to(BASE), "metadata 위치 이탈")
        metadata = json.loads(args.metadata.read_text())
        packages = {p["id"]: p for p in metadata["packages"]}
        root = packages[metadata["resolve"]["root"]]
        require(root["name"] == package["name"], "metadata root 불일치")
        require(len(root["targets"]) == 1 and root["targets"][0]["kind"] == ["test"], "target 추가")
        require(pathlib.Path(root["targets"][0]["src_path"]) == BASE / "main.rs", "시험 경로 이탈")
        for node in metadata["resolve"]["nodes"]:
            name = packages[node["id"]]["name"]
            if name == "sha2":
                require(sorted(node["features"]) == ["force-soft", "std"], "sha2 feature 변경")
            if name == "libc":
                require(sorted(node["features"]) == ["std"], "libc feature 변경")
    require(verify_sources() == sources and hashes() == before, "검사 중 입력 변경")
    print(json.dumps({"head": HEAD, "source_files": 39, "declared": len(names),
                      "rust_compiled": False, "rust_executed": 0, "inputs_unchanged": True}))


if __name__ == "__main__":
    main()

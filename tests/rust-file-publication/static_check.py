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
SOURCE = ROOT / "experiments/rust-file-publication"
HEAD = "10dc446c90d3527437298cff270b26d2e196a748"
SOURCE_MANIFEST = "cc4e4cc64a8f3b537df554c7d2895ea3722fbfb0906f1ada767d5cf9b5d8bbc6"
FILES = {
    "Cargo.toml", "Cargo.lock", "main.rs", "support.rs", "boundaries.rs",
    "paths.rs", "namespace.rs", "control.rs", "concurrency.rs", "models.rs",
    "source-pins.json", "contract.json", "run-plan.json", "static_check.py",
    "static_check_helper_tests.py",
}
MODULES = ("boundaries", "paths", "namespace", "control", "concurrency", "models")
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
        require(not path.is_symlink() and stat.S_ISREG(path.lstat().st_mode), "일반 시험 파일 필요")
        result[name] = digest(path.read_bytes())
    return result

def verify_sources():
    pins = json.loads((BASE / "source-pins.json").read_text())
    require(pins["head"] == HEAD and len(pins["files"]) == 10, "고정 source 기준 불일치")
    names = [row["path"] for row in pins["files"]]
    require(len(set(names)) == 10, "source 경로 중복")
    result = {}
    for row in pins["files"]:
        path = ROOT / row["path"]
        require(path.resolve().is_relative_to(SOURCE) and not path.is_symlink(), "source 경로 이탈")
        info = path.lstat()
        require(stat.S_ISREG(info.st_mode) and not info.st_mode & 0o111, "source mode 불일치")
        raw = path.read_bytes()
        require(digest(raw) == row["sha256"], "source SHA256 불일치")
        blob = hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest()
        require(blob == row["git_blob"] and row["mode"] == "100644", "source blob 불일치")
        result[row["path"]] = digest(raw)
    manifest = (SOURCE / "SHA256SUMS").read_bytes()
    require(digest(manifest) == SOURCE_MANIFEST, "source manifest 불일치")
    relative = {str((ROOT / name).relative_to(SOURCE)) for name in names} - {"SHA256SUMS"}
    entries = parse_manifest(manifest.decode(), relative)
    for name, value in entries.items():
        require(digest((SOURCE / name).read_bytes()) == value, "source manifest 파일 불일치")
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
    require(contract["source_head"] == HEAD and contract["source_manifest_sha256"] == SOURCE_MANIFEST, "계약 head 불일치")
    names = []
    for module in MODULES:
        source = (BASE / (module + ".rs")).read_text()
        found = re.findall(r"#\[test\]\s*fn ([a-z0-9_]+)\(", source)
        require(len(found) == source.count("#[test]"), "시험 선언 분석 실패")
        names.extend(module + "::" + name for name in found)
    require(len(names) == len(set(names)) == contract["declared_test_count"] == 54, "선언 수 불일치")
    require(sorted(names) == contract["test_names"] == sorted(contract["evidence_classes"]), "이름 집합 불일치")
    for path in BASE.glob("*.rs"):
        source = path.read_text()
        require("#[ignore" not in source and "#[should_panic" not in source, "시험 제외 금지")
        require(not re.search(r"include!|#\[path|Command::|execveat|\bmknod\b|\bchmod\b|\bchown\b", source), "범위 밖 Rust 경계")
        require("\u00b7" not in source, "가운뎃점 금지")
    require("impl Drop for Sandbox" not in (BASE / "support.rs").read_text(), "실패 증거 자동 정리 금지")
    plan = json.loads((BASE / "run-plan.json").read_text())
    require(plan["bounds"]["per_case_seconds"] == 30 and plan["bounds"]["channel_wait_seconds"] == 5, "시간 상한 변경")
    manifest = tomllib.loads((BASE / "Cargo.toml").read_text())
    require(manifest["package"]["autotests"] is False, "자동 시험 발견 금지")
    require(manifest["test"] == [{"name": "publication_public_api", "path": "main.rs"}], "target 불일치")
    require(manifest["dependencies"] == {
        "publication": {"package": "zipelo-file-publication", "path": "../../experiments/rust-file-publication"},
        "libc": {"version": "=0.2.191", "default-features": False, "features": ["std"]},
    }, "직접 의존성 변경")
    normal = lambda rows: sorted(rows, key=lambda row: (row["name"], row["version"]))
    original = tomllib.loads((SOURCE / "Cargo.lock").read_text())["package"]
    locked = tomllib.loads((BASE / "Cargo.lock").read_text())["package"]
    package = {"name": "zipelo-file-publication-regressions", "version": "0.1.0", "dependencies": ["libc", "zipelo-file-publication"]}
    require(normal(locked) == normal(original + [package]), "기존 lock graph 변경")
    if args.metadata:
        require(args.metadata.resolve().is_relative_to(BASE), "metadata 위치 이탈")
        metadata = json.loads(args.metadata.read_text())
        packages = {p["id"]: p for p in metadata["packages"]}
        root_package = packages[metadata["resolve"]["root"]]
        require(root_package["name"] == package["name"], "metadata root 불일치")
        require(len(root_package["targets"]) == 1 and root_package["targets"][0]["kind"] == ["test"], "target 추가")
        registry = {(p["name"], p["version"], p["checksum"]) for p in locked if "source" in p}
        expected = {(p["name"], p["version"], p["checksum"]) for p in json.loads((SOURCE / "dependencies.json").read_text())["direct"]}
        require(registry == expected, "registry graph 변경")
        for node in metadata["resolve"]["nodes"]:
            if packages[node["id"]]["name"] == "libc":
                require(sorted(node["features"]) == ["std"], "libc feature 변경")
    require(verify_sources() == sources and hashes() == before, "검사 중 입력 변경")
    print(json.dumps({"head": HEAD, "source_files": 10, "declared": len(names),
                      "rust_compiled": False, "rust_executed": 0, "inputs_unchanged": True}))

if __name__ == "__main__":
    main()

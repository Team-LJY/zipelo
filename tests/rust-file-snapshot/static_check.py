import argparse
import hashlib
import json
import pathlib
import re
import sys
import tomllib

BASE = pathlib.Path(__file__).resolve().parent
ROOT = BASE.parents[1]
HEAD = "b7c3d5575943f8caab8bbdcf465c278c3e7059d1"
SOURCE = ROOT / "experiments/rust-file-snapshot"
SOURCE_MANIFEST = "668b1c3f140a033a6a0768b84ec52f111ee5fbfaf707632d2fc9a5a380758120"
FILES = {
    "Cargo.toml", "Cargo.lock", "main.rs", "support.rs", "boundaries.rs",
    "paths.rs", "mutations.rs", "control.rs", "source-pins.json",
    "contract.json", "static_check.py", "static_check_helper_tests.py",
}
MODULES = ("boundaries", "paths", "mutations", "control")
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
        require(not path.is_symlink(), "시험 파일 링크 거부")
        result[name] = digest(path.read_bytes())
    return result


def verify_sources():
    pins = json.loads((BASE / "source-pins.json").read_text())
    require(pins["head"] == HEAD and len(pins["files"]) == 9, "고정 source 기준 불일치")
    names = [row["path"] for row in pins["files"]]
    require(len(set(names)) == 9, "source 경로 중복")
    result = {}
    for row in pins["files"]:
        path = ROOT / row["path"]
        require(path.resolve().is_relative_to(SOURCE) and not path.is_symlink(), "source 경로 이탈")
        raw = path.read_bytes()
        require(digest(raw) == row["sha256"], "source SHA256 불일치")
        blob = hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest()
        require(blob == row["git_blob"] and row["mode"] == "100644", "source blob 불일치")
        result[row["path"]] = digest(raw)
    manifest = (SOURCE / "SHA256SUMS").read_bytes()
    require(digest(manifest) == SOURCE_MANIFEST, "source manifest 불일치")
    entries = parse_manifest(manifest.decode(), set(names) - {"experiments/rust-file-snapshot/SHA256SUMS"})
    for name, value in entries.items():
        require(digest((ROOT / name).read_bytes()) == value, "source manifest 파일 불일치")
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
    require(contract["source_head"] == HEAD, "계약 head 불일치")
    names = []
    for module in MODULES:
        source = (BASE / (module + ".rs")).read_text()
        found = re.findall(r"#\[test\]\s*fn ([a-z0-9_]+)\(", source)
        require(len(found) == source.count("#[test]"), "시험 선언 분석 실패")
        names.extend(module + "::" + name for name in found)
    require(len(names) == len(set(names)) == contract["declared_test_count"] == 72, "선언 수 불일치")
    require(sorted(names) == contract["test_names"] == sorted(contract["evidence_classes"]), "이름 집합 불일치")
    for path in BASE.glob("*.rs"):
        source = path.read_text()
        require("#[ignore" not in source and "#[should_panic" not in source, "시험 제외 금지")
        require(not re.search(r"include!|#\[path|Command::|execveat|\bmknod\b", source), "범위 밖 Rust 경계")
    manifest = tomllib.loads((BASE / "Cargo.toml").read_text())
    require(manifest["package"]["autotests"] is False, "자동 시험 발견 금지")
    require(manifest["test"] == [{"name": "snapshot_public_api", "path": "main.rs"}], "target 불일치")
    require(manifest["dependencies"] == {
        "snapshot": {"package": "zipelo-file-snapshot", "path": "../../experiments/rust-file-snapshot"},
        "libc": {"version": "=0.2.191", "default-features": False, "features": ["std"]},
    }, "직접 의존성 변경")
    normal = lambda rows: sorted(rows, key=lambda row: (row["name"], row["version"]))
    original = tomllib.loads((SOURCE / "Cargo.lock").read_text())["package"]
    locked = tomllib.loads((BASE / "Cargo.lock").read_text())["package"]
    root = {"name": "zipelo-file-snapshot-regressions", "version": "0.1.0", "dependencies": ["libc", "zipelo-file-snapshot"]}
    require(normal(locked) == normal(original + [root]), "기존 lock graph 변경")
    if args.metadata:
        require(args.metadata.resolve().is_relative_to(BASE), "metadata 위치 이탈")
        metadata = json.loads(args.metadata.read_text())
        packages = {p["id"]: p for p in metadata["packages"]}
        root_package = packages[metadata["resolve"]["root"]]
        require(root_package["name"] == root["name"], "metadata root 불일치")
        require(len(root_package["targets"]) == 1 and root_package["targets"][0]["kind"] == ["test"], "target 추가")
        registry = {(p["name"], p["version"], p["checksum"]) for p in locked if "source" in p}
        expected = {(p["name"], p["version"], p["checksum"]) for p in json.loads((SOURCE / "dependencies.json").read_text())["registry_packages"]}
        require(registry == expected, "registry graph 변경")
        for node in metadata["resolve"]["nodes"]:
            package = packages[node["id"]]
            if package["name"] == "sha2":
                require(sorted(node["features"]) == ["force-soft", "std"], "sha2 feature 변경")
            if package["name"] == "libc":
                require(sorted(node["features"]) == ["std"], "libc feature 변경")
    require(verify_sources() == sources and hashes() == before, "검사 중 입력 변경")
    print(json.dumps({"head": HEAD, "source_files": 9, "declared": len(names),
                      "rust_compiled": False, "rust_executed": 0, "inputs_unchanged": True}))


if __name__ == "__main__":
    main()

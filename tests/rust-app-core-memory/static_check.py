import argparse
import hashlib
import json
import pathlib
import re
import runpy
import sys
import tomllib

BASE = pathlib.Path(__file__).resolve().parent
ROOT = BASE.parents[1]
SOURCE = ROOT / "experiments/rust-app-core-memory"
HEAD = "23b8d1c615f24f42469138fcc63e7fce7cda4f24"
SOURCE_MANIFEST = "13afa3d9bf53fd5a1c69af085842b722953dd36de1d63d305fea93bf78580b8d"
FILES = {
    "Cargo.toml", "Cargo.lock", "main.rs", "support.rs", "control.rs",
    "delivery.rs", "gzip.rs", "ustar.rs", "zip.rs", "send.rs",
    "generate_fixtures.py", "fixtures.hex", "fixtures.json", "source-pins.json",
    "contract.json", "static_check.py", "static_check_helper_tests.py",
}
MODULES = ("control", "delivery", "gzip", "send", "ustar", "zip")
GUARD = "최적화 모드에서는 정적 검사를 실행할 수 없음"


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def parse_manifest(text, expected_paths):
    result = {}
    for line in text.splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_./-]+)", line)
        require(match is not None, "잘못된 manifest 행")
        expected, name = match.groups()
        path = pathlib.PurePosixPath(name)
        require(not path.is_absolute() and ".." not in path.parts
                and str(path) == name and name not in result, "중복 또는 잘못된 경로")
        result[name] = expected
    require(set(result) == set(expected_paths), "manifest 경로 누락 또는 추가")
    return result


def verify_manifest(base, expected_digest, expected_paths):
    path = base / "SHA256SUMS"
    require(not path.is_symlink(), "manifest symlink 거부")
    raw = path.read_bytes()
    require(digest(raw) == expected_digest, "manifest digest 불일치")
    entries = parse_manifest(raw.decode("utf-8"), expected_paths)
    for name, expected in entries.items():
        target = base / name
        require(target.resolve().is_relative_to(base.resolve()) and not target.is_symlink(),
                "manifest 경로 이탈")
        require(digest(target.read_bytes()) == expected, "파일 digest 불일치: " + name)
    return entries


def verify_sources():
    pins = json.loads((BASE / "source-pins.json").read_text())
    require(pins["head"] == HEAD and len(pins["files"]) == 69, "source pin 기준 불일치")
    paths = [item["path"] for item in pins["files"]]
    require(len(set(paths)) == 69, "source pin 중복")
    core_paths = {p.removeprefix("experiments/rust-app-core-memory/")
                  for p in paths if p.startswith("experiments/rust-app-core-memory/")}
    require(len(core_paths) == 9 and "SHA256SUMS" in core_paths, "코어 파일 수 불일치")
    verify_manifest(SOURCE, SOURCE_MANIFEST, core_paths - {"SHA256SUMS"})
    imported = json.loads((SOURCE / "sources.json").read_text())["files"]
    require(len(imported) == 60, "통합 원본 수 불일치")
    require(set(paths) == {item["path"] for item in imported}
            | {"experiments/rust-app-core-memory/" + p for p in core_paths}, "source 경로 불일치")
    pin_map = {item["path"]: item for item in pins["files"]}
    for item in imported:
        require(all(item[key] == pin_map[item["path"]][key] for key in ("sha256", "git_blob")),
                "통합 원본 pin 불일치")
    result = {}
    for item in pins["files"]:
        path = ROOT / item["path"]
        require(not path.is_symlink() and path.resolve().is_relative_to(ROOT), "source 경로 이탈")
        raw = path.read_bytes()
        require(digest(raw) == item["sha256"], "source digest 불일치: " + item["path"])
        blob = hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest()
        require(blob == item["git_blob"], "원격 blob 불일치: " + item["path"])
        result[item["path"]] = digest(raw)
    return result


def verify_metadata(path):
    require(path.resolve().is_relative_to(BASE), "metadata는 tests 안에 있어야 함")
    metadata = json.loads(path.read_text())
    packages = {package["id"]: package for package in metadata["packages"]}
    selected = {}
    for node in metadata["resolve"]["nodes"]:
        package = packages[node["id"]]
        if package["source"]:
            selected[(package["name"], package["version"])] = sorted(node["features"])
    expected = {
        (package["name"], package["version"]): sorted(package["features"])
        for package in json.loads((SOURCE / "dependencies.json").read_text())["packages"]
    }
    require(selected == expected and len(selected) == 29, "선택 registry feature 변경")
    root = packages[metadata["resolve"]["root"]]
    require(root["name"] == "zipelo-app-core-memory-regressions", "metadata root 불일치")
    require(len(root["targets"]) == 1 and root["targets"][0]["kind"] == ["test"],
            "test target 이외 대상 추가")
    require(root["targets"][0]["name"] == "app_core_public_api", "target 이름 불일치")
    return len(selected)


def main():
    if sys.flags.optimize:
        raise RuntimeError(GUARD)
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest-sha256", required=True)
    parser.add_argument("--metadata", type=pathlib.Path)
    args = parser.parse_args()
    entries = verify_manifest(BASE, args.manifest_sha256, FILES)
    before = verify_sources()
    contract = json.loads((BASE / "contract.json").read_text())
    require(contract["source_head"] == HEAD, "contract 기준 head 불일치")
    names = []
    for module in MODULES:
        source = (BASE / (module + ".rs")).read_text()
        found = re.findall(r"#\[test\]\s*fn ([a-z0-9_]+)\(", source)
        require(len(found) == source.count("#[test]"), "정적 시험 이름 분석 실패")
        names.extend(module + "::" + name for name in found)
        require("#[ignore" not in source and "#[should_panic" not in source, "시험 제외 금지")
    require(len(names) == len(set(names)) == contract["declared_test_count"],
            "정적 선언 수 불일치")
    require(sorted(names) == contract["test_names"], "고정 시험 이름 불일치")
    for path in BASE.glob("*.rs"):
        source = path.read_text()
        require(not re.search(r"\bunsafe\s*\{|#\[path|include!|std::process|std::thread|std::fs", source),
                "범위 밖 Rust 동작")
    main_source = (BASE / "main.rs").read_text()
    require(set(re.findall(r"^mod ([a-z_]+);$", main_source, re.M))
            == set(MODULES) | {"support"}, "모듈 누락")
    manifest = tomllib.loads((BASE / "Cargo.toml").read_text())
    require(manifest["package"]["autotests"] is False, "자동 test 발견 금지")
    require(manifest["test"] == [{"name": "app_core_public_api", "path": "main.rs"}],
            "test 경로 불일치")
    require(manifest["dependencies"] == {
        "app_core": {"package": "zipelo-app-core-memory",
                     "path": "../../experiments/rust-app-core-memory"}
    }, "직접 의존성 변경")
    original_lock = tomllib.loads((SOURCE / "Cargo.lock").read_text())["package"]
    test_lock = tomllib.loads((BASE / "Cargo.lock").read_text())["package"]
    addition = {"name": "zipelo-app-core-memory-regressions", "version": "0.1.0",
                "dependencies": ["zipelo-app-core-memory"]}
    normalize = lambda rows: sorted(rows, key=lambda row: (row["name"], row["version"]))
    require(normalize(test_lock) == normalize(original_lock + [addition]), "기존 lock 변경")
    generated_hex, generated_json = runpy.run_path(str(BASE / "generate_fixtures.py"))["build"]()
    require((BASE / "fixtures.hex").read_text() == generated_hex, "fixture bytes 불일치")
    require((BASE / "fixtures.json").read_text() == generated_json, "fixture 출처 불일치")
    selected = verify_metadata(args.metadata) if args.metadata else None
    require(verify_sources() == before, "정적 검사 중 원본 변경")
    require(verify_manifest(BASE, args.manifest_sha256, FILES) == entries,
            "정적 검사 중 시험 입력 변경")
    print(json.dumps({
        "source_head": HEAD, "source_pins_verified": len(before),
        "manifest_sha256": args.manifest_sha256, "declared": len(names),
        "selected_registry_packages": selected, "rust_compiled": False,
        "runtime_discovered": None, "runtime_executed": 0,
        "fixture_stdlib_verification": True, "inputs_unchanged": True,
    }, ensure_ascii=False))


if __name__ == "__main__":
    main()

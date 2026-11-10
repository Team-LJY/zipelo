import argparse
import hashlib
import json
import pathlib
import re
import runpy
import tomllib

BASE = pathlib.Path(__file__).resolve().parent
ROOT = BASE.parent.parent
SOURCE = ROOT / "experiments/rust-memory-gzip-reader"
SOURCE_HEAD = "e1ffd7bbffc358b6e53358682a93bf3ce075e892"
SOURCE_MANIFEST = "13e41e7d8ce2442cd8d93b1229211a5257e0568acc3466d5f9ec37f03c72a736"
TEST_FILES = {
    "Cargo.toml", "Cargo.lock", "main.rs", "support.rs", "format.rs",
    "budgets.rs", "checkpoints.rs", "generate_fixtures.py",
    "fixtures.hex", "fixtures.json", "contract.json", "static_check.py",
}
SOURCE_FILES = {
    "Cargo.toml", "Cargo.lock", "LICENSE", "THIRD-PARTY-NOTICES.txt",
    "contract.json", "dependencies.json", "src/lib.rs", "src/budget.rs",
}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify_manifest(base, expected_digest, paths):
    manifest = base / "SHA256SUMS"
    assert re.fullmatch(r"[0-9a-f]{64}", expected_digest), "신뢰 hash 형식 오류"
    assert not manifest.is_symlink()
    assert digest(manifest) == expected_digest, str(manifest)
    records = {}
    for line in manifest.read_text().splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_./-]+)", line)
        assert match, f"잘못된 manifest 행: {line!r}"
        expected, name = match.groups()
        relative = pathlib.PurePosixPath(name)
        assert not relative.is_absolute() and all(part not in {"", ".", ".."} for part in name.split("/"))
        assert name not in records and name in paths, name
        path = base / name
        assert not any(parent.is_symlink() for parent in [path, *path.parents])
        assert path.resolve().is_relative_to(base.resolve()) and path.is_file(), name
        assert digest(path) == expected, name
        records[name] = expected
    assert set(records) == paths, "manifest 경로 누락"
    return records


def verify_pin(record):
    path = ROOT / record["path"]
    content = path.read_bytes()
    assert hashlib.sha256(content).hexdigest() == record["sha256"], record["path"]
    blob = b"blob " + str(len(content)).encode() + b"\0" + content
    assert hashlib.sha1(blob).hexdigest() == record["git_blob"], record["path"]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-head", required=True)
    parser.add_argument("--test-manifest-sha256", required=True)
    args = parser.parse_args()
    assert args.source_head == SOURCE_HEAD
    test_records = verify_manifest(BASE, args.test_manifest_sha256, TEST_FILES)
    source_records = verify_manifest(SOURCE, SOURCE_MANIFEST, SOURCE_FILES)
    contract = json.loads((BASE / "contract.json").read_text())
    assert contract["source_commit"] == SOURCE_HEAD
    pins = contract["source_pins"] + contract["baseline"]["pins"]
    assert len(pins) == len({row["path"] for row in pins}) == 16
    assert {row["path"] for row in contract["source_pins"]} == {
        "experiments/rust-memory-gzip-reader/" + name for name in SOURCE_FILES | {"SHA256SUMS"}
    }
    for record in pins:
        verify_pin(record)

    names = []
    counts = {}
    for path in sorted(BASE.glob("*.rs")):
        source = path.read_text()
        assert not re.search(r"#\[ignore\]|\bunsafe\s*\{|include!|#\[path|\bextern\s", source), path.name
        found = re.findall(r"#\[test\]\s*fn\s+(\w+)\s*\(", source)
        names.extend(f"{path.stem}::{name}" for name in found)
        if found:
            counts[path.stem] = len(found)
    assert len(names) == len(set(names)) == contract["expected_test_count"]
    assert sorted(names) == contract["test_names"]
    assert set(counts) == {"format", "budgets", "checkpoints"}
    for mapping in contract["applicability"]:
        assert mapping["status"] in {"maintained", "adapted", "not_applicable"}
        for prefix in mapping["rust_prefixes"]:
            assert any(name.startswith(prefix) for name in names), prefix
    for flag in range(32):
        for selected in ["selected", "unselected"]:
            assert f"format::flags_{flag:02}_{selected}" in names
            assert f"format::truncate_flags_{flag:02}_{selected}" in names

    cargo = tomllib.loads((BASE / "Cargo.toml").read_text())
    assert cargo["dependencies"] == {
        "zipelo-memory-gzip-reader": {"path": "../../experiments/rust-memory-gzip-reader"}
    }
    assert cargo["test"] == [{"name": "memory_gzip_reader", "path": "main.rs"}]
    assert cargo["package"]["autotests"] is False
    assert "build" not in cargo["package"] and "build-dependencies" not in cargo
    assert "workspace" in cargo
    source_lock = tomllib.loads((SOURCE / "Cargo.lock").read_text())
    test_lock = tomllib.loads((BASE / "Cargo.lock").read_text())
    source_packages = {row["name"]: row for row in source_lock["package"]}
    test_packages = {row["name"]: row for row in test_lock["package"]}
    assert set(test_packages) == set(source_packages) | {"zipelo-memory-gzip-regressions"}
    assert all(test_packages[key] == row for key, row in source_packages.items())
    assert test_packages["zipelo-memory-gzip-regressions"]["dependencies"] == ["zipelo-memory-gzip-reader"]

    generator = runpy.run_path(str(BASE / "generate_fixtures.py"))
    generated, fixture_info = generator["produce"]()
    assert generated == (BASE / "fixtures.hex").read_text()
    recorded_info = json.loads((BASE / "fixtures.json").read_text())
    assert fixture_info["fixtures"] == recorded_info["fixtures"]
    assert len(recorded_info["fixtures"]) == 40
    assert contract["truncation_prefix_assertions_planned"] == 2 * sum(
        row["archive_bytes"] for name, row in recorded_info["fixtures"].items() if name.startswith("flags_")
    )
    for record in pins:
        verify_pin(record)
    verify_manifest(BASE, args.test_manifest_sha256, TEST_FILES)
    verify_manifest(SOURCE, SOURCE_MANIFEST, SOURCE_FILES)
    print(json.dumps({
        "result": "static_checks_passed",
        "source_commit": SOURCE_HEAD,
        "source_manifest_sha256": SOURCE_MANIFEST,
        "test_manifest_sha256": args.test_manifest_sha256,
        "helper_sha256": test_records["static_check.py"],
        "declared_tests": len(names),
        "module_counts": counts,
        "source_manifest_entries": len(source_records),
        "test_manifest_entries": len(test_records),
        "csharp_files_checked": len(contract["baseline"]["pins"]),
        "python_fixtures_regenerated": len(recorded_info["fixtures"]),
        "rust_compiled": False,
        "runtime_discovered": None,
        "runtime_executed": 0,
        "runtime_passed": None,
    }, ensure_ascii=False, indent=4))


if __name__ == "__main__":
    main()

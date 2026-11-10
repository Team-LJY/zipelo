import argparse
import hashlib
import json
import pathlib
import re
import tomllib

BASE = pathlib.Path(__file__).resolve().parent
ROOT = BASE.parent.parent


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    contract = json.loads((BASE / "contract.json").read_text())
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-head", default=contract["source_commit"])
    args = parser.parse_args()
    expected = contract["source_pins"][args.source_head]
    for name, digest in expected.items():
        assert sha256(ROOT / name) == digest, name

    for record in contract["baseline_blobs"]:
        content = (ROOT / record["path"]).read_bytes()
        blob = b"blob " + str(len(content)).encode() + b"\0" + content
        assert hashlib.sha1(blob).hexdigest() == record["git_blob"], record["path"]

    names = []
    counts = {}
    for path in sorted(BASE.glob("*.rs")):
        content = path.read_text()
        assert not re.search(r"#\[ignore\]|\bunsafe\s*\{|include!|#\[path", content), path.name
        found = re.findall(r"#\[test\]\s*fn\s+(\w+)\s*\(", content)
        names.extend(path.stem + "::" + name for name in found)
        if found:
            counts[path.stem] = len(found)
    assert len(names) == len(set(names)) == contract["test_count"]
    assert sorted(names) == contract["test_names"]
    mappings = contract["applicability"]
    assert len(mappings) == len({row["csharp"] for row in mappings}) == 103
    assert all(name in names for row in mappings for name in row["rust"])

    manifest = tomllib.loads((BASE / "Cargo.toml").read_text())
    assert manifest["dependencies"]["zipelo-memory-ustar-reader"]["path"] == "../../experiments/rust-memory-ustar-reader"
    assert manifest["test"] == [{"name": "memory_ustar_reader", "path": "main.rs"}]
    source_lock = tomllib.loads((ROOT / "experiments/rust-memory-ustar-reader/Cargo.lock").read_text())
    test_lock = tomllib.loads((BASE / "Cargo.lock").read_text())
    source_packages = {package["name"]: package for package in source_lock["package"]}
    test_packages = {package["name"]: package for package in test_lock["package"]}
    assert set(test_packages) == set(source_packages) | {"zipelo-memory-ustar-regressions"}
    assert all(test_packages[name] == package for name, package in source_packages.items())

    for line in (BASE / "SHA256SUMS").read_text().splitlines():
        digest, name = line.split("  ", 1)
        assert name.startswith("tests/rust-memory-ustar-reader/")
        assert sha256(ROOT / name) == digest, name

    print(json.dumps({
        "source_head_checked": args.source_head,
        "declared_tests": len(names),
        "module_counts": counts,
        "source_files_checked": len(expected),
        "baseline_files_checked": len(contract["baseline_blobs"]),
        "registry_lock_records_unchanged": len([p for p in source_packages.values() if "source" in p]),
        "runtime_tests_discovered": None,
        "runtime_tests_executed": 0,
        "result": "static_checks_passed"
    }, ensure_ascii=False, indent=4))


if __name__ == "__main__":
    main()

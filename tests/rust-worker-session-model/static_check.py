import hashlib
import json
from pathlib import Path
import re
import tomllib

ROOT = Path(__file__).resolve().parents[2]
MODULE = ROOT / "experiments/rust-worker-session-model"
TESTS = ROOT / "tests/rust-worker-session-model"


def check():
    package = tomllib.loads((MODULE / "Cargo.toml").read_text())
    assert not package.get("dependencies") and not package.get("build-dependencies")
    assert package["package"]["publish"] is False
    assert package["package"]["autotests"] is False
    assert package["test"] == [{"name": "worker_session_model", "path": "../../tests/rust-worker-session-model/main.rs"}]
    lock = tomllib.loads((MODULE / "Cargo.lock").read_text())
    assert len(lock["package"]) == 1 and not lock["package"][0].get("dependencies")
    source = "\n".join(path.read_text() for path in (MODULE / "src").glob("*.rs"))
    assert "#![forbid(unsafe_code)]" in (MODULE / "src/lib.rs").read_text()
    assert not re.search(r"std::(?:fs|process|net|os)|Command::|extern \"|unsafe\s*\{|include!|include_bytes!", source)
    assert not (MODULE / "build.rs").exists()
    for manifest in [MODULE / "SHA256SUMS", TESTS / "SHA256SUMS"]:
        for line in manifest.read_text().splitlines():
            digest, name = line.split("  ", 1)
            path = ROOT / name
            assert not path.is_symlink() and path.is_file()
            assert hashlib.sha256(path.read_bytes()).hexdigest() == digest, name
    contract = json.loads((TESTS / "contract.json").read_text())
    names = re.findall(r"#\[test\]\s*fn\s+(\w+)\s*\(", (TESTS / "main.rs").read_text())
    assert len(names) == len(set(names)) == contract["expected_count"] == 30
    assert names == contract["names"]
    assert not re.search(r"#\[ignore|should_panic|std::process|std::fs|std::net", (TESTS / "main.rs").read_text())
    assert all(contract[key] is False for key in ["compiled", "discovered", "executed"])
    model = json.loads((MODULE / "contract.json").read_text())
    assert model["base_head"] == "f7fc8cbece281e2f9a6374b4640ca4dd9d321fbc"
    assert model["scope"]["external_dependencies"] == 0
    for method in ["live_verified", "publication_allowed"]:
        assert re.search(r"pub fn " + method + r"\(&self\) -> bool\s*\{\s*false\s*\}", source)
    return {"state": "passed", "declared_cases": len(names), "external_dependencies": 0, "rust_compiled": False, "rust_discovered": False, "rust_executed": False, "scope": "source/hash/manifest/name checks only; not a Rust semantic or OS proof"}


if __name__ == "__main__":
    print(json.dumps(check(), sort_keys=True))

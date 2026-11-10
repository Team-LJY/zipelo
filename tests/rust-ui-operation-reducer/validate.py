import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
CRATE = ROOT / "experiments/rust-ui-operation-reducer"
ARTIFACTS = HERE / "artifacts"
ARTIFACTS.mkdir(exist_ok=True)
ENV = dict(os.environ, CARGO_TARGET_DIR=str(ARTIFACTS / "target"))
BASELINE = ROOT / "tests/rust-ui-probe/SHA256SUMS"
PROTECTED = [ROOT / line.split("  ", 1)[1] for line in BASELINE.read_text().splitlines()] + [BASELINE]


def hashes(paths):
    return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}


def run(name, args):
    result = subprocess.run(args, cwd=ROOT, env=ENV, text=True, capture_output=True)
    (ARTIFACTS / (name + ".stdout")).write_text(result.stdout)
    (ARTIFACTS / (name + ".stderr")).write_text(result.stderr)
    evidence = {"command": args, "exit": result.returncode}
    if result.returncode:
        raise RuntimeError(json.dumps(evidence) + "\n" + result.stdout + result.stderr)
    return result.stdout, evidence


before = hashes(PROTECTED)
source_paths = [CRATE / "src/lib.rs", CRATE / "src/listing.rs", HERE / "overflow.rs", HERE / "reducer.rs"]
source_before = hashes(source_paths + [CRATE / "Cargo.toml", CRATE / "Cargo.lock"])
unit_names = re.findall(r"#\[test\]\s+fn (\w+)\(", (HERE / "overflow.rs").read_text())
api_names = re.findall(r"#\[test\]\s+fn (\w+)\(", (HERE / "reducer.rs").read_text())
expected = sorted(["overflow::" + name for name in unit_names] + api_names)
assert len(expected) == len(set(expected)) == 49
base = ["cargo", "+1.99.1"]
flags = ["--locked", "--offline", "--manifest-path", str(CRATE.relative_to(ROOT) / "Cargo.toml")]
_, build = run("build", base + ["build", "--lib"] + flags)
listed, discovery = run("discovery", base + ["test", "--lib", "--test", "operation_reducer"] + flags + ["--", "--list"])
actual = sorted(re.findall(r"^([^\s]+): test$", listed, re.M))
assert actual == expected, (actual, expected)
output, execution = run("execution", base + ["test", "--lib", "--test", "operation_reducer"] + flags + ["--", "--test-threads=1"])
passed_names = sorted(re.findall(r"^test ([^\s]+) \.\.\. ok$", output, re.M))
assert passed_names == expected
summaries = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out", output)
assert sorted(summaries) == sorted([("6", "0", "0", "0", "0"), ("43", "0", "0", "0", "0")]), summaries
assert hashes(PROTECTED) == before
assert hashes(source_paths + [CRATE / "Cargo.toml", CRATE / "Cargo.lock"]) == source_before
report = {"scope": "작성자 측 순수 모의 reducer 시험임. 독립 검증/실제 GUI/작업 실행이 아님", "build": build, "discovery": discovery, "execution": execution, "discovered": len(actual), "executed": len(passed_names), "passed": len(passed_names), "failed": 0, "ignored": 0, "filtered": 0, "names": actual, "source_hashes": source_before, "baseline_before": before, "baseline_after": hashes(PROTECTED)}
(ARTIFACTS / "validation-report.json").write_text(json.dumps(report, ensure_ascii=False, indent=4) + "\n")
print(json.dumps({"discovered": len(actual), "executed": len(passed_names), "passed": len(passed_names), "summaries": summaries}, ensure_ascii=False))

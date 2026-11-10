import hashlib
import json
import os
from pathlib import Path
import re
import sys
import tomllib

if sys.flags.optimize or os.environ.get("PYTHONOPTIMIZE"):
    raise SystemExit("정상 Python mode 필요")
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def require(condition, message):
    if not condition:
        raise SystemExit(message)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def check():
    contract = json.loads((HERE / "contract.json").read_text())
    names = re.findall(r"#\[test\]\s*fn\s+(\w+)\s*\(", (HERE / "races.rs").read_text())
    require(names == contract["names"] and len(names) == len(set(names)), "이름/중복 불일치")
    old = json.loads((ROOT / "tests/rust-ui-memory-app/contract.json").read_text())["validation"]["names"]
    require(not set(old).intersection(names), "기존 이름 중복")
    for path, record in json.loads((HERE / "sources.json").read_text()).items():
        data = (ROOT / path).read_bytes()
        require(sha(ROOT / path) == record["sha256"], "고정 source hash 불일치: " + path)
        blob = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
        require(blob == record["git_blob"], "고정 blob 불일치: " + path)
    lines = (HERE / "SHA256SUMS").read_text().splitlines()
    listed = []
    for line in lines:
        digest, path = line.split("  ", 1)
        require(path.startswith("tests/rust-ui-memory-races/"), "manifest 경로 이탈")
        require(sha(ROOT / path) == digest, "후보 hash 불일치: " + path)
        listed.append(path)
    require(len(listed) == len(set(listed)), "manifest 중복")
    require(sorted(listed) == sorted("tests/rust-ui-memory-races/" + n for n in contract["files"] if n != "SHA256SUMS"), "manifest 파일 집합 불일치")
    assembly = (HERE / "assembly.rs").read_text()
    require(assembly.count("include!(") == 3, "원본 include 수 불일치")
    for source in ["runtime", "controller", "transport"]:
        require(f'include!("../../experiments/rust-ui-memory-app/src/{source}.rs");' in assembly, "고정 include 누락")
    for token in ["impl Controller", "impl Task", "impl Drop", "unsafe", "thread::sleep", "Barrier::"]:
        require(token not in assembly + (HERE / "races.rs").read_text().replace("#![forbid(unsafe_code)]", ""), "허용되지 않은 대체/대기: " + token)
    manifest = tomllib.loads((HERE / "Cargo.toml").read_text())
    require(set(manifest["dependencies"]) == {"core_memory"}, "직접 dependency 추가")
    original = tomllib.loads((ROOT / "experiments/rust-app-core-memory/Cargo.lock").read_text())["package"]
    candidate = tomllib.loads((HERE / "Cargo.lock").read_text())["package"]
    expected_root = {"name": "zipelo-ui-memory-races", "version": "0.1.0", "dependencies": ["zipelo-app-core-memory"]}
    require(sorted(candidate, key=lambda p: p["name"]) == sorted(original + [expected_root], key=lambda p: p["name"]), "원본 lock 외 package 변경")
    print(json.dumps({"declared": len(names), "fixed_inputs": len(json.loads((HERE / "sources.json").read_text())), "new_external_packages": 0, "compiled": False, "executed": False}))


if __name__ == "__main__":
    check()

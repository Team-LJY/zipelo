import hashlib
import json
import os
from pathlib import Path
import sys
import tomllib

if sys.flags.optimize or os.environ.get("PYTHONOPTIMIZE"):
    raise SystemExit("정상 Python mode 필요")
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def require(value, message):
    if not value:
        raise SystemExit(message)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def check():
    contract = json.loads((HERE / "contract.json").read_text())
    sources = json.loads((HERE / "sources.json").read_text())
    for path, record in sources.items():
        data = (ROOT / path).read_bytes()
        require(sha(ROOT / path) == record["sha256"], "고정 source 불일치: " + path)
        blob = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
        require(blob == record["git_blob"], "고정 blob 불일치: " + path)
    listed = []
    for line in (HERE / "SHA256SUMS").read_text().splitlines():
        digest, path = line.split("  ", 1)
        require(path.startswith("tests/rust-ui-memory-gui-races/"), "manifest 경로 이탈")
        require(sha(ROOT / path) == digest, "후보 hash 불일치: " + path)
        listed.append(path)
    expected = ["tests/rust-ui-memory-gui-races/" + n for n in contract["files"] if n != "SHA256SUMS"]
    require(sorted(listed) == sorted(expected) and len(listed) == len(set(listed)), "manifest 파일 집합 불일치")
    main = (HERE / "main.rs").read_text()
    gate = (HERE / "gates.rs").read_text()
    require('include!("../rust-ui-memory-races/assembly.rs");' in main, "PR45 조립 include 누락")
    require('include!("../../experiments/rust-ui-memory-app/src/app.rs");' in main, "원본 화면 include 누락")
    require("<App as eframe::App>::ui(&mut self.original, root, frame);" in main, "원본 UI 위임 누락")
    require("extern crate self as zipelo_ui_memory_app;" in main, "시험 Controller 연결 별칭 누락")
    require("시험용 화면 / 배포 바이너리 검증 아님" in main, "시험 표시 누락")
    for token in ["impl Controller", "impl Task", "fn poll(", "thread::sleep", "Barrier::", "#[test]"]:
        require(token not in main + gate, "메서드 대체/대기/선언 추가: " + token)
    require("release.recv_timeout(GATE_TIMEOUT)" in gate and "Duration::from_secs(60)" in gate, "유한 장벽 누락")
    require("runtime::deliver_completion(" in gate and "operation.gzip(" in gate, "실제 core/전달 호출 누락")
    require("--opt-in-xvfb" in main and "ZIPELO_XVFB_HARNESS" in main, "명시적 실행 opt-in 누락")
    original = tomllib.loads((ROOT / "experiments/rust-ui-memory-app/Cargo.toml").read_text())
    manifest = tomllib.loads((HERE / "Cargo.toml").read_text())
    require(manifest["dependencies"]["eframe"] == original["dependencies"]["eframe"], "GUI dependency 변경")
    require(set(manifest["dependencies"]) == {"eframe", "core_memory"}, "직접 dependency 추가")
    require(manifest["features"]["default"] == [], "기본 GUI 실행 금지")
    require(manifest["bin"][0]["required-features"] == ["gui-race-harness"], "opt-in feature 누락")
    prior = tomllib.loads((ROOT / "experiments/rust-ui-memory-app/Cargo.lock").read_text())["package"]
    actual = tomllib.loads((HERE / "Cargo.lock").read_text())["package"]
    for item in prior:
        if item["name"] == "zipelo-ui-memory-app":
            item["name"] = "zipelo-ui-memory-gui-races"
    key = lambda item: (item["name"], item["version"])
    require(sorted(prior, key=key) == sorted(actual, key=key), "원본 lock 외 package 변경")
    plan = json.loads((HERE / "run-plan.json").read_text())
    require("-nolisten tcp" in plan["future_launch_template"], "TCP 차단 누락")
    require(set(contract["scenarios"]) == {"cancel", "retract", "close"}, "시나리오 불일치")
    print(json.dumps({"fixed_sources": len(sources), "files": len(contract["files"]), "new_external_packages": 0, "new_test_declarations": 0, "built": False, "gui_executed": False}))


if __name__ == "__main__":
    check()

import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys

if sys.flags.optimize or os.environ.get("PYTHONOPTIMIZE"):
    raise SystemExit("정상 Python mode 필요")
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
parser = argparse.ArgumentParser()
parser.add_argument("--execute-reviewed", action="store_true")
parser.add_argument("--record", type=Path, required=True)
parser.add_argument("--workspace-lock", type=Path, required=True)
args = parser.parse_args()
if not args.execute_reviewed:
    raise SystemExit("고정 head 실행 배정 후 --execute-reviewed 필요")
record = args.record.resolve()
if not record.is_relative_to(HERE) or record.exists():
    raise SystemExit("새 tests 내부 기록 경로 필요")
lock = args.workspace_lock.resolve().open("r")
fcntl.flock(lock, fcntl.LOCK_EX)
subprocess.run(["git", "status", "--porcelain"], cwd=ROOT, stdout=subprocess.DEVNULL, check=True)
record.mkdir()
commands = []
report = {"commands": commands, "status": "started", "passed": [], "unexecuted": []}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def write_report():
    (record / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=4) + "\n")


def run(stage, command, timeout):
    proc = subprocess.Popen(command, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
    timed_out = False
    try:
        out, err = proc.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        timed_out = True
        os.killpg(proc.pid, signal.SIGKILL)
        out, err = proc.communicate()
    (record / (stage + ".stdout")).write_bytes(out)
    (record / (stage + ".stderr")).write_bytes(err)
    commands.append({"stage": stage, "command": command, "exit": proc.returncode, "timeout_seconds": timeout, "timed_out": timed_out, "stdout_sha256": digest(out), "stderr_sha256": digest(err)})
    write_report()
    if timed_out or proc.returncode:
        raise RuntimeError("첫 실패: " + stage)
    return out


fixed = json.loads((HERE / "sources.json").read_text())
inputs = {str(ROOT / p): v["sha256"] for p, v in fixed.items()}
for line in (HERE / "SHA256SUMS").read_text().splitlines():
    value, path = line.split("  ", 1)
    inputs[str(ROOT / path)] = value
inputs[str(HERE / "SHA256SUMS")] = digest((HERE / "SHA256SUMS").read_bytes())
contract = json.loads((HERE / "contract.json").read_text())
report["unexecuted"] = contract["names"].copy()
try:
    run("static", [sys.executable, str(HERE / "static_check.py")], 15)
    manifest = str(HERE / "Cargo.toml")
    output = run("compile", ["cargo", "+1.99.1", "test", "--locked", "--offline", "--manifest-path", manifest, "--test", "races", "--no-run", "--message-format=json", "--target-dir", str(record / "target")], 180)
    artifacts = [json.loads(line) for line in output.splitlines() if line.startswith(b"{")]
    bins = [a["executable"] for a in artifacts if a.get("reason") == "compiler-artifact" and a.get("target", {}).get("name") == "races" and a.get("executable")]
    if len(bins) != 1:
        raise RuntimeError("시험 binary 수 불일치")
    binary = bins[0]
    report["binary_sha256"] = digest(Path(binary).read_bytes())
    listing = run("discovery", [binary, "--list", "--format", "terse"], 10).decode().splitlines()
    if any(not line.endswith(": test") for line in listing if line.strip()):
        raise RuntimeError("발견 행 형식 불일치")
    names = [line[:-6] for line in listing if line.strip()]
    report["discovered"] = names
    if len(names) != len(set(names)) or set(names) != set(contract["names"]):
        raise RuntimeError("발견 이름 집합 불일치")
    for index, name in enumerate(contract["names"]):
        report["active"] = name
        write_report()
        out = run(f"test-{index:02}", [binary, name, "--exact", "--test-threads=1", "--nocapture"], 30).decode()
        expected = f"1 passed; 0 failed; 0 ignored; 0 measured; {len(names) - 1} filtered out"
        if expected not in out or not re.search(r"test " + re.escape(name) + r" \.\.\. ok", out):
            raise RuntimeError("실행 결과 불일치: " + name)
        report["passed"].append(name)
        report["unexecuted"].remove(name)
    report["status"] = "passed"
except Exception as error:
    report["status"] = "failed"
    report["failure"] = str(error)
    raise
finally:
    report["inputs_unchanged"] = all(digest(Path(p).read_bytes()) == value for p, value in inputs.items())
    if not report["inputs_unchanged"]:
        report["status"] = "failed"
        report["failure"] = "고정 입력 변경 감지"
    write_report()
    if not report["inputs_unchanged"]:
        raise RuntimeError("고정 입력 변경 감지")

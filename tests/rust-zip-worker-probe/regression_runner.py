import argparse
import copy
import json
import os
from pathlib import Path
import sys
import time

sys.dont_write_bytecode = True

from process_scope import bounded
from runner import ROOT, checked_file, check_sources, digest, no_links, read_json, require, run_root
from runner import run_schedule, validate_candidates, write_json


def run_child(args):
    gate, area, dotnet, host = map(Path, args)
    deadline = time.monotonic() + 10
    while not gate.exists() and time.monotonic() < deadline:
        time.sleep(0.01)
    require(gate.read_text(encoding="ascii") == "1", "회귀 외부 감독 장벽 필요")
    observed = bounded([str(dotnet), str(host), "--regression", str(area / "host-gate"),
                        str(area), str(dotnet)], area, area / "host-gate")
    require(observed["exitCode"] == 0 and not observed["forcedTermination"] and
            observed["ownedProcessesExited"], "직접 회귀 또는 종료 확인 실패")
    result = read_json(area / "regression.json")
    require(len(result["cases"]) == 29 and len(set(result["cases"])) == 29, "C# 직접 회귀 수 불일치")
    passed = list(result["cases"])

    def record(name, operation):
        require(operation(), "직접 회귀 실패: " + name)
        passed.append(name)

    def stops(report):
        started = []
        def execute(index):
            started.append(index)
            return report if index == 0 else {"stopRequired": False}
        records = list(run_schedule([0, 1], execute))
        return started == [0] and len(records) == 1

    record("schedule.source-io-stops-next", lambda: stops(result["cleanup"][0]))
    record("schedule.marker-io-stops-next", lambda: stops(result["cleanup"][1]))
    record("schedule.tree-io-stops-next", lambda: stops(result["cleanup"][2]))

    files = area / "candidate-files"
    files.mkdir()
    def entry(name, data):
        path = files / name
        path.write_bytes(data)
        return {"path": str(path), "sha256": digest(data)}
    value = {
        "dotnet": entry("dotnet.exe" if os.name == "nt" else "dotnet", b"host"),
        "host": entry("harness.dll", b"harness"),
        "implementations": [
            {**entry("csharp.dll", b"managed"), "name": "csharp", "invocationKind": "dotnet-managed"},
            {**entry("rust.exe", b"native"), "name": "rust", "invocationKind": "native"},
        ],
    }

    def accepts(value):
        try:
            validate_candidates(value)
            return True
        except ValueError:
            return False

    def changed(change):
        candidate = copy.deepcopy(value)
        change(candidate)
        return not accepts(candidate)

    record("candidate.distinct-accepted", lambda: accepts(value))
    record("candidate.same-file-rejected", lambda: changed(
        lambda candidate: candidate["implementations"][1].update(path=candidate["dotnet"]["path"])))
    same_bytes = entry("renamed-native.exe", b"managed")
    record("candidate.same-bytes-rejected", lambda: changed(
        lambda candidate: candidate["implementations"][1].update(same_bytes)))
    record("candidate.native-dll-rejected", lambda: changed(
        lambda candidate: candidate["implementations"][1].update(path=candidate["implementations"][0]["path"])))
    record("candidate.managed-kind-rejected", lambda: changed(
        lambda candidate: candidate["implementations"][0].update(invocationKind="native")))
    record("candidate.rust-kind-rejected", lambda: changed(
        lambda candidate: candidate["implementations"][1].update(invocationKind="dotnet-managed")))
    require(len(passed) == 38 and len(set(passed)) == 38, "전체 직접 회귀 수 불일치")
    write_json(area / "direct-summary.json", {"cases": passed, "count": len(passed), "hostSupervisor": observed})


def main():
    if len(sys.argv) == 6 and sys.argv[1] == "--child":
        run_child(sys.argv[2:])
        return 0
    parser = argparse.ArgumentParser()
    parser.add_argument("--dotnet", required=True)
    parser.add_argument("--dotnet-sha256", required=True)
    parser.add_argument("--host", required=True)
    parser.add_argument("--host-sha256", required=True)
    parser.add_argument("--work-parent", required=True)
    args = parser.parse_args()
    check_sources()
    dotnet = checked_file({"path": args.dotnet, "sha256": args.dotnet_sha256})
    host = checked_file({"path": args.host, "sha256": args.host_sha256})
    area = run_root(args.work_parent)
    gate = area / "regression-gate"
    observed = bounded([sys.executable, str(ROOT / "regression_runner.py"), "--child",
                        str(gate), str(area), str(dotnet), str(host)], area, gate)
    require(observed["exitCode"] == 0 and not observed["forcedTermination"] and
            observed["ownedProcessesExited"], "회귀 감독 또는 정리 실패")
    checked_file({"path": args.dotnet, "sha256": args.dotnet_sha256})
    checked_file({"path": args.host, "sha256": args.host_sha256})
    check_sources()
    print(json.dumps({"directCases": 38, "result": str(area / "direct-summary.json")}))
    return 0


if __name__ == "__main__":
    sys.dont_write_bytecode = True
    try:
        sys.exit(main())
    except Exception as error:
        print("DIRECT_REGRESSION_FAILURE:" + type(error).__name__, file=sys.stderr)
        sys.exit(2)

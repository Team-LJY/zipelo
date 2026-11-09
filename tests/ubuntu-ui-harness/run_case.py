"""단일 GUI 시험 프로세스만 제한하고 종료 미확인을 실패로 남김."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys


def signal_owned_group(group, number):
    try:
        os.killpg(group, number)
    except ProcessLookupError:
        pass


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--dotnet", required=True, type=Path)
    parser.add_argument("--assembly", required=True, type=Path)
    parser.add_argument("--sha256", required=True)
    contract = json.loads(Path(__file__).with_name("contract.json").read_text())
    parser.add_argument("--case", required=True, choices=contract["cases"])
    options = parser.parse_args()
    if sys.platform != "linux" or not options.dotnet.is_absolute() or not options.assembly.is_absolute():
        return 2
    if hashlib.sha256(options.assembly.read_bytes()).hexdigest() != options.sha256:
        return 2
    process = subprocess.Popen([str(options.dotnet), str(options.assembly), "--case", options.case],
                               start_new_session=True)
    try:
        code = process.wait(timeout=60)
    except subprocess.TimeoutExpired:
        signal_owned_group(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            signal_owned_group(process.pid, signal.SIGKILL)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                print("STOP: 시험 프로세스 종료 미확인")
                return 125
        print("STOP: 외부 기한 초과. 제품 취소/정리 성공으로 계산하지 않음")
        return 124
    try:
        os.killpg(process.pid, 0)
    except ProcessLookupError:
        pass
    else:
        print("STOP: 소유 프로세스 그룹 잔류. 후속 실행 금지")
        return 125
    if code in (124, 125):
        print("STOP: 후속 사례 실행 금지. 소유 임시 항목과 종료 상태를 중앙에서 확인")
    return code if 0 <= code <= 255 else 1


if __name__ == "__main__":
    raise SystemExit(main())

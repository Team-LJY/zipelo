"""소유 세션과 후손만 확인하며 정리 미확인은 실패로 남김."""

import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time


def enable_subreaper():
    library = ctypes.CDLL(None, use_errno=True)
    if library.prctl(36, 1, 0, 0, 0) != 0:
        raise OSError(ctypes.get_errno(), "subreaper unavailable")


def snapshot():
    result = {}
    for path in Path("/proc").iterdir():
        if not path.name.isdecimal():
            continue
        try:
            if path.stat().st_uid != os.getuid():
                continue
            fields = (path / "stat").read_text().rsplit(")", 1)[1].split()
            result[int(path.name)] = (int(fields[19]), int(fields[1]), int(fields[3]), fields[0])
        except (FileNotFoundError, ProcessLookupError):
            continue
    return result


class OwnedProcesses:
    def __init__(self, leader):
        self.leader = leader
        self.identities = {}
        self.refresh()

    def refresh(self):
        rows = snapshot()
        changed = True
        while changed:
            changed = False
            for pid, row in rows.items():
                if pid == os.getpid():
                    continue
                parent_owned = row[1] in self.identities and rows.get(row[1], (None,))[0] == self.identities[row[1]]
                if row[2] == self.leader or row[1] == os.getpid() or parent_owned:
                    if pid not in self.identities:
                        self.identities[pid] = row[0]
                        changed = True
        return {pid: row for pid, row in rows.items() if self.identities.get(pid) == row[0]}

    def reap(self):
        for pid in list(self.refresh()):
            if pid == self.leader:
                continue
            try:
                os.waitpid(pid, os.WNOHANG)
            except ChildProcessError:
                pass

    def signal(self, number):
        for pid, identity in list(self.refresh().items()):
            descriptor = None
            try:
                descriptor = os.pidfd_open(pid)
                current = snapshot().get(pid)
                if current is not None and current[0] == identity[0]:
                    signal.pidfd_send_signal(descriptor, number)
            except ProcessLookupError:
                pass
            finally:
                if descriptor is not None:
                    os.close(descriptor)


def clean_owned(process, owner):
    for number in (signal.SIGTERM, signal.SIGKILL):
        process.poll()
        owner.reap()
        if process.returncode is not None and not owner.refresh():
            return True
        owner.signal(number)
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            process.poll()
            owner.reap()
            if process.returncode is not None and not owner.refresh():
                return True
            time.sleep(0.02)
    return False


def finish(process, owner, timed_out):
    process.poll()
    owner.reap()
    residual = bool(owner.refresh())
    if not clean_owned(process, owner):
        print("STOP: 소유 세션/후손 정리 미확인. 후속 실행 금지")
        return 125
    if timed_out:
        print("STOP: 외부 기한 초과. 후속 실행 금지")
        return 124
    if residual:
        print("STOP: 정상 반환 뒤 후손 잔류를 정리함. 후속 실행 금지")
        return 125
    code = process.returncode
    if code in (124, 125):
        print("STOP: 앱 정리 미확인. 후속 실행 금지")
    return code if code is not None and 0 <= code <= 255 else 1


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--dotnet", required=True, type=Path)
    parser.add_argument("--assembly", required=True, type=Path)
    parser.add_argument("--sha256", required=True)
    contract = json.loads(Path(__file__).with_name("contract.json").read_text())
    choice = parser.add_mutually_exclusive_group(required=True)
    choice.add_argument("--case", choices=contract["cases"])
    choice.add_argument("--probe", choices=["before-show", "after-show"])
    options = parser.parse_args()
    if sys.platform != "linux" or not options.dotnet.is_absolute() or not options.assembly.is_absolute():
        return 2
    if hashlib.sha256(options.assembly.read_bytes()).hexdigest() != options.sha256:
        return 2
    try:
        enable_subreaper()
        process = subprocess.Popen([str(options.dotnet), str(options.assembly),
                                    "--case" if options.case else "--probe", options.case or options.probe],
                                   start_new_session=True)
    except Exception:
        print("STOP: 소유 세션 생성 실패. 후속 실행 금지")
        return 125
    owner = None
    timed_out = False
    failed = False
    deadline = time.monotonic() + 60
    try:
        owner = OwnedProcesses(process.pid)
        while process.poll() is None:
            owner.refresh()
            if time.monotonic() >= deadline:
                timed_out = True
                break
            time.sleep(0.02)
        timed_out = timed_out or time.monotonic() >= deadline
    except BaseException:
        failed = True
    finally:
        try:
            owner = owner or OwnedProcesses(process.pid)
            code = finish(process, owner, timed_out)
        except BaseException:
            print("STOP: 소유 프로세스 상태 확인 실패. 후속 실행 금지")
            code = 125
    return 125 if failed else code


if __name__ == "__main__":
    raise SystemExit(main())

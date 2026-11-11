"""생성 표본을 실제 작업자 프로세스에 전달해 현재 main의 기대값을 검사함."""

import argparse
import gzip
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import tarfile
import zipfile

from generate import CHUNK, NAME, ROOT, digest



def check_payload(source, size):
    remaining = size
    while remaining:
        chunk = source.read(min(CHUNK, remaining))
        assert chunk and chunk == bytes(len(chunk))
        remaining -= len(chunk)


def check_reference(path, artifact):
    size = artifact["payload_bytes"]
    if path.name.endswith(".zip"):
        with zipfile.ZipFile(path) as archive:
            assert archive.namelist() == [NAME]
            assert archive.getinfo(NAME).file_size == size
            with archive.open(NAME) as source:
                check_payload(source, size)
                assert source.read(1) == b""
    else:
        with gzip.open(path, "rb") as source:
            header = source.read(512)
            info = tarfile.TarInfo.frombuf(header, "utf-8", "strict")
            assert info.name == NAME and info.size == size and info.isfile()
            check_payload(source, size)
            tail_size = (-size) % 512 + 1024
            assert source.read(tail_size) == bytes(tail_size)
            assert source.read(1) == b""


def verify(directory, probe, worker):
    directory = directory.resolve()
    directory.relative_to(ROOT / "tests")
    manifest = json.loads((directory / "expectations.json").read_text(encoding="utf-8"))
    for name, artifact in manifest["artifacts"].items():
        assert (directory / name).stat().st_size == artifact["archive_bytes"]
        assert digest(directory / name) == artifact["sha256"]
        check_reference(directory / name, artifact)
    results = []
    for case in manifest["cases"]:
        path = directory / case["file"]
        artifact = manifest["artifacts"][case["file"]]
        before = path.stat()
        with tempfile.TemporaryDirectory(prefix="extract-", dir=directory) as scratch:
            destination = Path(scratch)
            command = [
                str(probe.resolve()), str(worker.resolve()), str(path), str(destination),
                str(case["limits"]["output"]), case["expected"]["outcome"],
                str(artifact["payload_bytes"]),
            ]
            process = subprocess.Popen(
                command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                start_new_session=(os.name == "posix"),
            )
            try:
                stdout, stderr = process.communicate(timeout=360)
            except subprocess.TimeoutExpired:
                if os.name == "posix":
                    os.killpg(process.pid, signal.SIGKILL)
                else:
                    process.kill()
                process.communicate()
                raise
            assert process.returncode == 0, (case["id"], stdout, stderr)
            if case["expected"]["outcome"] == "Completed":
                with (destination / "payload.bin").open("rb") as source:
                    while chunk := source.read(65536):
                        assert not any(chunk)
            else:
                assert not list(destination.iterdir())
        after = path.stat()
        assert (before.st_size, before.st_mtime_ns) == (after.st_size, after.st_mtime_ns)
        results.append({"case": case["id"], "status": "passed"})
        print(f"PASS {case['id']}", flush=True)
    for name, artifact in manifest["artifacts"].items():
        assert digest(directory / name) == artifact["sha256"]
    return results


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--directory", required=True, type=Path)
    parser.add_argument("--probe", required=True, type=Path)
    parser.add_argument("--worker", required=True, type=Path)
    args = parser.parse_args()
    verify(args.directory, args.probe, args.worker)

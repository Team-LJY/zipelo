"""참조 판독기와 실제 작업자로 독립된 한도 기대값을 검사함."""

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

from generate import CHUNK, NAME, ROOT, build, digest, violations


def check_payload(source, size):
    remaining = size
    while remaining:
        chunk = source.read(min(CHUNK, remaining))
        assert chunk and chunk == bytes(len(chunk))
        remaining -= len(chunk)


def check_reference(path, artifact):
    size = artifact["payload_bytes"]
    if artifact["format"] == "Zip":
        with zipfile.ZipFile(path) as archive:
            assert archive.namelist() == [NAME]
            info = archive.getinfo(NAME)
            assert info.file_size == size
            assert info.compress_size == artifact["ratio_input_bytes"]
            with archive.open(NAME) as source:
                check_payload(source, size)
                assert source.read(1) == b""
    else:
        assert path.stat().st_size == artifact["ratio_input_bytes"]
        with gzip.open(path, "rb") as source:
            if artifact["format"] == "TarGzip":
                header = source.read(512)
                info = tarfile.TarInfo.frombuf(header, "utf-8", "strict")
                assert info.name == NAME and info.size == size and info.isfile()
            check_payload(source, size)
            if artifact["format"] == "TarGzip":
                tail_size = (-size) % 512 + 1024
                assert source.read(tail_size) == bytes(tail_size)
            assert source.read(1) == b""


def verify(directory, probe, worker, large=False):
    directory = directory.resolve()
    directory.relative_to(ROOT / "tests")
    manifest = json.loads((directory / "expectations.json").read_text(encoding="utf-8"))
    assert not large or manifest["large"]
    for name, artifact in manifest["artifacts"].items():
        assert (directory / name).stat().st_size == artifact["archive_bytes"]
        assert digest(directory / name) == artifact["sha256"]
        check_reference(directory / name, artifact)
        print(f"REFERENCE {name}", flush=True)
    results = []
    for case in manifest["cases"]:
        path = directory / case["file"]
        artifact = manifest["artifacts"][case["file"]]
        assert violations(artifact, case["limits"]) == case["violated_limits"]
        before = path.stat()
        with tempfile.TemporaryDirectory(prefix="case-", dir=directory) as scratch:
            destination = Path(scratch) / "output"
            staging = Path(scratch) / "staging"
            destination.mkdir(mode=0o700)
            staging.mkdir(mode=0o700)
            command = [
                str(probe.resolve()), str(worker.resolve()), str(path), str(destination),
                str(case["limits"]["input"]), str(case["limits"]["output"]),
                str(case["limits"]["entries"]), case["expected"]["outcome"],
                str(artifact["payload_bytes"]), artifact["format"],
            ]
            environment = os.environ.copy()
            environment["TMPDIR"] = str(staging)
            environment["TMP"] = str(staging)
            environment["TEMP"] = str(staging)
            process = subprocess.Popen(
                command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=environment,
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
            assert not list(staging.iterdir())
            if case["expected"]["outcome"] == "Completed":
                with (destination / NAME).open("rb") as source:
                    check_payload(source, artifact["payload_bytes"])
                    assert source.read(1) == b""
            else:
                assert not list(destination.iterdir())
        after = path.stat()
        assert (before.st_size, before.st_mtime_ns) == (after.st_size, after.st_mtime_ns)
        results.append({"case": case["id"], "status": "passed"})
        print(f"PASS {case['id']}", flush=True)
    for name, artifact in manifest["artifacts"].items():
        assert digest(directory / name) == artifact["sha256"]
    return results


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--directory", type=Path, help="기존 표본 또는 새 표본 경로")
    parser.add_argument("--large", action="store_true", help="1 GiB 성공과 16 GiB 초과도 검사함")
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "tests/ci/target")) / "debug"
    suffix = ".exe" if os.name == "nt" else ""
    parser.add_argument("--probe", type=Path, default=target / ("zipelo-large-archive-probe" + suffix))
    parser.add_argument("--worker", type=Path, default=target / ("zipelo-archive-worker" + suffix))
    args = parser.parse_args()
    assert args.probe.is_file() and args.worker.is_file(), "local 빌드를 먼저 실행해야 함"
    if args.directory:
        if not args.directory.exists():
            build(args.directory, args.large)
        verify(args.directory, args.probe, args.worker, args.large)
    else:
        with tempfile.TemporaryDirectory(prefix="fixture-", dir=ROOT / "tests/large-archive-fixtures") as scratch:
            directory = Path(scratch) / "corpus"
            build(directory, args.large)
            verify(directory, args.probe, args.worker, args.large)


if __name__ == "__main__":
    main()

"""고정 크기 버퍼로 합성 압축파일과 현재 main의 기대값을 생성함."""

import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import tarfile
import zipfile


ROOT = Path(__file__).resolve().parents[2]
GIB = 1024 ** 3
CHUNK = 64 * 1024
INPUT_LIMIT = 64 * 1024 ** 2
OUTPUT_LIMIT = 128 * 1024 ** 2
BASELINE = "d6b1a45"
NAME = "payload.bin"


class ZeroStream(io.RawIOBase):
    def __init__(self, size):
        self.remaining = size

    def read(self, size=-1):
        if size < 0 or size > CHUNK:
            raise ValueError("표본 읽기는 64 KiB 이하로 제한함")
        count = min(size, self.remaining)
        self.remaining -= count
        return bytes(count)


def write_zip(path, size, compressed):
    info = zipfile.ZipInfo(NAME, (1980, 1, 1, 0, 0, 0))
    info.compress_type = zipfile.ZIP_DEFLATED if compressed else zipfile.ZIP_STORED
    info.create_system = 3
    info.external_attr = 0o100600 << 16
    info.file_size = size
    with zipfile.ZipFile(path, "x", compression=info.compress_type) as archive:
        with archive.open(info, "w") as output:
            source = ZeroStream(size)
            while chunk := source.read(CHUNK):
                output.write(chunk)


def tar_size(size):
    return 512 + ((size + 511) // 512) * 512 + 1024


def write_targz(path, size, compressed):
    info = tarfile.TarInfo(NAME)
    info.size = size
    info.mode = 0o600
    info.mtime = 0
    with path.open("xb") as raw:
        with gzip.GzipFile(
            filename="", mode="wb", fileobj=raw,
            compresslevel=6 if compressed else 0, mtime=0,
        ) as output:
            output.write(info.tobuf(format=tarfile.USTAR_FORMAT))
            source = ZeroStream(size)
            while chunk := source.read(CHUNK):
                output.write(chunk)
            output.write(bytes((-size) % 512 + 1024))


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(CHUNK):
            value.update(chunk)
    return value.hexdigest()


def build(directory, large=False):
    directory = Path(directory).resolve()
    directory.relative_to(ROOT / "tests")
    directory.mkdir(mode=0o700, parents=True, exist_ok=False)
    cases = []
    artifacts = {}

    def add(name, size, compressed, category, limits):
        path = directory / name
        if name.endswith(".zip"):
            write_zip(path, size, compressed)
            expanded = size
        else:
            write_targz(path, size, compressed)
            expanded = tar_size(size)
        artifacts[name] = {
            "archive_bytes": path.stat().st_size,
            "payload_bytes": size,
            "decoded_bytes": expanded,
            "sha256": digest(path),
        }
        for suffix, output_limit, failed in limits:
            cases.append({
                "id": name + "-" + suffix,
                "file": name,
                "category": category,
                "limits": {"input": INPUT_LIMIT, "output": output_limit, "entries": 10000},
                "expected": {
                    "outcome": "Failed" if failed else "Completed",
                    "error": "Limit" if failed else "None",
                    "publication": "None" if failed else "Complete",
                    "created_count": 0 if failed else 1,
                    "retained_partial_count": 0,
                    "output_events": 0 if failed else 1,
                },
            })

    for name, size in (("boundary.zip", CHUNK), ("boundary.tar.gz", CHUNK - 1536)):
        add(name, size, True, "output-boundary", [
            ("below", CHUNK + 1, False),
            ("equal", CHUNK, False),
            ("above", CHUNK - 1, True),
        ])
    if large:
        for extension in ("zip", "tar.gz"):
            add("large-input." + extension, GIB, False, "input-limit", [
                ("default", OUTPUT_LIMIT, True),
            ])
            add("large-expansion." + extension, GIB, True, "output-limit", [
                ("default", OUTPUT_LIMIT, True),
            ])
    manifest = {
        "baseline_commit": BASELINE,
        "worker_execution_verified": False,
        "generator_chunk_bytes": CHUNK,
        "artifacts": artifacts,
        "cases": cases,
    }
    (directory / "expectations.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=4) + "\n", encoding="utf-8"
    )
    return manifest


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", required=True, type=Path, help="tests 아래의 새 디렉터리")
    parser.add_argument("--large", action="store_true", help="1 GiB 표본 4개도 생성함")
    args = parser.parse_args()
    manifest = build(args.output, args.large)
    print(f"{len(manifest['cases'])} cases: {args.output / 'expectations.json'}")


if __name__ == "__main__":
    main()

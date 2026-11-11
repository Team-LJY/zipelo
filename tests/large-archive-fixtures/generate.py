"""고정 크기 버퍼로 새 스트리밍 한도의 독립 경계 표본을 생성함."""

import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import struct
import tarfile
import zipfile
import zlib


ROOT = Path(__file__).resolve().parents[2]
GIB = 1024 ** 3
CHUNK = 64 * 1024
INPUT_LIMIT = 2 ** 64 - 1
OUTPUT_LIMIT = 16 * GIB
RATIO_LIMIT = 1000
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


def write_zip(path, size, compressed, level=6):
    info = zipfile.ZipInfo(NAME, (1980, 1, 1, 0, 0, 0))
    info.compress_type = zipfile.ZIP_DEFLATED if compressed else zipfile.ZIP_STORED
    info._compresslevel = level
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


def write_ratio_gzip(path, size, packed_size):
    compressor = zlib.compressobj(level=6, wbits=-15)
    source = ZeroStream(size)
    crc = 0
    chunks = []
    while chunk := source.read(CHUNK):
        crc = zlib.crc32(chunk, crc)
        chunks.append(compressor.compress(chunk))
    data = b"".join(chunks) + compressor.flush()
    padding = packed_size - 20 - len(data)
    assert 0 <= padding <= 65535
    header = bytes.fromhex("1f8b08040000000000ff") + struct.pack("<H", padding)
    with path.open("xb") as output:
        output.write(header + bytes(padding) + data + struct.pack("<II", crc, size))
    assert path.stat().st_size == packed_size


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(CHUNK):
            value.update(chunk)
    return value.hexdigest()


def violations(artifact, limits):
    failed = []
    if artifact["archive_bytes"] > limits["input"]:
        failed.append("input")
    if artifact["decoded_bytes"] > limits["output"]:
        failed.append("output")
    if artifact["decoded_bytes"] > artifact["ratio_input_bytes"] * RATIO_LIMIT:
        failed.append("ratio")
    return failed


def build(directory, large=False):
    directory = Path(directory).resolve()
    directory.relative_to(ROOT / "tests")
    directory.mkdir(mode=0o700, parents=True, exist_ok=False)
    cases = []
    artifacts = {}

    def add(name, size, compressed, category, boundaries, level=6, packed_size=None):
        path = directory / name
        if name.endswith(".zip"):
            write_zip(path, size, compressed, level)
            expanded = size
            with zipfile.ZipFile(path) as archive:
                ratio_input = archive.getinfo(NAME).compress_size
            archive_format = "Zip"
        elif name.endswith(".tar.gz"):
            write_targz(path, size, compressed)
            expanded = tar_size(size)
            ratio_input = path.stat().st_size
            archive_format = "TarGzip"
        else:
            write_ratio_gzip(path, size, packed_size)
            expanded = size
            ratio_input = path.stat().st_size
            archive_format = "Gzip"
        artifact = {
            "format": archive_format,
            "archive_bytes": path.stat().st_size,
            "payload_bytes": size,
            "decoded_bytes": expanded,
            "ratio_input_bytes": ratio_input,
            "sha256": digest(path),
        }
        artifacts[name] = artifact
        for suffix, output_limit, exceeded in boundaries:
            limits = {"input": INPUT_LIMIT, "output": output_limit, "entries": 10000}
            assert violations(artifact, limits) == exceeded, (name, suffix, artifact)
            failed = bool(exceeded)
            cases.append({
                "id": name + "-" + suffix,
                "file": name,
                "category": category,
                "limits": limits,
                "violated_limits": exceeded,
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
            ("below", CHUNK + 1, []),
            ("equal", CHUNK, []),
            ("above", CHUNK - 1, ["output"]),
        ])
    for suffix, delta in (("below", -1), ("equal", 0), ("above", 1)):
        add("ratio-" + suffix + ".gz", 2_000_000 + delta, True, "ratio-boundary", [
            ("default", OUTPUT_LIMIT, ["ratio"] if delta > 0 else []),
        ], packed_size=2000)
    for extension in ("zip", "tar.gz"):
        add("ratio-only." + extension, 16 * 1024 ** 2, True, "ratio-limit", [
            ("default", OUTPUT_LIMIT, ["ratio"]),
        ])
    if large:
        for extension in ("zip", "tar.gz"):
            add("large-input." + extension, GIB, False, "large-input-success", [
                ("default", OUTPUT_LIMIT, []),
            ])
        add("output-only.zip", OUTPUT_LIMIT + 1, True, "hard-output-limit", [
            ("default", OUTPUT_LIMIT, ["output"]),
        ], level=1)
    manifest = {
        "policy": {"revision": 2, "input": INPUT_LIMIT, "output": OUTPUT_LIMIT, "ratio": RATIO_LIMIT},
        "large": large,
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
    parser.add_argument("--large", action="store_true", help="1 GiB 성공과 16 GiB 초과 표본도 생성함")
    args = parser.parse_args()
    manifest = build(args.output, args.large)
    print(f"{len(manifest['cases'])} cases: {args.output / 'expectations.json'}")


if __name__ == "__main__":
    main()

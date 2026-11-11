"""작은 합성 표본과 작업자 거부 기대값을 생성함."""

import gzip
import hashlib
from io import BytesIO
import json
from pathlib import Path
import stat
import struct
import tarfile
import zipfile


ROOT = Path(__file__).resolve().parent
PREFIX = ("before.txt", b"must not be written\n", None)
BOMB = b"A" * 65536


def zip_bytes(entries):
    stream = BytesIO()
    with zipfile.ZipFile(stream, "w") as archive:
        for name, data, target in entries:
            info = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.compress_type = zipfile.ZIP_DEFLATED
            mode = stat.S_IFLNK | 0o777 if target else stat.S_IFREG | 0o600
            info.external_attr = mode << 16
            archive.writestr(info, target.encode("ascii") if target else data)
    return stream.getvalue()


def tar_bytes(entries):
    stream = BytesIO()
    with tarfile.open(fileobj=stream, mode="w", format=tarfile.USTAR_FORMAT) as archive:
        for name, data, target in entries:
            info = tarfile.TarInfo(name)
            info.mode = 0o600
            if target:
                info.type = tarfile.SYMTYPE
                info.linkname = target
                archive.addfile(info)
            else:
                info.size = len(data)
                archive.addfile(info, BytesIO(data))
    return stream.getvalue()


def gzip_bytes(data):
    result = bytearray(gzip.compress(data, compresslevel=9, mtime=0))
    result[9] = 255
    return bytes(result)


def build():
    files = {}
    cases = []

    def add(name, data, category, format_name, errors, **details):
        files[name] = data
        cases.append({
            "file": name,
            "category": category,
            "format": format_name,
            "bytes": len(data),
            "sha256": hashlib.sha256(data).hexdigest(),
            "error_any_of": errors,
            **details,
        })

    for label, name in (
        ("dotdot", "../outside/escape.txt"),
        ("absolute-posix", "/zipelo-fixture-outside/escape.txt"),
        ("absolute-drive", "C:/zipelo-fixture-outside/escape.txt"),
        ("backslash-dotdot", "..\\outside\\escape.txt"),
    ):
        add(f"zip-{label}.zip", zip_bytes([PREFIX, (name, b"escape\n", None)]),
            "unsafe-path", "ZIP", ["UnsafePath"], hostile_name=name)
    for label, name in (
        ("dotdot", "../outside/escape.txt"),
        ("absolute-posix", "/zipelo-fixture-outside/escape.txt"),
    ):
        data = tar_bytes([PREFIX, (name, b"escape\n", None)])
        add(f"tar-{label}.tar", data, "unsafe-path", "TAR",
            ["UnsafePath"], hostile_name=name)
        if label == "dotdot":
            add("targz-dotdot.tar.gz", gzip_bytes(data), "unsafe-path", "TAR_GZIP",
                ["UnsafePath"], hostile_name=name)

    entries = [PREFIX, ("link", b"", "../outside"), ("link/escape.txt", b"escape\n", None)]
    for name, data, format_name in (
        ("zip-symlink-escape.zip", zip_bytes(entries), "ZIP"),
        ("tar-symlink-escape.tar", tar_bytes(entries), "TAR"),
    ):
        add(name, data, "symlink-escape", format_name,
            ["UnsafePath", "UnsupportedFeature"], link_name="link", link_target="../outside")

    entries = [PREFIX, ("large.txt", BOMB, None)]
    for name, data, format_name, expanded in (
        ("zip-output-limit.zip", zip_bytes(entries), "ZIP", len(PREFIX[1]) + len(BOMB)),
        ("gzip-output-limit.gz", gzip_bytes(BOMB), "GZIP", len(BOMB)),
        ("targz-output-limit.tar.gz", gzip_bytes(tar_bytes(entries)), "TAR_GZIP", len(tar_bytes(entries))),
    ):
        add(name, data, "output-limit", format_name, ["Limit"],
            expanded_bytes=expanded,
            limits={"input": 1048576, "output": 1024, "entries": 64},
            gzip_output_name="payload.bin" if format_name == "GZIP" else None)

    data = bytearray(zip_bytes([PREFIX, ("broken.txt", b"header\n", None)]))
    with zipfile.ZipFile(BytesIO(data)) as archive:
        offset = archive.infolist()[1].header_offset
    struct.pack_into("<H", data, offset + 26, 65535)
    add("zip-bad-local-header.zip", bytes(data), "damaged-header", "ZIP",
        ["Integrity"], damage="second local filename length exceeds archive")

    data = bytearray(tar_bytes([PREFIX, ("broken.txt", b"header\n", None)]))
    data[1024] ^= 1
    add("tar-bad-checksum.tar", bytes(data), "damaged-header", "TAR",
        ["Integrity"], damage="second header name changed without checksum update")

    data = bytearray(gzip_bytes(b"header\n"))
    data[3] |= 0x20
    add("gzip-reserved-flags.gz", bytes(data), "damaged-header", "GZIP",
        ["Integrity"], damage="reserved flag bit 0x20", gzip_output_name="payload.bin")

    manifest = {
        "schema_version": 1,
        "worker_protocol_revision": 2,
        "worker_execution_verified": False,
        "request_defaults": {
            "action": "Extract",
            "selection": "All",
            "list_id": None,
            "gzip_output_name": None,
            "limits": {"input": 1048576, "output": 131072, "entries": 64},
        },
        "expected": {
            "terminal_count": 1,
            "outcome": "Failed",
            "publication": "None",
            "created_count": 0,
            "retained_partial_count": 0,
            "output_event_count": 0,
            "destination_mutation_count": 0,
            "outside_mutation_count": 0,
            "archive_bytes_unchanged": True,
            "terminal_is_last": True,
            "stdout_eof": True,
            "stderr_eof": True,
            "child_exit_code": 0,
        },
        "cases": cases,
    }
    return files, manifest


def main():
    files, manifest = build()
    directory = ROOT / "fixtures"
    directory.mkdir(exist_ok=True)
    for name, data in files.items():
        (directory / name).write_bytes(data)
    (ROOT / "expectations.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=4) + "\n", encoding="utf-8"
    )


if __name__ == "__main__":
    main()

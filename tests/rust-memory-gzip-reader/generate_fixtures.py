import gzip
import hashlib
import json
import pathlib
import platform
import struct
import zlib

BASE = pathlib.Path(__file__).resolve().parent


def member(payload, flags=0, raw=None, name=b"../../escape", comment=b"note", extra=b"\x00\xff\x07"):
    header = bytearray(b"\x1f\x8b\x08" + bytes([flags]) + b"\0\0\0\0\0\xff")
    if flags & 4:
        header += struct.pack("<H", len(extra)) + extra
    if flags & 8:
        header += name + b"\0"
    if flags & 16:
        header += comment + b"\0"
    if flags & 2:
        header += struct.pack("<H", zlib.crc32(header) & 0xffff)
    if raw is None:
        encoder = zlib.compressobj(6, zlib.DEFLATED, -15)
        raw = encoder.compress(payload) + encoder.flush()
    trailer = struct.pack("<II", zlib.crc32(payload), len(payload) & 0xffffffff)
    return bytes(header) + raw + trailer, len(header), len(raw)


def produce():
    records = {}
    lines = []

    def add(key, payload, **kwargs):
        archive, header, raw = member(payload, **kwargs)
        assert gzip.decompress(archive) == payload
        lines.append(key + ":" + archive.hex())
        records[key] = {
            "archive_bytes": len(archive),
            "header_bytes": header,
            "deflate_bytes": raw,
            "payload_bytes": len(payload),
            "crc32": zlib.crc32(payload),
            "archive_sha256": hashlib.sha256(archive).hexdigest(),
            "payload_sha256": hashlib.sha256(payload).hexdigest(),
        }

    for flags in range(32):
        add(f"flags_{flags:02}", b"123456789", flags=flags)
    add("empty_fixed", b"", raw=bytes.fromhex("0300"))
    add("empty_stored", b"", raw=bytes.fromhex("010000ffff"))
    add("zeros_three", b"\0\0\0")
    add("byte_a", b"A", raw=bytes.fromhex("010100feff41"))
    add("opaque_name", b"123456789", flags=26, name=b"\xff../x", comment=b"\xfe")
    add("empty_options", b"123456789", flags=30, name=b"", comment=b"", extra=b"")
    add("large_output", b"A" * 200003)
    encoder = zlib.compressobj(0, zlib.DEFLATED, -15)
    payload = bytes(i % 251 for i in range(65537))
    add("multiple_input", payload, raw=encoder.compress(payload) + encoder.flush())
    return "\n".join(lines) + "\n", {
        "generator": "Python standard gzip/zlib; synthetic known payloads only",
        "python": platform.python_version(),
        "zlib_runtime": zlib.ZLIB_RUNTIME_VERSION,
        "fixtures": records,
    }


if __name__ == "__main__":
    data, manifest = produce()
    (BASE / "fixtures.hex").write_text(data)
    (BASE / "fixtures.json").write_text(json.dumps(manifest, indent=4) + "\n")

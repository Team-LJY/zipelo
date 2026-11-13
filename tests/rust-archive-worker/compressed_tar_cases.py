import bz2
import hashlib
import io
import lzma
from pathlib import Path
import sys
import tarfile
import zlib


root = Path(sys.argv[1])
kind = sys.argv[2]
case = sys.argv[3]
payload = bytes(range(251)) * 1800
if case == "bomb":
    payload = bytes(8 * 1024 * 1024)
buffer = io.BytesIO()
with tarfile.open(fileobj=buffer, mode="w", format=tarfile.USTAR_FORMAT) as archive:
    if case != "empty":
        name = "../escape" if case == "unsafe" else "folder/payload.bin"
        member = tarfile.TarInfo(name)
        member.size = len(payload)
        archive.addfile(member, io.BytesIO(payload))
        if case == "link":
            member = tarfile.TarInfo("link")
            member.type = tarfile.SYMTYPE
            member.linkname = "folder/payload.bin"
            archive.addfile(member)
        member = tarfile.TarInfo("empty/")
        member.type = tarfile.DIRTYPE
        archive.addfile(member)
        member = tarfile.TarInfo("한글.txt")
        member.size = 3
        archive.addfile(member, io.BytesIO(b"end"))
plain = buffer.getvalue()
if case == "bad-tar":
    plain = bytes([plain[0] ^ 1]) + plain[1:]
if case == "raw":
    plain = b"not a tar archive"
if case == "empty-stream":
    plain = b""

def compress(data):
    if kind == "bz2":
        return bz2.compress(data)
    check = {
        "crc32": lzma.CHECK_CRC32,
        "sha256": lzma.CHECK_SHA256,
        "no-check": lzma.CHECK_NONE,
    }.get(case, lzma.CHECK_CRC64)
    filters = None
    if case == "filters":
        filters = [
            {"id": lzma.FILTER_X86},
            {"id": lzma.FILTER_DELTA, "dist": 3},
            {"id": lzma.FILTER_LZMA2, "dict_size": 1 << 20},
        ]
    return lzma.compress(data, check=check, filters=filters)

if case in ("multi", "multi-bad", "padding"):
    half = len(plain) // 2
    padding = bytes(4) if kind == "xz" and case == "padding" else b""
    packed = compress(plain[:half]) + padding + compress(plain[half:]) + padding
else:
    packed = compress(plain)
if case == "truncated":
    packed = packed[:-5]
elif case == "multi-bad":
    packed = packed[:-5]
elif case == "corrupt":
    at = len(packed) // 2
    packed = packed[:at] + bytes([packed[at] ^ 128]) + packed[at + 1:]
elif case == "trailing":
    packed += b"garbage"
elif case == "dictionary":
    packed = bytearray(packed)
    header_end = 12 + (packed[12] + 1) * 4
    assert packed[14:16] == b"\x21\x01"
    packed[16] = 40
    packed[header_end - 4:header_end] = zlib.crc32(packed[12:header_end - 4]).to_bytes(4, "little")
    packed = bytes(packed)
elif case == "bad-padding":
    packed += b"\0"
if case in ("index-size", "footer-offset", "index-huge"):
    packed = bytearray(packed)
    if case == "index-size":
        length = (int.from_bytes(packed[-8:-4], "little") + 1) * 4
        index = len(packed) - 12 - length
        cursor = index + 2
        while packed[cursor] & 128:
            cursor += 1
        cursor += 1
        packed[cursor] ^= 1
        packed[-16:-12] = zlib.crc32(packed[index:-16]).to_bytes(4, "little")
    else:
        size = int.from_bytes(packed[-8:-4], "little") + 1
        if case == "index-huge":
            size = (8 * 1024 * 1024) // 4
        packed[-8:-4] = size.to_bytes(4, "little")
        packed[-12:-8] = zlib.crc32(packed[-8:-2]).to_bytes(4, "little")
    packed = bytes(packed)
(root / "plain-size").write_text(str(len(plain)))
(root / "input").write_bytes(packed)
if case in ("normal", "multi", "padding", "crc32", "sha256", "no-check", "filters"):
    decoded = bz2.decompress(packed) if kind == "bz2" else lzma.decompress(packed)
    if case == "padding":
        decoded = b""
        remaining = packed
        while remaining:
            part = lzma.LZMADecompressor()
            decoded += part.decompress(remaining)
            assert part.eof
            remaining = part.unused_data.lstrip(bytes(1))
    assert decoded == plain, case
    with tarfile.open(fileobj=io.BytesIO(decoded), mode="r:") as archive:
        actual = archive.extractfile("folder/payload.bin").read()
    assert hashlib.sha256(actual).digest() == hashlib.sha256(payload).digest()
    (root / "expected").write_bytes(actual)

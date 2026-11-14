"""독립 압축기로 미리보기 입력을 생성함."""
import gzip
import io
from pathlib import Path
import sys
import struct
import zlib
import tarfile
import zipfile

from seven_zip_fixtures import archive, integer, property_bytes, SIGNATURE

root = Path(sys.argv[1])
kind = sys.argv[2]
name = sys.argv[3]
data = (root / "payload").read_bytes()
if kind.startswith("zip"):
    with zipfile.ZipFile(root / "input", "w", compression=zipfile.ZIP_DEFLATED) as target:
        if kind.startswith("zip-selected"):
            target.writestr("before.bin", b"unselected" * 10000)
        target.writestr(name, data)
        target.writestr("after.bin", b"unselected" * 10000)
    if kind == "zip-selected-damaged":
        with zipfile.ZipFile(root / "input") as target:
            info = target.getinfo("before.bin")
        raw = bytearray((root / "input").read_bytes())
        name_size, extra_size = struct.unpack_from("<HH", raw, info.header_offset + 26)
        at = info.header_offset + 30 + name_size + extra_size + info.compress_size // 2
        raw[at] ^= 1
        (root / "input").write_bytes(raw)
elif kind == "7z-folders":
    entries = [("before.bin", b"prefix" * 10000), (name, data), ("after.bin", b"suffix" * 10000)]
    parts = [part for _, part in entries]
    info = b"\x06\x00\x03\x09" + b"".join(integer(len(part)) for part in parts) + b"\x00"
    info += b"\x07\x0b\x03\x00" + b"\x01\x01\x00" * 3 + b"\x0c"
    info += b"".join(integer(len(part)) for part in parts)
    info += b"\x0a\x01" + b"".join(struct.pack("<I", zlib.crc32(part)) for part in parts) + b"\x00\x08\x00\x00"
    files = b"\x05\x03" + property_bytes(17, b"\x00" + b"".join(n.encode("utf-16le") + b"\x00\x00" for n, _ in entries))
    header = b"\x01\x04" + info + files + b"\x00\x00"
    content = b"".join(parts)
    start = struct.pack("<QQI", len(content), len(header), zlib.crc32(header))
    (root / "input").write_bytes(SIGNATURE + struct.pack("<I", zlib.crc32(start)) + start + content + header)
elif kind == "aes":
    import encryption_fixtures
    encryption_fixtures.PAYLOAD = data
    raw = encryption_fixtures.aes_zip(2, 8)
    assert raw.count(b"payload.bin") == 2
    (root / "input").write_bytes(raw.replace(b"payload.bin", b"payload.txt"))
elif kind.startswith("7z"):
    entries = [("before.bin", b"solid prefix" * 20000, 0x20), (name, data, 0x20), ("after.bin", b"solid suffix" * 10000, 0x20)]
    (root / "input").write_bytes(archive(entries, method=kind.split("-")[1], encoded=True))
elif kind == "gzip":
    (root / (name + ".gz")).write_bytes(gzip.compress(data, mtime=0))
else:
    with tarfile.open(root / "input", "w" if kind.startswith("tar") else "w:" + kind, format=tarfile.USTAR_FORMAT) as target:
        info = tarfile.TarInfo(name)
        info.size = len(data)
        if kind in {"tar-folder", "tar-link"}:
            info.type = tarfile.DIRTYPE if kind == "tar-folder" else tarfile.SYMTYPE
            info.linkname = "target.txt" if kind == "tar-link" else ""
            info.size = 0
            target.addfile(info)
        else:
            target.addfile(info, io.BytesIO(data))

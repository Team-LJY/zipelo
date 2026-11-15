"""시스템 임시 폴더보다 큰 독립 ZIP과 7z 입력을 만듦."""
from pathlib import Path
import sys
import zipfile

from seven_zip_fixtures import archive, integer, property_bytes, packed, streams, SIGNATURE
import struct
import zlib

root = Path(sys.argv[1])
payload = b"spool fallback preview\n"
filler = bytes(range(256)) * (16 * 1024 * 1024 // 256)
with zipfile.ZipFile(
    root / "archive" / "input.zip", "w", compression=zipfile.ZIP_STORED
) as target:
    target.writestr("preview.txt", payload)
    target.writestr("filler.bin", filler)
(root / "archive" / "input.7z").write_bytes(
    archive([("preview.txt", payload, 0x20), ("filler.bin", filler, 0x20)], method="copy")
)

if len(sys.argv) > 2 and sys.argv[2] == "non-solid":
    entries = [("preview.txt", payload), ("filler.bin", filler)]
    packed_bytes = b"".join(data for _, data in entries)
    sizes = b"".join(integer(len(data)) for _, data in entries)
    digests = b"".join(struct.pack("<I", zlib.crc32(data)) for _, data in entries)
    info = b"\x06\x00\x02\x09" + sizes + b"\x0a\x01" + digests + b"\x00"
    info += b"\x07\x0b\x02\x00" + b"\x01\x01\x00" * 2
    info += b"\x0c" + sizes + b"\x0a\x01" + digests + b"\x00\x08\x00\x00"
    files = b"\x05\x02" + property_bytes(
        17, b"\x00" + b"".join(name.encode("utf-16le") + b"\x00\x00" for name, _ in entries)
    )
    files += property_bytes(21, b"\x01\x00" + struct.pack("<II", 0x20, 0x20))
    header = b"\x01\x04" + info + files + b"\x00\x00"
    encoded, coder = packed(header, "lzma")
    next_header = b"\x17" + streams(len(packed_bytes), encoded, coder, header, [header])
    packed_bytes += encoded
    start = struct.pack("<QQI", len(packed_bytes), len(next_header), zlib.crc32(next_header))
    (root / "archive" / "input.7z").write_bytes(
        SIGNATURE + struct.pack("<I", zlib.crc32(start)) + start + packed_bytes + next_header
    )

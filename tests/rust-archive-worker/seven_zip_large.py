"""고정 버퍼로 대용량 7z 표본을 생성함."""
import lzma
from pathlib import Path
import shutil
import struct
import sys
import zlib

from seven_zip_fixtures import integer, property_bytes, SIGNATURE


def generate(root, method, size, declared=None):
    root = Path(root)
    packed_path = root / "packed"
    settings = {
        "id": lzma.FILTER_LZMA2 if method == "lzma2" else lzma.FILTER_LZMA1,
        "dict_size": 64 * 1024 * 1024,
    }
    compressor = None if method == "copy" else lzma.LZMACompressor(
        format=lzma.FORMAT_RAW, filters=[settings]
    )
    crc = 0
    packed_crc = 0
    packed_size = 0
    with packed_path.open("xb") as output:
        remaining = size
        while remaining:
            chunk = bytes(min(65536, remaining))
            crc = zlib.crc32(chunk, crc)
            encoded = chunk if compressor is None else compressor.compress(chunk)
            output.write(encoded)
            packed_crc = zlib.crc32(encoded, packed_crc)
            packed_size += len(encoded)
            remaining -= len(chunk)
        if compressor is not None:
            encoded = compressor.flush()
            output.write(encoded)
            packed_crc = zlib.crc32(encoded, packed_crc)
            packed_size += len(encoded)
    if method == "copy":
        coder = b"\x01\x00"
    elif method == "lzma":
        coder = b"\x23\x03\x01\x01\x05\x5d" + struct.pack("<I", 64 * 1024 * 1024)
    else:
        coder = b"\x21\x21\x01\x1c"
    info = b"\x06\x00\x01\x09" + integer(packed_size)
    info += b"\x0a\x01" + struct.pack("<I", packed_crc) + b"\x00"
    info += b"\x07\x0b\x01\x00\x01" + coder + b"\x0c" + integer(size if declared is None else declared)
    info += b"\x0a\x01" + struct.pack("<I", crc) + b"\x00\x08\x00\x00"
    name = "payload.bin".encode("utf-16le") + b"\x00\x00"
    header = b"\x01\x04" + info + b"\x05\x01" + property_bytes(17, b"\x00" + name) + b"\x00\x00"
    start = struct.pack("<QQI", packed_size, len(header), zlib.crc32(header))
    with (root / "input").open("xb") as output, packed_path.open("rb") as packed:
        output.write(SIGNATURE + struct.pack("<I", zlib.crc32(start)) + start)
        shutil.copyfileobj(packed, output, 65536)
        output.write(header)
    packed_path.unlink()
    if declared is None and sys.platform == "linux":
        verify_zero(root / "input", size)



def verify_zero(path, size):
    import ctypes as c
    from seven_zip_reference import library
    lib = library()
    lib.archive_entry_size.argtypes = [c.c_void_p]
    lib.archive_entry_size.restype = c.c_longlong
    archive = lib.archive_read_new()
    try:
        assert lib.archive_read_support_filter_all(archive) == 0
        assert lib.archive_read_support_format_7zip(archive) == 0
        assert lib.archive_read_open_filename(archive, str(path).encode(), 65536) == 0
        entry = c.c_void_p()
        assert lib.archive_read_next_header(archive, c.byref(entry)) == 0
        assert lib.archive_entry_pathname(entry) == b"payload.bin"
        assert lib.archive_entry_size(entry) == size
        buffer = c.create_string_buffer(65536)
        total = 0
        while True:
            n = lib.archive_read_data(archive, buffer, len(buffer))
            assert n >= 0, lib.archive_error_string(archive)
            if not n:
                break
            assert buffer.raw[:n] == bytes(n)
            total += n
        assert total == size
        assert lib.archive_read_next_header(archive, c.byref(entry)) == 1
    finally:
        lib.archive_read_free(archive)

if __name__ == "__main__":
    generate(sys.argv[1], sys.argv[2], int(sys.argv[3]),
             int(sys.argv[4]) if len(sys.argv) > 4 else None)

"""공용 생성기의 고정 버퍼를 재사용해 스트리밍 회귀 표본을 만듦."""
import gzip
import importlib.util
from pathlib import Path
import sys
import struct
import io
import tarfile
import zipfile

spec = importlib.util.spec_from_file_location(
    "large_fixtures", Path(__file__).resolve().parents[1] / "large-archive-fixtures/generate.py"
)
fixtures = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixtures)


def generate(directory, kind, size):
    directory = Path(directory)
    output = directory / "input"
    if kind.startswith("offset64-descriptor"):
        class Pipe(io.BytesIO):
            def seekable(self):
                return False
            def seek(self, *args):
                raise io.UnsupportedOperation()
        data = Pipe()
        with zipfile.ZipFile(data, "w", compression=zipfile.ZIP_STORED) as archive:
            archive.writestr(fixtures.NAME, bytes(size))
        original = data.getvalue()
        central_at = struct.unpack_from("<I", original, len(original) - 6)[0]
        body = bytearray(original[:central_at])
        struct.pack_into("<H", body, 4, 45)
        central = bytearray(original[central_at:-22])
        struct.pack_into("<H", central, 6, 45)
        struct.pack_into("<H", central, 30, 12)
        struct.pack_into("<I", central, 42, 0xffffffff)
        central += struct.pack("<HHQ", 1, 8, 0)
        if kind.endswith("unsigned"):
            assert body[-16:-12] == b"PK\x07\x08"
            body = body[:-16] + body[-12:]
        end = bytearray(original[-22:])
        struct.pack_into("<II", end, 12, len(central), len(body))
        output.write_bytes(body + central + end)
        with zipfile.ZipFile(output) as reference:
            assert reference.read(fixtures.NAME) == bytes(size)
    elif kind.startswith("central64"):
        data = io.BytesIO()
        with zipfile.ZipFile(data, "w", compression=zipfile.ZIP_STORED) as archive:
            archive.writestr(fixtures.NAME, bytes(size))
        original = data.getvalue()
        end = original[-22:]
        central_at = struct.unpack_from("<I", end, 16)[0]
        central = bytearray(original[central_at:-22])
        name_len = struct.unpack_from("<H", central, 28)[0]
        struct.pack_into("<III", central, 16, struct.unpack_from("<I", central, 16)[0], 0xffffffff, 0xffffffff)
        struct.pack_into("<H", central, 30, 28)
        struct.pack_into("<I", central, 42, 0xffffffff)
        declared = (1 << 64) - 1 if kind == "central64-size-overflow" else size
        offset = (1 << 64) - 1 if kind == "central64-offset-overflow" else 0
        central = central[:46 + name_len] + struct.pack("<HHQQQ", 1, 24, declared, size, offset)
        directory = original[:central_at] + central
        record = struct.pack("<4sQHHIIQQQQ", b"PK\x06\x06", 44, 45, 45, 0, 0, 1, 1, len(central), central_at)
        locator = struct.pack("<4sIQI", b"PK\x06\x07", 0, len(directory), 1)
        end = struct.pack("<4sHHHHIIH", b"PK\x05\x06", 0, 0, 65535, 65535, 0xffffffff, 0xffffffff, 0)
        output.write_bytes(directory + record + locator + end)
        if kind == "central64":
            with zipfile.ZipFile(output) as reference:
                assert reference.read(fixtures.NAME) == bytes(size)
    elif kind == "descriptor64" or kind.startswith("locator64"):
        class Pipe(io.BytesIO):
            def seekable(self):
                return False
            def seek(self, *args):
                raise io.UnsupportedOperation()
        data = Pipe()
        with zipfile.ZipFile(data, "w", compression=zipfile.ZIP_STORED) as archive:
            with archive.open(fixtures.NAME, "w", force_zip64=True) as target:
                target.write(bytes(size))
        original = data.getvalue()
        if kind.startswith("locator64"):
            end = original[-22:]
            count, central_size, central_at = struct.unpack_from("<HII", end, 10)
            record_at = len(original) - 22
            record = bytearray(struct.pack(
                "<4sQHHIIQQQQ", b"PK\x06\x06", 44, 45, 45,
                0, 0, count, count, central_size, central_at,
            ))
            locator = bytearray(struct.pack("<4sIQI", b"PK\x06\x07", 0, record_at, 1))
            if kind == "locator64-bad-offset":
                struct.pack_into("<Q", locator, 8, (1 << 64) - 1)
            elif kind == "locator64-bad-size":
                struct.pack_into("<Q", record, 4, (1 << 64) - 1)
            elif kind == "locator64-conflicting-count":
                struct.pack_into("<QQ", record, 24, count + 1, count + 1)
            elif kind == "locator64-multidisk":
                struct.pack_into("<I", locator, 16, 2)
            original = original[:-22] + record + locator + end
            if kind == "locator64":
                assert len(original) == 260
                assert count == 1 and central_size < 0xffffffff and central_at < 0xffffffff
        output.write_bytes(original)
        if kind in ("descriptor64", "locator64"):
            with zipfile.ZipFile(output) as reference:
                assert reference.read(fixtures.NAME) == bytes(size)
    elif kind == "classic-locator-decoy":
        info = zipfile.ZipInfo(fixtures.NAME)
        info.comment = struct.pack("<4sIQI", b"PK\x06\x07", 0, 0, 1)
        with zipfile.ZipFile(output, "x") as archive:
            archive.writestr(info, bytes(size))
    elif kind == "gzip-bomb-joined":
        with output.open("xb") as raw:
            for level in (6, 0):
                with gzip.GzipFile(fileobj=raw, mode="wb", compresslevel=level, mtime=0) as target:
                    source = fixtures.ZeroStream(size)
                    while chunk := source.read(fixtures.CHUNK):
                        target.write(chunk)
    elif kind == "store":
        fixtures.write_zip(output, size, False)
    elif kind in ("deflate", "bomb", "zip64"):
        info = zipfile.ZipInfo(fixtures.NAME)
        info.file_size = size
        info.compress_type = zipfile.ZIP_DEFLATED
        info._compresslevel = 6 if kind == "bomb" else 1
        with zipfile.ZipFile(output, "x") as archive:
            with archive.open(info, "w", force_zip64=(kind == "zip64")) as target:
                source = fixtures.ZeroStream(size)
                while chunk := source.read(fixtures.CHUNK):
                    target.write(chunk)
    else:
        with output.open("xb") as raw:
            if kind == "tar":
                target = raw
            else:
                target = gzip.GzipFile(filename="", fileobj=raw, mode="wb", compresslevel=1, mtime=0)
            if kind in ("tar", "targz"):
                info = tarfile.TarInfo(fixtures.NAME)
                info.size = size
                info.mode = 0o600
                target.write(info.tobuf(format=tarfile.USTAR_FORMAT))
            source = fixtures.ZeroStream(size)
            while chunk := source.read(fixtures.CHUNK):
                target.write(chunk)
            if kind in ("tar", "targz"):
                target.write(bytes((-size) % 512 + 1024))
            if target is not raw:
                target.close()


if __name__ == "__main__":
    generate(sys.argv[1], sys.argv[2], int(sys.argv[3]))

import io
from pathlib import Path
import struct
import sys
import zipfile


PAYLOAD = b"Central directory recovery reference\n"


def verify(path):
    with zipfile.ZipFile(path) as archive:
        assert archive.namelist() == ["folder/", "folder/hello.txt"]
        assert archive.read("folder/") == b""
        assert archive.read("folder/hello.txt") == PAYLOAD


def generate(path):
    stream = io.BytesIO()
    with zipfile.ZipFile(stream, "w") as archive:
        with archive.open("folder/", "w", force_zip64=True):
            pass
        archive.writestr("folder/hello.txt", PAYLOAD)
    original = stream.getvalue()
    damaged = bytearray(original)
    name_length, extra_length = struct.unpack_from("<HH", damaged, 26)
    extra = 30 + name_length
    assert extra_length == 20
    assert struct.unpack_from("<HHQQ", damaged, extra) == (1, 16, 0, 0)
    assert struct.unpack_from("<II", damaged, 18) == (0xFFFFFFFF, 0xFFFFFFFF)
    struct.pack_into("<QQ", damaged, extra + 4, 0xFFFFFFFFFFFFFFFF, 0xEEEEEEEEEEEEEEEE)
    assert damaged[:extra + 4] == original[:extra + 4]
    assert damaged[extra + 20:] == original[extra + 20:]
    with zipfile.ZipFile(io.BytesIO(damaged)) as archive:
        folder = archive.infolist()[0]
        assert folder.file_size == folder.compress_size == folder.CRC == 0
        assert folder.extra == b""
    path.write_bytes(damaged)
    verify(path)


if __name__ == "__main__":
    if sys.argv[1] == "--verify":
        verify(Path(sys.argv[2]))
    else:
        generate(Path(sys.argv[1]))

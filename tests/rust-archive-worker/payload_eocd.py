import hashlib
import io
import json
from pathlib import Path
import struct
import sys
import zipfile


class StreamingBuffer(io.BytesIO):
    def seekable(self):
        return False

    def seek(self, *args):
        raise io.UnsupportedOperation("seek")


def create(folder, variant):
    descriptor = bool(variant & 1)
    damaged_payload_header = bool(variant & 2)
    forged_empty_member = bool(variant & 32)
    first_data = b"" if forged_empty_member else b"first content"
    nested_comment = bool(variant & 4) or forged_empty_member
    padding = 13 if variant & 8 else 0

    def archive(payload):
        buffer = StreamingBuffer() if descriptor else io.BytesIO()
        with zipfile.ZipFile(buffer, "w", compression=zipfile.ZIP_STORED) as writer:
            members = [("first.txt", first_data), ("second.bin", payload)]
            for name, data in members:
                info = zipfile.ZipInfo(name, (2024, 1, 1, 0, 0, 0))
                writer.writestr(info, data)
        return bytearray(buffer.getvalue())

    original = archive(b"")
    real_end = len(original) - 22
    directory = struct.unpack_from("<I", original, real_end + 16)[0]
    first_len = 46 + sum(struct.unpack_from("<HHH", original, directory + 28))
    payload_size = padding + first_len + 22
    original = archive(bytes(payload_size))
    real_end = len(original) - 22
    directory = struct.unpack_from("<I", original, real_end + 16)[0]
    local = struct.unpack_from("<I", original, directory + first_len + 42)[0]
    name_len, extra_len = struct.unpack_from("<HH", original, local + 26)
    data_at = local + 30 + name_len + extra_len
    fake_cd = original[directory : directory + first_len]
    fake_end = bytearray(original[real_end : real_end + 22])
    comment_cd_bytes = fake_cd
    if forged_empty_member:
        comment_cd_bytes = bytearray(46)
        comment_cd_bytes[:4] = b"PK\x01\x02"
        local_flags = struct.unpack_from("<H", original, directory + 6)[0]
        struct.pack_into("<H", comment_cd_bytes, 8, local_flags)
        struct.pack_into("<I", comment_cd_bytes, 42, directory)
    comment_size = len(comment_cd_bytes) + 22 if nested_comment else 0
    struct.pack_into(
        "<HHIIH",
        fake_end,
        8,
        1,
        1,
        first_len,
        data_at + padding,
        len(original) + comment_size - data_at - payload_size,
    )
    payload = bytes(padding) + fake_cd + fake_end
    original = archive(payload)
    if nested_comment:
        comment_cd = len(original)
        comment_end = bytearray(original[-22:])
        struct.pack_into(
            "<HHIIH", comment_end, 8, 1, 1, len(comment_cd_bytes), comment_cd, 0
        )
        struct.pack_into("<H", original, real_end + 20, comment_size)
        original.extend(comment_cd_bytes + comment_end)
    damaged = local if damaged_payload_header else 0
    original[damaged : damaged + 4] = b"BAD!"
    if variant & 16:
        struct.pack_into("<I", original, local + 14, 0x12345678)
    (folder / "damaged.zip").write_bytes(original)
    expected = {
        "first.txt": hashlib.sha256(first_data).hexdigest(),
        "second.bin": hashlib.sha256(payload).hexdigest(),
    }
    (folder / "expected.json").write_text(json.dumps(expected), encoding="utf-8")


def verify(folder):
    expected = json.loads((folder / "expected.json").read_text(encoding="utf-8"))
    with zipfile.ZipFile(folder / "recovered.zip") as archive:
        assert len(archive.infolist()) == len(expected)
        assert set(archive.namelist()) == set(expected)
        assert archive.testzip() is None
        for name, digest in expected.items():
            assert hashlib.sha256(archive.read(name)).hexdigest() == digest, name


if __name__ == "__main__":
    if sys.argv[1] == "create":
        create(Path(sys.argv[2]), int(sys.argv[3]))
    elif sys.argv[1] == "verify":
        verify(Path(sys.argv[2]))
    else:
        raise ValueError("unknown mode")

"""공개 시험 암호로 7zAES(AES-256-CBC) 7z 표본을 만들고 실제 7-Zip 표본으로 키 유도를 대조함."""

import argparse
import hashlib
import lzma
from pathlib import Path
import struct
import subprocess
import zlib

from seven_zip_fixtures import SIGNATURE, bits, integer, property_bytes

ROOT = Path(__file__).resolve().parent / "fixtures"
PASSWORD = "zipelo-암호"
AES_ID = b"\x06\xf1\x07\x01"
FIRST = b"Zipelo 7z AES interoperability\n" * 60
SECOND = bytes((index * 31 + (index >> 4)) % 256 for index in range(3000))


def key(password, salt, cycles):
    data = salt + password.encode("utf-16le")
    digest = hashlib.sha256()
    for counter in range(1 << cycles):
        digest.update(data + counter.to_bytes(8, "little"))
    return digest.digest()


def cbc(data, secret, iv, decrypt=False):
    return subprocess.run(
        ["openssl", "enc", "-aes-256-cbc", "-nopad", "-K", secret.hex(), "-iv", iv.hex()]
        + (["-d"] if decrypt else []),
        input=data, check=True, stdout=subprocess.PIPE).stdout


def aes_properties(cycles, salt, iv):
    first = cycles | (0x80 if salt else 0) | (0x40 if iv else 0)
    second = ((max(len(salt) - 1, 0)) << 4) | max(len(iv) - 1, 0)
    return bytes([first, second]) + salt + iv


def coder(method_id, properties=b""):
    flags = len(method_id) | (0x20 if properties else 0)
    return bytes([flags]) + method_id + (integer(len(properties)) + properties if properties else b"")


class Encryption:
    def __init__(self, cycles=19, salt=b""):
        self.cycles = cycles
        self.salt = salt
        self.secret = key(PASSWORD, salt, cycles) if cycles <= 24 else bytes(32)
        self.counter = 0

    def folder(self, method, data):
        """7z a -p와 같이 [7zAES, 주 코더] 순서와 결합 쌍 (1, 0)으로 폴더를 만듦."""
        if method == "copy":
            inner, main = data, coder(b"\x00")
        else:
            settings = {"id": lzma.FILTER_LZMA1, "dict_size": 4096, "lc": 3, "lp": 0, "pb": 2}
            inner = lzma.compress(data, format=lzma.FORMAT_RAW, filters=[settings])
            main = coder(b"\x03\x01\x01", bytes([(2 * 5 + 0) * 9 + 3]) + struct.pack("<I", 4096))
        self.counter += 1
        iv = hashlib.sha256(b"iv" + bytes([self.counter])).digest()[:16]
        padded = inner + bytes(-len(inner) % 16)
        packed = cbc(padded, self.secret, iv)
        assert cbc(packed, self.secret, iv, decrypt=True)[:len(inner)] == inner
        description = b"\x02" + coder(AES_ID, aes_properties(self.cycles, self.salt, iv)) + main
        description += integer(1) + integer(0)
        return packed, description, [len(inner), len(data)]


def streams(position, folders, substreams):
    """folders는 (packed, 폴더 설명, 코더별 출력 크기, 원본) 목록이고 substreams는 폴더별 파일 내용임."""
    info = b"\x06" + integer(position) + integer(len(folders)) + b"\x09"
    info += b"".join(integer(len(packed)) for packed, _, _, _ in folders)
    info += b"\x0a\x01" + b"".join(struct.pack("<I", zlib.crc32(p)) for p, _, _, _ in folders) + b"\x00"
    info += b"\x07\x0b" + integer(len(folders)) + b"\x00" + b"".join(d for _, d, _, _ in folders)
    info += b"\x0c" + b"".join(integer(size) for _, _, sizes, _ in folders for size in sizes)
    info += b"\x0a\x01" + b"".join(struct.pack("<I", zlib.crc32(data)) for _, _, _, data in folders)
    info += b"\x00"
    if any(len(parts) != 1 for parts in substreams):
        info += b"\x08\x0d" + b"".join(integer(len(parts)) for parts in substreams)
        info += b"\x09" + b"".join(integer(len(part)) for parts in substreams for part in parts[:-1])
        digests = [part for parts in substreams if len(parts) != 1 for part in parts]
        info += b"\x0a\x01" + b"".join(struct.pack("<I", zlib.crc32(part)) for part in digests)
        info += b"\x00"
    else:
        info += b"\x08\x00"
    return info + b"\x00"


def archive(entries, groups, method, encryption, encrypted_header=False, damage=None):
    """entries는 (이름, 내용, 속성), groups는 폴더마다 묶을 비어 있지 않은 항목 이름 목록임."""
    contents = dict((name, data) for name, data, _ in entries)
    substreams = [[contents[name] for name in group] for group in groups]
    folders = []
    for parts in substreams:
        data = b"".join(parts)
        packed, description, sizes = encryption.folder(method, data)
        folders.append((packed, description, sizes, data))
    body = b"".join(packed for packed, _, _, _ in folders)
    if damage == "packed":
        # 묶음 CRC는 원래 암호문으로 계산해 두어 암호문 손상이 무결성 오류가 되게 함.
        body = body[:-1] + bytes([body[-1] ^ 1])
    header = b"\x01\x04" + streams(0, folders, substreams)
    header += b"\x05" + integer(len(entries))
    empties = [not data for _, data, _ in entries]
    if any(empties):
        header += property_bytes(14, bits(empties))
        header += property_bytes(15, bits([attr & 0x10 == 0 for _, data, attr in entries if not data]))
    header += property_bytes(17, b"\x00" + b"".join(name.encode("utf-16le") + b"\x00\x00" for name, _, _ in entries))
    header += property_bytes(21, b"\x01\x00" + b"".join(struct.pack("<I", attr) for _, _, attr in entries))
    header += b"\x00\x00"
    if encrypted_header:
        packed, description, sizes = encryption.folder("lzma", header)
        info = b"\x17" + streams(len(body), [(packed, description, sizes, header)], [[header]])
        body += packed
        header = info
    start = struct.pack("<QQI", len(body), len(header), zlib.crc32(header))
    return SIGNATURE + struct.pack("<I", zlib.crc32(start)) + start + body + header


def check_reference():
    """실제 7-Zip이 만든 분할 표본의 암호화 헤더를 같은 키 유도로 풀어 CRC를 대조함."""
    parts = sorted((ROOT / "split7z").glob("encrypted-header.7z.[0-9][0-9][0-9]"))
    data = b"".join(part.read_bytes() for part in parts)
    offset, length, _ = struct.unpack("<QQI", data[12:32])
    header = data[32 + offset:32 + offset + length]
    # 7-Zip 26.00의 -mhc=off -mhe=on 헤더: 7zAES 단일 코더, 2^19회, 솔트 없음, 16바이트 IV.
    at = header.index(AES_ID) + len(AES_ID)
    size = header[at]
    properties = header[at + 1:at + 1 + size]
    assert properties[0] & 0x3f == 19 and properties[0] & 0x80 == 0
    iv = properties[2:]
    packed_size = int.from_bytes(header[header.index(b"\x09") + 1:][:2], "big") & 0x3fff
    plain_size = int.from_bytes(header[header.index(b"\x0c") + 1:][:2], "big") & 0x3fff
    crc = struct.unpack("<I", header[header.index(b"\x0a\x01", header.index(b"\x0c")) + 2:][:4])[0]
    packed = data[32 + 0:32 + offset][-packed_size:]
    plain = cbc(packed, key("zipelo-split-test", b"", 19), iv, decrypt=True)[:plain_size]
    assert zlib.crc32(plain) == crc


def build():
    entries = [("folder", b"", 0x10), ("folder/empty", b"", 0x20),
               ("folder/hello.txt", FIRST, 0x20), ("한글.bin", SECOND, 0x20)]
    files = [["folder/hello.txt", "한글.bin"]]
    single = [["folder/hello.txt"], ["한글.bin"]]
    return {
        "aes-copy.7z": archive(entries, files, "copy", Encryption()),
        "aes-lzma-header.7z": archive(entries, single, "lzma", Encryption(10, bytes(range(16))),
                                      encrypted_header=True),
        "aes-bad-packed.7z": archive(entries, files, "copy", Encryption(), damage="packed"),
        "aes-cycles.7z": archive(entries, files, "copy", Encryption(25)),
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, default=ROOT / "7z")
    args = parser.parse_args()
    check_reference()
    for name, data in build().items():
        (args.output / name).write_bytes(data)


if __name__ == "__main__":
    main()

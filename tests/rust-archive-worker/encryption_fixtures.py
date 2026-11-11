"""공개 시험 암호로 독립 AES 표본 생성과 검증을 수행함."""

import argparse
import hashlib
import hmac
import io
from pathlib import Path
import struct
import subprocess
import tempfile
import zipfile
import zlib

ROOT = Path(__file__).resolve().parent
PASSWORD = b"fixture-password"
PAYLOAD = b"Zipelo encrypted interoperability\n" * 8


def crypt(data, key):
    counters = b"".join(i.to_bytes(16, "little") for i in range(1, (len(data) + 15) // 16 + 1))
    if not counters:
        return b""
    mask = subprocess.run(["openssl", "enc", "-aes-256-ecb", "-nopad", "-K", key.hex()],
                          input=counters, check=True, stdout=subprocess.PIPE).stdout
    return bytes(a ^ b for a, b in zip(data, mask))


def aes_zip(version, method):
    data = PAYLOAD
    packed = data if method == 0 else zlib.compress(data, wbits=-15)
    salt = bytes(range(16))
    keys = hashlib.pbkdf2_hmac("sha1", PASSWORD, salt, 1000, 66)
    cipher = crypt(packed, keys[:32])
    encrypted = salt + keys[64:] + cipher + hmac.digest(keys[32:64], cipher, "sha1")[:10]
    name = b"payload.bin"
    crc = zlib.crc32(data) if version == 1 else 0
    extra = struct.pack("<HHH2sBH", 0x9901, 7, version, b"AE", 3, method)
    local = struct.pack("<IHHHHHIIIHH", 0x04034B50, 51, 0x801, 99, 0, 33,
                        crc, len(encrypted), len(data), len(name), len(extra)) + name + extra + encrypted
    central = struct.pack("<IHHHHHHIIIHHHHHII", 0x02014B50, 0x333, 51, 0x801, 99, 0, 33,
                          crc, len(encrypted), len(data), len(name), len(extra), 0, 0, 0,
                          0o100600 << 16, 0) + name + extra
    return local + central + struct.pack("<IHHHHIIH", 0x06054B50, 0, 0, 1, 1,
                                         len(central), len(local), 0)


def verify(path):
    raw = Path(path).read_bytes()
    with zipfile.ZipFile(io.BytesIO(raw)) as archive:
        assert archive.infolist()
        for info in archive.infolist():
            assert info.compress_type == 99 and info.flag_bits & 1
            fields = struct.unpack_from("<HHHHHIIIHH", raw, info.header_offset + 4)
            name_len, extra_len = fields[-2:]
            extra_at = info.header_offset + 30 + name_len
            assert raw[extra_at:extra_at + extra_len] == info.extra
            tag, length, version, vendor, strength, method = struct.unpack("<HHH2sBH", info.extra)
            assert (tag, length, vendor, strength) == (0x9901, 7, b"AE", 3)
            assert version in (1, 2)
            start = extra_at + extra_len
            encrypted = raw[start:start + info.compress_size]
            keys = hashlib.pbkdf2_hmac("sha1", PASSWORD, encrypted[:16], 1000, 66)
            assert hmac.compare_digest(keys[64:], encrypted[16:18])
            cipher = encrypted[18:-10]
            assert hmac.compare_digest(hmac.digest(keys[32:64], cipher, "sha1")[:10], encrypted[-10:])
            packed = crypt(cipher, keys[:32])
            data = packed if method == 0 else zlib.decompress(packed, wbits=-15)
            assert len(data) == info.file_size
            assert info.CRC == (0 if version == 2 else zlib.crc32(data))
            assert data == PAYLOAD


def generate():
    directory = ROOT / "fixtures"
    for version in (1, 2):
        for method, name in ((0, "store"), (8, "deflate")):
            path = directory / f"aes{version}-{name}.zip"
            path.write_bytes(aes_zip(version, method))
            verify(path)
    runs = ROOT / "runs"
    runs.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(dir=runs) as scratch:
        source = Path(scratch) / "payload.bin"
        source.write_bytes(PAYLOAD)
        for level, name in (("-0", "store"), ("-9", "deflate")):
            result = subprocess.run(["zip", "-q", "-X", "-P", PASSWORD.decode(), level, "-", source.name],
                                    cwd=scratch, check=True, stdout=subprocess.PIPE).stdout
            with zipfile.ZipFile(io.BytesIO(result)) as archive:
                assert archive.read("payload.bin", pwd=PASSWORD) == PAYLOAD
                assert archive.infolist()[0].flag_bits & 8
            (directory / f"zipcrypto-{name}.zip").write_bytes(result)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--generate", action="store_true")
    parser.add_argument("--verify", type=Path)
    args = parser.parse_args()
    if args.generate:
        generate()
    elif args.verify:
        verify(args.verify)
    else:
        parser.error("--generate 또는 --verify가 필요함")

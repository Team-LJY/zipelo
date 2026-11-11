"""Python 표준 LZMA 구현으로 합성 7z 표본을 생성함."""
import lzma
from pathlib import Path
import struct
import zlib

ROOT = Path(__file__).resolve().parent / "fixtures" / "7z"
SIGNATURE = b"7z\xbc\xaf\x27\x1c\x00\x04"
PAYLOAD = b"Zipelo 7z interoperability\n" * 100


def integer(n):
    for i in range(8):
        if n < 1 << (7 + 7 * i):
            return bytes([(0xFF << (8 - i) & 255) | (n >> (8 * i))]) + n.to_bytes(8, "little")[:i]
    return b"\xff" + struct.pack("<Q", n)


def bits(values):
    return bytes(sum((128 >> bit) if i + bit < len(values) and values[i + bit] else 0
                     for bit in range(8)) for i in range(0, len(values), 8))


def property_bytes(tag, value):
    return bytes([tag]) + integer(len(value)) + value


def packed(data, method, lc=3, lp=0, pb=2):
    if method == "copy":
        return data, b"\x01\x00"
    is_lzma2 = method == "lzma2"
    settings = {"id": lzma.FILTER_LZMA2 if is_lzma2 else lzma.FILTER_LZMA1,
                "dict_size": 4096, "lc": lc, "lp": lp, "pb": pb}
    stream = lzma.compress(data, format=lzma.FORMAT_RAW, filters=[settings])
    assert lzma.decompress(stream, format=lzma.FORMAT_RAW, filters=[settings]) == data
    props = b"\x00" if is_lzma2 else bytes([(pb * 5 + lp) * 9 + lc]) + struct.pack("<I", 4096)
    method_id = b"\x21" if is_lzma2 else b"\x03\x01\x01"
    return stream, bytes([0x20 | len(method_id)]) + method_id + integer(len(props)) + props


def streams(position, packed_bytes, coder, data, substreams):
    info = b"\x06" + integer(position) + b"\x01\x09" + integer(len(packed_bytes))
    info += b"\x0a\x01" + struct.pack("<I", zlib.crc32(packed_bytes)) + b"\x00"
    info += b"\x07\x0b\x01\x00\x01" + coder + b"\x0c" + integer(len(data))
    info += b"\x0a\x01" + struct.pack("<I", zlib.crc32(data)) + b"\x00"
    if len(substreams) != 1:
        info += b"\x08\x0d" + integer(len(substreams))
        if substreams:
            info += b"\x09" + b"".join(integer(len(part)) for part in substreams[:-1])
            info += b"\x0a\x01" + b"".join(struct.pack("<I", zlib.crc32(part)) for part in substreams)
        info += b"\x00"
    else:
        info += b"\x08\x00"
    return info + b"\x00"


def archive(entries, method="lzma2", encoded=False, properties=(3, 0, 2), damage=None):
    parts = [data for _, data, _ in entries if data]
    data = b"".join(parts)
    compressed, coder = packed(data, method, *properties)
    if damage == "payload":
        compressed = compressed[:len(compressed) // 2]
    if damage == "trailing":
        compressed += b"unexpected"
    if damage == "dictionary":
        coder = b"\x21\x21\x01\x28"
    info = b"\x01\x04" + streams(0, compressed, coder, data, parts)
    files = b"\x05" + integer(len(entries))
    empties = [not content for _, content, _ in entries]
    if any(empties):
        files += property_bytes(14, bits(empties))
        files += property_bytes(15, bits([attr & 0x10 == 0 for _, content, attr in entries if not content]))
    files += property_bytes(17, b"\x00" + b"".join(name.encode("utf-16le") + b"\x00\x00" for name, _, _ in entries))
    files += property_bytes(21, b"\x01\x00" + b"".join(struct.pack("<I", attr) for _, _, attr in entries))
    header = info + files + b"\x00\x00"
    if damage == "crc":
        # 데이터 CRC만 틀리게 하며 헤더와 packed CRC는 다시 계산함.
        good = struct.pack("<I", zlib.crc32(data))
        at = header.index(good)
        header = header[:at] + bytes([header[at] ^ 1]) + header[at + 1:]
    if encoded:
        encoded_bytes, encoded_coder = packed(header, "lzma")
        next_header = b"\x17" + streams(len(compressed), encoded_bytes, encoded_coder, header, [header])
        compressed += encoded_bytes
        header = next_header
    start = struct.pack("<QQI", len(compressed), len(header), zlib.crc32(header))
    return SIGNATURE + struct.pack("<I", zlib.crc32(start)) + start + compressed + header


def build():
    result = {}
    entries = [("folder/hello.txt", PAYLOAD, 0x20)]
    for method in ("copy", "lzma", "lzma2"):
        result[f"{method}.7z"] = archive(entries, method)
    result["encoded-solid.7z"] = archive([("folder", b"", 0x10), ("folder/empty", b"", 0x20),
        ("folder/hello.txt", PAYLOAD, 0x20), ("한글.txt", PAYLOAD[::-1], 0x20)], encoded=True)
    for i, params in enumerate(((0, 0, 0), (4, 0, 4), (1, 2, 1), (2, 2, 3))):
        result[f"properties-{i}.7z"] = archive(entries, "lzma", properties=params)
    result["long-match.7z"] = archive([("folder/hello.txt", PAYLOAD * 100, 0x20)])
    noise = bytes(((i * 1103515245 + (i >> 2) * 12345) >> ((i % 4) * 8)) & 255 for i in range(100000))
    result["lzma2-chunks.7z"] = archive([("noise", noise, 0x20)])
    for name, hostile in (("dotdot", "../outside.txt"), ("absolute", "/outside.txt"),
                          ("drive", "C:/outside.txt"), ("backslash", "..\\outside.txt"),
                          ("deep", "a/" * 65 + "file")):
        result[f"unsafe-{name}.7z"] = archive([("before.txt", PAYLOAD, 0x20), (hostile, PAYLOAD, 0x20)])
    result["symlink.7z"] = archive([("before.txt", PAYLOAD, 0x20), ("link", b"../outside", (0o120777 << 16) | 0x8000)])
    result["collision.7z"] = archive([("hello", PAYLOAD, 0x20), ("HELLO", PAYLOAD, 0x20)])
    for damage in ("payload", "crc", "dictionary", "trailing"):
        result[f"bad-{damage}.7z"] = archive(entries, damage=damage)
    return result


if __name__ == "__main__":
    ROOT.mkdir(exist_ok=False)
    for name, data in build().items():
        (ROOT / name).write_bytes(data)

import hashlib
import io
import json
from pathlib import Path
import stat
import struct
import warnings
import zipfile


DIRECTORY = Path(__file__).resolve().parent
ROOT = DIRECTORY.parents[1]
FIXTURES = DIRECTORY / "fixtures"
STAMP = (2024, 1, 1, 0, 0, 0)
REGULAR = stat.S_IFREG | 0o600
PAYLOAD = b"zipelo-extraction-content\n"
MAX_SAMPLE_BYTES = 4096
UNIX_FIELD_REFERENCE = "https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT"


def archive(entries, compression=zipfile.ZIP_STORED):
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w") as zipped, warnings.catch_warnings():
        warnings.filterwarnings("ignore", message="Duplicate name:", category=UserWarning)
        for name, content, mode, extra in entries:
            info = zipfile.ZipInfo(name, STAMP)
            info.create_system = 3
            info.external_attr = mode << 16
            if stat.S_ISDIR(mode):
                info.external_attr |= 0x10
            info.extra = extra
            info.compress_type = compression
            zipped.writestr(info, content)
    result = output.getvalue()
    if len(result) > MAX_SAMPLE_BYTES:
        raise ValueError("작은 수용 샘플의 크기 상한을 넘음")
    return result


def regular(name, content=PAYLOAD):
    return (name, content, REGULAR, b"")


def mutate_field(data, local_offset, central_offset, value):
    changed = bytearray(data)
    central = data.index(b"PK\x01\x02")
    struct.pack_into("<I", changed, local_offset, value)
    struct.pack_into("<I", changed, central + central_offset, value)
    return bytes(changed)


def build_samples():
    normal = [
        regular("hello.txt"),
        ("nested/", b"", stat.S_IFDIR | 0o700, b""),
        regular("nested/data.bin", bytes(range(32))),
        regular("nested/empty.txt", b""),
    ]
    single = archive([regular("payload.txt")], zipfile.ZIP_DEFLATED)
    stored = archive([regular("payload.txt")])
    bad_payload = bytearray(stored)
    bad_payload[stored.index(PAYLOAD)] ^= 1
    bad_crc = struct.unpack_from("<I", single, 14)[0] ^ 1
    mismatch = bytearray(single)
    struct.pack_into("<I", mismatch, 22, len(PAYLOAD) + 1)
    # UNIX 링크 대상 이름은 NUL 없이 12바이트 고정 필드 뒤에 저장함.
    link_target = b"../sentinel.txt"
    unix_data = struct.pack("<IIHH", 0, 0, 0, 0) + link_target
    unix_extra = struct.pack("<HH", 0x000D, len(unix_data)) + unix_data
    return {
        "normal-stored.zip": archive(normal),
        "normal-deflated.zip": archive(normal, zipfile.ZIP_DEFLATED),
        "traversal.zip": archive([regular("../escape.txt")]),
        "backslash-traversal.zip": archive([regular("..\\escape.txt")]),
        "absolute-posix.zip": archive([regular("/escape.txt")]),
        "absolute-drive.zip": archive([regular("C:/escape.txt")]),
        "absolute-unc.zip": archive([regular("//server/share/escape.txt")]),
        "duplicate.zip": archive([regular("same.txt", b"one"), regular("same.txt", b"two")]),
        "file-before-child.zip": archive([regular("node", b"one"), regular("node/child", b"two")]),
        "child-before-file.zip": archive([regular("node/child", b"one"), regular("node", b"two")]),
        "case-distinct.zip": archive([regular("Readme.txt", b"first"), regular("README.TXT", b"second")]),
        "linux-names.zip": archive([regular("CON", b"one"), regular("name:stream", b"two")]),
        "symlink.zip": archive([("link", link_target, stat.S_IFLNK | 0o777, b"")]),
        "unix-hardlink.zip": archive([("link", b"", REGULAR, unix_extra)]),
        "special-fifo.zip": archive([("pipe", b"", stat.S_IFIFO | 0o600, b"")]),
        "bad-payload-crc.zip": bytes(bad_payload),
        "bad-header-crc.zip": mutate_field(single, 14, 16, bad_crc),
        "length-smaller.zip": mutate_field(single, 22, 24, len(PAYLOAD) - 1),
        "length-larger.zip": mutate_field(single, 22, 24, len(PAYLOAD) + 1),
        "local-central-length.zip": bytes(mismatch),
        "truncated.zip": single[:-12],
        "cumulative.zip": archive([regular("first.bin", bytes(range(8))), regular("second.bin", bytes(range(8, 16)))]),
    }


def build_manifest(samples):
    entries = []
    for name, data in sorted(samples.items()):
        entries.append({"file": name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
    return {
        "schema_version": 1,
        "source": "자체 생성한 작은 ZIP 샘플이며 디스크 해제를 수행하지 않음",
        "unix_link_reference": UNIX_FIELD_REFERENCE,
        "unix_link_section": "4.5.7, 0x000d 링크 메타데이터의 거부 검사. 모든 ZIP 링크 방언을 대표하지 않음",
        "normal_files": {
            "hello.txt": {"bytes": len(PAYLOAD), "sha256": hashlib.sha256(PAYLOAD).hexdigest()},
            "nested/data.bin": {"bytes": 32, "sha256": hashlib.sha256(bytes(range(32))).hexdigest()},
            "nested/empty.txt": {"bytes": 0, "sha256": hashlib.sha256(b"").hexdigest()},
        },
        "samples": entries,
    }


def write_new(path, content):
    if path.is_symlink():
        raise ValueError("샘플 경로의 링크를 허용하지 않음")
    if path.exists():
        if path.read_bytes() != content:
            raise ValueError("기존 샘플을 덮어쓰지 않음")
        return
    with path.open("xb") as output:
        output.write(content)


def main():
    if DIRECTORY != ROOT / "tests" / "extraction":
        raise ValueError("프로젝트 tests 아래 경로가 필요함")
    for path in (FIXTURES, *FIXTURES.parents):
        if path.is_symlink():
            raise ValueError("샘플 경로의 링크를 허용하지 않음")
    FIXTURES.mkdir(exist_ok=True)
    samples = build_samples()
    for name, content in samples.items():
        write_new(FIXTURES / name, content)
    manifest = json.dumps(build_manifest(samples), ensure_ascii=False, indent=4) + "\n"
    write_new(DIRECTORY / "fixtures.json", manifest.encode("utf-8"))
    print(f"해제 샘플 {len(samples)}개, {sum(map(len, samples.values()))}바이트")


if __name__ == "__main__":
    main()

import io
from pathlib import Path
import stat
import zipfile


ROOT = Path(__file__).resolve().parents[2]
FIXTURES = Path(__file__).resolve().parent / "fixtures"
STAMP = (2024, 1, 1, 0, 0, 0)
PAYLOAD = b"zipelo-safe-test-payload\n"
MAX_SAMPLE_BYTES = 16 * 1024


def archive(entries):
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_STORED) as zipped:
        for name, content, mode in entries:
            info = zipfile.ZipInfo(name, STAMP)
            info.create_system = 3
            info.external_attr = mode << 16
            zipped.writestr(info, content)
    result = output.getvalue()
    if len(result) > MAX_SAMPLE_BYTES:
        raise ValueError("안전 샘플 크기 제한을 초과함")
    return result


def build_samples():
    regular = stat.S_IFREG | 0o600
    normal = archive([
        ("hello.txt", PAYLOAD, regular),
        ("한글/빈파일.txt", b"", regular),
    ])
    corrupted = bytearray(normal)
    corrupted[normal.index(PAYLOAD)] ^= 1
    return {
        "normal.zip": normal,
        "traversal.zip": archive([("../escape.txt", PAYLOAD, regular)]),
        "backslash-traversal.zip": archive([("..\\escape.txt", PAYLOAD, regular)]),
        "case-collision.zip": archive([
            ("Readme.txt", b"first", regular),
            ("README.TXT", b"second", regular),
        ]),
        "parent-collision.zip": archive([
            ("folder", b"first", regular),
            ("folder/child.txt", b"second", regular),
        ]),
        "symlink.zip": archive([
            ("link", b"../sentinel.txt", stat.S_IFLNK | 0o777),
        ]),
        "bad-crc.zip": bytes(corrupted),
        "truncated.zip": normal[:-12],
    }


def main():
    # 샘플은 해제하지 않고 지정된 테스트 폴더에만 생성함.
    if FIXTURES.is_symlink() or FIXTURES.parent != ROOT / "tests" / "security":
        raise ValueError("샘플 경로가 안전하지 않음")
    FIXTURES.mkdir(exist_ok=True)
    for name, content in build_samples().items():
        path = FIXTURES / name
        if path.exists() or path.is_symlink():
            if path.is_symlink() or path.read_bytes() != content:
                raise FileExistsError(f"기존 샘플을 덮어쓰지 않음: {name}")
        else:
            with path.open("xb") as output:
                output.write(content)
        print(f"{name}: {len(content)} bytes")


if __name__ == "__main__":
    main()

"""기존 폴더 16개와 1 MiB 파일의 분할 7z 회귀 표본을 생성함."""

from pathlib import Path
import sys

from seven_zip_fixtures import archive


def main():
    root = Path(sys.argv[1])
    entries = [(f"d{index:03}", b"", 0x10) for index in range(16)]
    if sys.argv[2] == "file":
        entries.append(("payload.bin", bytes(range(256)) * 4096, 0x20))
    else:
        assert sys.argv[2] == "directories"
    data = archive(entries, method="copy")
    width = 256 * 1024
    for index, begin in enumerate(range(0, len(data), width), start=1):
        (root / f"sample.7z.{index:03}").write_bytes(data[begin:begin + width])


if __name__ == "__main__":
    main()

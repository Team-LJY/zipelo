"""시스템 임시 폴더보다 큰 독립 ZIP과 7z 입력을 만듦."""
from pathlib import Path
import sys
import zipfile

from seven_zip_fixtures import archive

root = Path(sys.argv[1])
payload = b"spool fallback preview\n"
filler = bytes(range(256)) * (16 * 1024 * 1024 // 256)
with zipfile.ZipFile(
    root / "archive" / "input.zip", "w", compression=zipfile.ZIP_STORED
) as target:
    target.writestr("preview.txt", payload)
    target.writestr("filler.bin", filler)
(root / "archive" / "input.7z").write_bytes(
    archive([("preview.txt", payload, 0x20), ("filler.bin", filler, 0x20)], method="copy")
)

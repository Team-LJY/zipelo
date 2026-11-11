import gzip
from io import BytesIO
from pathlib import Path
import tarfile
import zipfile

ROOT = Path(__file__).resolve().parent / "fixtures"
PAYLOAD = b"Zipelo real worker flow\n"
ROOT.mkdir(exist_ok=True)
stream = BytesIO()
with zipfile.ZipFile(stream, "w") as archive:
    info = zipfile.ZipInfo("content.txt", (1980, 1, 1, 0, 0, 0))
    info.compress_type = zipfile.ZIP_DEFLATED
    archive.writestr(info, PAYLOAD)
(ROOT / "sample.zip").write_bytes(stream.getvalue())
stream = BytesIO()
with tarfile.open(fileobj=stream, mode="w", format=tarfile.USTAR_FORMAT) as archive:
    info = tarfile.TarInfo("content.txt")
    info.size = len(PAYLOAD)
    info.mode = 0o600
    archive.addfile(info, BytesIO(PAYLOAD))
compressed = bytearray(gzip.compress(stream.getvalue(), mtime=0))
compressed[9] = 255
(ROOT / "sample.tar.gz").write_bytes(compressed)

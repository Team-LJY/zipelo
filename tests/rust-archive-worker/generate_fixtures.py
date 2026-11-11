import gzip
from io import BytesIO
from pathlib import Path
import tarfile
import zipfile

root = Path(__file__).resolve().parent / "fixtures"
root.mkdir(exist_ok=True)
payload = b"Zipelo worker interoperability\n"
for method, label in ((zipfile.ZIP_STORED, "store"), (zipfile.ZIP_DEFLATED, "deflate")):
    with zipfile.ZipFile(root / f"{label}.zip", "w", compression=method) as archive:
        archive.writestr("folder/hello.txt", payload)
        archive.writestr("empty/", b"")
for name in ("../escape.txt", "/absolute.txt", "C:/drive.txt", "CON.txt"):
    label = {"../escape.txt": "traversal", "/absolute.txt": "absolute", "C:/drive.txt": "drive", "CON.txt": "reserved"}[name]
    with zipfile.ZipFile(root / f"{label}.zip", "w") as archive:
        archive.writestr(name, payload)
with zipfile.ZipFile(root / "link.zip", "w") as archive:
    entry = zipfile.ZipInfo("link")
    entry.create_system = 3
    entry.external_attr = 0o120777 << 16
    archive.writestr(entry, "../outside")
with zipfile.ZipFile(root / "bomb.zip", "w", compression=zipfile.ZIP_DEFLATED) as archive:
    archive.writestr("bomb.txt", b"0" * 1048576)
with zipfile.ZipFile(root / "collision.zip", "w") as archive:
    archive.writestr("A.txt", b"first")
    archive.writestr("a.txt", b"second")
with zipfile.ZipFile(root / "parent-collision.zip", "w") as archive:
    archive.writestr("a", b"file")
    archive.writestr("a/b.txt", b"nested")
stream = BytesIO()
with tarfile.open(fileobj=stream, mode="w", format=tarfile.USTAR_FORMAT) as archive:
    entry = tarfile.TarInfo("folder/hello.txt")
    entry.size = len(payload)
    archive.addfile(entry, BytesIO(payload))
tar = stream.getvalue()
(root / "sample.tar").write_bytes(tar)
(root / "sample.tar.gz").write_bytes(gzip.compress(tar, mtime=0))
stream = BytesIO()
with gzip.GzipFile(filename="../../untrusted-name", fileobj=stream, mode="wb", mtime=0) as archive:
    archive.write(payload)
(root / "sample.gz").write_bytes(stream.getvalue())
(root / "concat.gz").write_bytes(gzip.compress(b"one", mtime=0) + gzip.compress(b"two", mtime=0))

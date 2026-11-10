import gzip
import hashlib
import io
import json
import tarfile
import zlib


def gzip_bytes(payload):
    archive = io.BytesIO()
    with gzip.GzipFile(filename="", mode="wb", compresslevel=6, fileobj=archive, mtime=0) as writer:
        writer.write(payload)
    return archive.getvalue()


def build():
    payload = b"123456789"
    archive = io.BytesIO()
    with tarfile.open(fileobj=archive, mode="w", format=tarfile.USTAR_FORMAT) as writer:
        info = tarfile.TarInfo("digits.txt")
        info.size = len(payload)
        info.mode = 0o644
        info.mtime = 0
        writer.addfile(info, io.BytesIO(payload))
    tar_bytes = archive.getvalue()[:2048]
    values = {
        "digits": payload,
        "gzip_digits": gzip_bytes(payload),
        "gzip_empty": gzip_bytes(b""),
        "ustar_one": tar_bytes,
        "ustar_sha256": hashlib.sha256(tar_bytes).digest(),
    }
    if gzip.decompress(values["gzip_digits"]) != payload:
        raise RuntimeError("GZip fixture 검증 실패")
    with tarfile.open(fileobj=io.BytesIO(tar_bytes), mode="r:") as reader:
        if reader.getnames() != ["digits.txt"] or reader.extractfile("digits.txt").read() != payload:
            raise RuntimeError("USTAR fixture 검증 실패")
    metadata = {
        "provenance": "Python 표준 gzip/zlib와 tarfile USTAR_FORMAT으로 만든 합성 bytes임",
        "gzip": {"mtime": 0, "level": 6, "crc32": zlib.crc32(payload)},
        "ustar": {"trimmed_length": 2048, "terminator_blocks": 2, "mtime": 0, "mode": "0644"},
        "files": {name: {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
                  for name, data in sorted(values.items())},
        "limitations": "ZIP 결과는 native 호출과 비교함. 독립 ZIP parser나 codec 검증이 아님",
    }
    return (
        "".join(name + " " + data.hex() + "\n" for name, data in sorted(values.items())),
        json.dumps(metadata, ensure_ascii=False, indent=4) + "\n",
    )

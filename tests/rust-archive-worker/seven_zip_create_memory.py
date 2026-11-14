"""1 GiB 입력의 7z 작성 메모리와 게시 결과를 검증함."""

import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import time


def native(path):
    value = os.fsencode(path)
    return b"\x01" + struct.pack("<I", len(value)) + value


with tempfile.TemporaryDirectory(prefix="7zc-memory-", dir=os.environ["TMPDIR"]) as temporary:
    root = Path(temporary)
    source = root / "source"
    with source.open("xb") as output:
        output.truncate(1024 * 1024 * 1024)
    archive = root / "result.7z"
    body = b"\x04" + struct.pack("<I", 1) + native(source)
    body += struct.pack("<I", 11) + b"payload.bin" + native(archive)
    body += b"\x00\x00\x00" + struct.pack("<QQI", 2**64 - 1, 16 * 1024**3, 10000)
    frame = b"ZAW1" + struct.pack("<HHIIQQ", 1, 1, len(body), 0, 17, 0) + body
    started = time.monotonic()
    result = subprocess.run(
        ["/usr/bin/time", "-f", "%M", "-o", str(root / "rss"), sys.argv[1]],
        input=frame, capture_output=True, timeout=180,
        env={**os.environ, "TMPDIR": str(root), "TMP": str(root), "TEMP": str(root)},
    )
    assert result.returncode == 0, result.stderr
    assert not result.stderr, result.stderr
    at = 0
    terminal = None
    outputs = 0
    while at < len(result.stdout):
        magic, version, kind, size, reserved, job, sequence = struct.unpack_from("<4sHHIIQQ", result.stdout, at)
        assert (magic, version, reserved, job) == (b"ZAW1", 1, 0, 17)
        at += 32
        payload = result.stdout[at:at + size]
        assert len(payload) == size
        at += size
        if kind == 105:
            outputs += 1
        if kind == 106:
            assert at == len(result.stdout)
            terminal = payload
    assert terminal[:3] == b"\x00\x00\x02", terminal
    assert struct.unpack_from("<QQ", terminal, 3) == (1, 0)
    assert outputs == 1 and archive.is_file()
    rss = int((root / "rss").read_text())
    assert rss <= 64 * 1024, rss
    print(json.dumps({
        "seven_zip_creation_input_bytes": source.stat().st_size,
        "archive_bytes": archive.stat().st_size,
        "peak_rss_kib": rss,
        "elapsed_seconds": round(time.monotonic() - started, 3),
    }))

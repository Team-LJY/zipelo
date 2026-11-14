import argparse
import hashlib
import io
import json
from pathlib import Path
import struct
import zipfile


ROOT = Path(__file__).resolve().parent
SOURCES = [
    ("hello.txt", b"Zipelo recovery reference\n", zipfile.ZIP_STORED),
    ("한글/자료.txt", "복구할 한글 데이터\n".encode("utf-8") * 16, zipfile.ZIP_DEFLATED),
    ("empty.txt", b"", zipfile.ZIP_STORED),
    ("empty-dir/", b"", zipfile.ZIP_STORED),
    ("middle.bin", bytes(range(256)) * 4, zipfile.ZIP_STORED),
    ("after.txt", b"Intact entry after the damaged payload\n", zipfile.ZIP_DEFLATED),
]


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def corpus():
    stream = io.BytesIO()
    with zipfile.ZipFile(stream, "w") as archive:
        for name, data, method in SOURCES:
            info = zipfile.ZipInfo(name, date_time=(2020, 1, 2, 3, 4, 6))
            info.create_system = 3
            info.external_attr = (0o40755 if name.endswith("/") else 0o100644) << 16
            if name.endswith("/"):
                info.external_attr |= 0x10
            info.compress_type = method
            archive.writestr(info, data, compresslevel=6)
    original = stream.getvalue()
    with zipfile.ZipFile(io.BytesIO(original)) as archive:
        directory_offset = archive.start_dir
        entries = []
        for info, (_, data, _) in zip(archive.infolist(), SOURCES):
            name_size, extra_size = struct.unpack_from("<HH", original, info.header_offset + 26)
            data_offset = info.header_offset + 30 + name_size + extra_size
            entries.append({
                "name": info.filename,
                "kind": "directory" if info.is_dir() else "file",
                "bytes": len(data),
                "sha256": sha256(data),
                "crc32": f"{info.CRC:08x}",
                "method": info.compress_type,
                "local_offset": info.header_offset,
                "data_offset": data_offset,
                "compressed_bytes": info.compress_size,
            })
    middle = entries[4]
    cut = middle["data_offset"] + middle["compressed_bytes"] // 2
    corrupt_offset = middle["data_offset"] + 17
    corrupt = bytearray(original[:directory_offset])
    corrupt[corrupt_offset] ^= 0x80
    bad_eocd = bytearray(original)
    bad_eocd[-22:-18] = b"BAD!"
    local_offset = entries[1]["local_offset"]
    bad_local = bytearray(original)
    bad_local[local_offset:local_offset + 4] = b"BAD!"
    cases = [
        ("control.zip", "control", original, [], ["verified"] * 6),
        ("missing-directory.zip", "header_damage", original[:directory_offset],
         [{"operation": "truncate", "offset": directory_offset}], ["verified"] * 6),
        ("broken-eocd.zip", "header_damage", bytes(bad_eocd),
         [{"operation": "replace", "offset": len(original) - 22, "hex": "42414421"}],
         ["verified"] * 6),
        ("middle-truncated.zip", "middle_truncation", original[:cut],
         [{"operation": "truncate", "offset": cut}],
         ["verified"] * 4 + ["truncated", "not_observable"]),
        ("body-corrupt.zip", "payload_corruption", bytes(corrupt),
         [{"operation": "truncate", "offset": directory_offset},
          {"operation": "xor", "offset": corrupt_offset, "value": 128}],
         ["verified"] * 4 + ["integrity_failed", "verified"]),
        ("unrecoverable.zip", "unrecoverable", original[:20],
         [{"operation": "truncate", "offset": 20}], ["not_observable"] * 6),
        ("broken-local-header.zip", "header_damage", bytes(bad_local),
         [{"operation": "replace", "offset": local_offset, "hex": "42414421"}],
         ["verified"] * 6),
    ]
    files = {}
    samples = []
    for name, category, data, mutations, states in cases:
        verified = [entry["name"] for entry, state in zip(entries, states) if state == "verified"]
        samples.append({
            "file": name,
            "category": category,
            "bytes": len(data),
            "sha256": sha256(data),
            "mutations_from_control": mutations,
            "expected": {
                "publish_archive": bool(verified) and category != "control",
                "warning_required": category != "control" and name != "broken-local-header.zip",
                "must_not_claim_complete_original": category != "control" and name != "broken-local-header.zip",
                "must_preserve_source_sha256": sha256(data),
                "verified_entries": verified,
                "excluded_entries": [entry["name"] for entry, state in zip(entries, states)
                                     if state != "verified"],
                "oracle_states": dict(zip((entry["name"] for entry in entries), states)),
            },
        })
        if name == "broken-local-header.zip":
            samples[-1]["expected"]["recovery_summary"] = {
                "mode": "CentralDirectory",
                "known_entries": 6,
                "verified_entries": 6,
                "lost_entries": 0,
                "unverified_entries": 0,
                "unknown_loss": False,
            }
            samples[-1]["expected"]["terminal_after_successful_publication"] = {
                "outcome": "Completed",
                "error": "None",
                "publication": "Complete",
                "created_count": 1,
            }
        files[name] = data
    manifest = {
        "schema_version": 1,
        "worker_execution_verified": False,
        "description": "자체 생성 표본임. oracle_states는 원본을 아는 시험의 기대값이며 제품 보고 계약이 아님",
        "source_entries": entries,
        "samples": samples,
    }
    files["expectations.json"] = (json.dumps(manifest, ensure_ascii=False, indent=4) + "\n").encode()
    return files


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    files = corpus()
    if args.check:
        for name, data in files.items():
            if (ROOT / name).read_bytes() != data:
                raise SystemExit(f"재생성 불일치: {name}")
        print(f"재생성 일치: {len(files) - 1}개 ZIP과 기대값")
    else:
        for name, data in files.items():
            (ROOT / name).write_bytes(data)


if __name__ == "__main__":
    main()

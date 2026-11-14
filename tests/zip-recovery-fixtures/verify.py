import argparse
import hashlib
import io
import json
from pathlib import Path
import struct
import unittest
import zipfile
import zlib

from generate import ROOT, SOURCES, corpus


MANIFEST = json.loads((ROOT / "expectations.json").read_text(encoding="utf-8"))
ENTRIES = {entry["name"]: entry for entry in MANIFEST["source_entries"]}
SAMPLES = {sample["file"]: sample for sample in MANIFEST["samples"]}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def reference_restore_local_signature(data):
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        offset = archive.getinfo("한글/자료.txt").header_offset
    if data[offset:offset + 4] != b"BAD!":
        raise ValueError("표본의 로컬 서명 손상 위치 불일치")
    return data[:offset] + b"PK\x03\x04" + data[offset + 4:]


def local_payload(data, entry):
    offset = entry["local_offset"]
    if data[offset:offset + 4] != b"PK\x03\x04" or len(data) < offset + 30:
        raise ValueError("완전한 로컬 헤더 없음")
    header = struct.unpack_from("<4s5H3I2H", data, offset)
    method, crc, packed, size, name_size, extra_size = (
        header[3], header[6], header[7], header[8], header[9], header[10]
    )
    start = offset + 30 + name_size + extra_size
    end = start + packed
    if end > len(data):
        raise ValueError("본문 절단")
    payload = data[start:end]
    if method == zipfile.ZIP_DEFLATED:
        decoder = zlib.decompressobj(-15)
        payload = decoder.decompress(payload) + decoder.flush()
        if not decoder.eof or decoder.unused_data or decoder.unconsumed_tail:
            raise ValueError("DEFLATE 경계 불일치")
    elif method != zipfile.ZIP_STORED:
        raise ValueError("표본에 없는 압축 방법")
    if len(payload) != size or zlib.crc32(payload) != crc:
        raise ValueError("본문 크기 또는 CRC 불일치")
    return payload


def verify_recovered(sample_name, recovered):
    sample = SAMPLES[sample_name]
    if not sample["expected"]["publish_archive"]:
        raise ValueError("이 표본은 복구본을 게시하면 안 됨")
    source = (ROOT / sample_name).read_bytes()
    if digest(source) != sample["sha256"]:
        raise ValueError("손상 원본이 변경됨")
    with zipfile.ZipFile(recovered) as archive:
        names = archive.namelist()
        expected = sample["expected"]["verified_entries"]
        if len(names) != len(set(names)) or set(names) != set(expected):
            raise ValueError("복구 항목 집합 불일치")
        for info in archive.infolist():
            reference = ENTRIES[info.filename]
            if info.file_size != reference["bytes"]:
                raise ValueError("복구 내용 크기 불일치")
            with archive.open(info) as member:
                payload = member.read(reference["bytes"] + 1)
            if info.is_dir() != (reference["kind"] == "directory"):
                raise ValueError("파일과 폴더 종류 불일치")
            if len(payload) != reference["bytes"] or digest(payload) != reference["sha256"]:
                raise ValueError("복구 내용 해시 불일치")


class CorpusTests(unittest.TestCase):
    def test_fixture_hashes_and_categories(self):
        self.assertEqual({sample["category"] for sample in SAMPLES.values()}, {
            "control", "header_damage", "middle_truncation", "payload_corruption", "unrecoverable"
        })
        for name, sample in SAMPLES.items():
            with self.subTest(name=name):
                data = (ROOT / name).read_bytes()
                self.assertEqual(len(data), sample["bytes"])
                self.assertEqual(digest(data), sample["sha256"])
                self.assertEqual(digest(data), sample["expected"]["must_preserve_source_sha256"])

    def test_regeneration_matches_committed_bytes(self):
        for name, data in corpus().items():
            self.assertEqual((ROOT / name).read_bytes(), data, name)

    def test_control_with_independent_zipfile_reader(self):
        with zipfile.ZipFile(ROOT / "control.zip") as archive:
            self.assertIsNone(archive.testzip())
            self.assertEqual(archive.namelist(), list(ENTRIES))
            for name, expected, _ in SOURCES:
                self.assertEqual(archive.read(name), expected)
                self.assertEqual(digest(expected), ENTRIES[name]["sha256"])

    def test_exact_damage_operations(self):
        original = (ROOT / "control.zip").read_bytes()
        for name, sample in SAMPLES.items():
            damaged = bytearray(original)
            for operation in sample["mutations_from_control"]:
                offset = operation["offset"]
                if operation["operation"] == "truncate":
                    del damaged[offset:]
                elif operation["operation"] == "replace":
                    replacement = bytes.fromhex(operation["hex"])
                    damaged[offset:offset + len(replacement)] = replacement
                elif operation["operation"] == "xor":
                    damaged[offset] ^= operation["value"]
                else:
                    self.fail("알 수 없는 손상 연산")
            self.assertEqual(bytes(damaged), (ROOT / name).read_bytes(), name)

    def test_damage_requires_recovery(self):
        for name, sample in SAMPLES.items():
            if sample["category"] == "control":
                continue
            with self.subTest(name=name), self.assertRaises(zipfile.BadZipFile):
                with zipfile.ZipFile(ROOT / name) as archive:
                    for info in archive.infolist():
                        archive.read(info)

    def test_complete_inventory_with_only_local_signature_damage(self):
        original = (ROOT / "control.zip").read_bytes()
        damaged = (ROOT / "broken-local-header.zip").read_bytes()
        with zipfile.ZipFile(io.BytesIO(damaged)) as archive:
            self.assertEqual(archive.namelist(), list(ENTRIES))
            self.assertEqual(damaged[archive.start_dir:], original[archive.start_dir:])
            offset = 0
            for info in archive.infolist():
                self.assertEqual(info.header_offset, offset)
                name_size, extra_size = struct.unpack_from("<HH", damaged, offset + 26)
                offset += 30 + name_size + extra_size + info.compress_size
            self.assertEqual(offset, archive.start_dir)
            with self.assertRaises(zipfile.BadZipFile):
                archive.read("한글/자료.txt")
        repaired = reference_restore_local_signature(damaged)
        self.assertEqual(repaired, original)
        with zipfile.ZipFile(io.BytesIO(repaired)) as archive:
            self.assertIsNone(archive.testzip())
            for info in archive.infolist():
                self.assertEqual(digest(archive.read(info)), ENTRIES[info.filename]["sha256"])
        self.assertEqual((ROOT / "broken-local-header.zip").read_bytes(), damaged)
        expected = SAMPLES["broken-local-header.zip"]["expected"]
        self.assertEqual(expected["recovery_summary"], {
            "mode": "CentralDirectory", "known_entries": 6, "verified_entries": 6,
            "lost_entries": 0, "unverified_entries": 0, "unknown_loss": False,
        })
        self.assertFalse(expected["must_not_claim_complete_original"])
        self.assertFalse(expected["warning_required"])
        self.assertEqual(expected["terminal_after_successful_publication"], {
            "outcome": "Completed", "error": "None", "publication": "Complete", "created_count": 1,
        })

    def test_verified_entries_have_intact_local_payloads(self):
        for name, sample in SAMPLES.items():
            data = (ROOT / name).read_bytes()
            if name == "broken-local-header.zip":
                data = reference_restore_local_signature(data)
            for entry_name in sample["expected"]["verified_entries"]:
                with self.subTest(sample=name, entry=entry_name):
                    payload = local_payload(data, ENTRIES[entry_name])
                    self.assertEqual(digest(payload), ENTRIES[entry_name]["sha256"])

    def test_excluded_entries_are_not_verified(self):
        for name, sample in SAMPLES.items():
            data = (ROOT / name).read_bytes()
            for entry_name in sample["expected"]["excluded_entries"]:
                with self.subTest(sample=name, entry=entry_name), self.assertRaises(ValueError):
                    local_payload(data, ENTRIES[entry_name])
        counts = [len(sample["expected"]["verified_entries"]) for sample in SAMPLES.values()]
        self.assertEqual(counts, [6, 6, 6, 4, 5, 0, 6])
        self.assertEqual(SAMPLES["middle-truncated.zip"]["expected"]["oracle_states"]["after.txt"],
                         "not_observable")

    def test_reference_recovered_output_checker(self):
        for name, sample in SAMPLES.items():
            if not sample["expected"]["publish_archive"]:
                continue
            stream = io.BytesIO()
            with zipfile.ZipFile(stream, "w") as archive:
                for entry_name, payload, _ in SOURCES:
                    if entry_name in sample["expected"]["verified_entries"]:
                        archive.writestr(entry_name, payload)
            stream.seek(0)
            verify_recovered(name, stream)

    def test_checker_rejects_missing_wrong_and_extra_entries(self):
        for mutation in ("missing", "wrong", "extra"):
            stream = io.BytesIO()
            with zipfile.ZipFile(stream, "w") as archive:
                for index, (name, payload, _) in enumerate(SOURCES):
                    if mutation == "missing" and index == 0:
                        continue
                    archive.writestr(name, b"wrong" if mutation == "wrong" and index == 0 else payload)
                if mutation == "extra":
                    archive.writestr("unexpected.txt", b"extra")
            stream.seek(0)
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                verify_recovered("missing-directory.zip", stream)
        with self.assertRaises(ValueError):
            verify_recovered("unrecoverable.zip", io.BytesIO())


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--recovered", nargs=2, metavar=("SAMPLE", "OUTPUT_ZIP"))
    args = parser.parse_args()
    if args.recovered:
        verify_recovered(args.recovered[0], Path(args.recovered[1]))
        print("복구본 항목과 해시 일치")
    else:
        unittest.main(argv=[__file__], verbosity=2)

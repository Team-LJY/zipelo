import hashlib
import importlib.util
from io import BytesIO
import json
from pathlib import Path
import stat
import struct
import tarfile
import unittest
import zipfile
import zlib


CORPUS = Path(__file__).resolve().parents[1] / "malicious-archive-fixtures"
FIXTURES = CORPUS / "fixtures"
MAX_EXPANSION = 131072


def bounded_gzip(data):
    decoder = zlib.decompressobj(31)
    output = decoder.decompress(data, MAX_EXPANSION + 1)
    if len(output) > MAX_EXPANSION or not decoder.eof or decoder.unused_data:
        raise ValueError("표본의 해제 크기 또는 스트림 경계가 유효하지 않음")
    return output


def tar_members(data):
    with tarfile.open(fileobj=BytesIO(data), mode="r:") as archive:
        return archive.getmembers()


class MaliciousFixtureTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.manifest = json.loads((CORPUS / "expectations.json").read_text(encoding="utf-8"))
        cls.cases = cls.manifest["cases"]

    def test_inventory_and_integrity(self):
        names = [case["file"] for case in self.cases]
        self.assertEqual(len(names), 15)
        self.assertEqual(len(names), len(set(names)))
        self.assertEqual(set(names), {path.name for path in FIXTURES.iterdir()})
        self.assertEqual(
            {case["category"] for case in self.cases},
            {"unsafe-path", "symlink-escape", "output-limit", "damaged-header"},
        )
        for case in self.cases:
            with self.subTest(file=case["file"]):
                data = (FIXTURES / case["file"]).read_bytes()
                self.assertLessEqual(len(data), MAX_EXPANSION)
                self.assertEqual(len(data), case["bytes"])
                self.assertEqual(hashlib.sha256(data).hexdigest(), case["sha256"])

    def test_regeneration_matches_committed_bytes(self):
        spec = importlib.util.spec_from_file_location("malicious_fixture_generator", CORPUS / "generate.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        files, manifest = module.build()
        self.assertEqual(manifest, self.manifest)
        for name, data in files.items():
            self.assertEqual(data, (FIXTURES / name).read_bytes())

    def test_expected_rejection_without_writes(self):
        expected = self.manifest["expected"]
        self.assertEqual(expected["outcome"], "Failed")
        self.assertEqual(expected["publication"], "None")
        for key in (
            "created_count", "retained_partial_count", "output_event_count",
            "destination_mutation_count", "outside_mutation_count",
        ):
            self.assertEqual(expected[key], 0)
        self.assertFalse(self.manifest["worker_execution_verified"])
        for case in self.cases:
            self.assertTrue(case["error_any_of"])

    def test_hostile_paths_follow_valid_member(self):
        for case in self.cases:
            if case["category"] != "unsafe-path":
                continue
            with self.subTest(file=case["file"]):
                data = (FIXTURES / case["file"]).read_bytes()
                if case["format"] == "ZIP":
                    with zipfile.ZipFile(BytesIO(data)) as archive:
                        names = archive.namelist()
                        self.assertIsNone(archive.testzip())
                else:
                    if case["format"] == "TAR_GZIP":
                        data = bounded_gzip(data)
                    names = [member.name for member in tar_members(data)]
                self.assertEqual(names, ["before.txt", case["hostile_name"]])
                name = names[-1].replace("\\", "/")
                self.assertTrue(name.startswith(("../", "/", "C:/")))

    def test_links_target_sibling_before_descendant_file(self):
        for case in self.cases:
            if case["category"] != "symlink-escape":
                continue
            with self.subTest(file=case["file"]):
                data = (FIXTURES / case["file"]).read_bytes()
                if case["format"] == "ZIP":
                    with zipfile.ZipFile(BytesIO(data)) as archive:
                        members = archive.infolist()
                        self.assertIsNone(archive.testzip())
                        self.assertEqual(members[1].create_system, 3)
                        self.assertTrue(stat.S_ISLNK(members[1].external_attr >> 16))
                        target = archive.read(members[1]).decode("ascii")
                        names = [member.filename for member in members]
                else:
                    members = tar_members(data)
                    self.assertTrue(members[1].issym())
                    target = members[1].linkname
                    names = [member.name for member in members]
                self.assertEqual(target, "../outside")
                self.assertEqual(names, ["before.txt", "link", "link/escape.txt"])

    def test_bombs_are_valid_bounded_streams_exceeding_request_limit(self):
        for case in self.cases:
            if case["category"] != "output-limit":
                continue
            with self.subTest(file=case["file"]):
                data = (FIXTURES / case["file"]).read_bytes()
                if case["format"] == "ZIP":
                    with zipfile.ZipFile(BytesIO(data)) as archive:
                        expanded = 0
                        for member in archive.infolist():
                            with archive.open(member) as stream:
                                payload = stream.read(MAX_EXPANSION + 1)
                                self.assertLessEqual(len(payload), MAX_EXPANSION)
                                self.assertEqual(len(payload), member.file_size)
                                expanded += len(payload)
                        self.assertEqual(archive.namelist(), ["before.txt", "large.txt"])
                else:
                    output = bounded_gzip(data)
                    expanded = len(output)
                    if case["format"] == "TAR_GZIP":
                        self.assertEqual([m.name for m in tar_members(output)],
                                         ["before.txt", "large.txt"])
                self.assertEqual(expanded, case["expanded_bytes"])
                self.assertGreater(expanded, case["limits"]["output"])
                self.assertLessEqual(expanded, MAX_EXPANSION)
                self.assertGreater(expanded / len(data), 100)

    def test_damaged_zip_retains_central_directory_but_rejects_local_header(self):
        data = (FIXTURES / "zip-bad-local-header.zip").read_bytes()
        with zipfile.ZipFile(BytesIO(data)) as archive:
            self.assertEqual(archive.namelist(), ["before.txt", "broken.txt"])
            self.assertEqual(archive.read("before.txt"), b"must not be written\n")
            offset = archive.infolist()[1].header_offset
            self.assertEqual(struct.unpack_from("<H", data, offset + 26)[0], 65535)
            with self.assertRaises(zipfile.BadZipFile):
                archive.read("broken.txt")

    def test_damaged_tar_has_bad_second_header_checksum(self):
        data = (FIXTURES / "tar-bad-checksum.tar").read_bytes()
        self.assertEqual(tarfile.TarInfo.frombuf(data[:512], "ascii", "strict").name, "before.txt")
        with self.assertRaises(tarfile.InvalidHeaderError):
            tarfile.TarInfo.frombuf(data[1024:1536], "ascii", "strict")

    def test_gzip_reserved_flag_is_rejected_and_control_is_valid(self):
        data = (FIXTURES / "gzip-reserved-flags.gz").read_bytes()
        self.assertEqual(data[3] & 0xE0, 0x20)
        with self.assertRaises(zlib.error):
            bounded_gzip(data)
        control = bytearray(data)
        control[3] &= ~0x20
        self.assertEqual(bounded_gzip(control), b"header\n")


if __name__ == "__main__":
    unittest.main()

import hashlib
import importlib.util
import io
import json
from pathlib import Path
import stat
import struct
import unittest
import zipfile
import zlib


DIRECTORY = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("zipelo_extraction_factory", DIRECTORY / "fixture_factory.py")
FACTORY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FACTORY)


class ExtractionFixtureTests(unittest.TestCase):
    def test_samples_and_manifest_are_reproducible(self):
        samples = FACTORY.build_samples()
        self.assertEqual(22, len(samples))
        self.assertEqual(set(samples), {path.name for path in (DIRECTORY / "fixtures").iterdir()})
        for name, data in samples.items():
            with self.subTest(name=name):
                path = DIRECTORY / "fixtures" / name
                self.assertFalse(path.is_symlink())
                self.assertEqual(data, path.read_bytes())
                self.assertLessEqual(len(data), FACTORY.MAX_SAMPLE_BYTES)
        self.assertEqual(FACTORY.build_manifest(samples), json.loads((DIRECTORY / "fixtures.json").read_text()))

    def test_normal_contents_and_hashes_for_both_methods(self):
        samples = FACTORY.build_samples()
        manifest = FACTORY.build_manifest(samples)
        for name in ("normal-stored.zip", "normal-deflated.zip"):
            with self.subTest(name=name), zipfile.ZipFile(io.BytesIO(samples[name])) as zipped:
                self.assertEqual(4, len(zipped.infolist()))
                self.assertTrue(zipped.getinfo("nested/").is_dir())
                for entry, expected in manifest["normal_files"].items():
                    data = zipped.read(entry)
                    self.assertEqual(expected["bytes"], len(data))
                    self.assertEqual(expected["sha256"], hashlib.sha256(data).hexdigest())

    def test_unsafe_names_are_metadata_only(self):
        samples = FACTORY.build_samples()
        expected = {
            "traversal.zip": "../escape.txt",
            "backslash-traversal.zip": "..\\escape.txt",
            "absolute-posix.zip": "/escape.txt",
            "absolute-drive.zip": "C:/escape.txt",
            "absolute-unc.zip": "//server/share/escape.txt",
        }
        for name, entry in expected.items():
            with self.subTest(name=name), zipfile.ZipFile(io.BytesIO(samples[name])) as zipped:
                self.assertEqual([entry], zipped.namelist())

    def test_conflicts_and_os_names_remain_distinct(self):
        samples = FACTORY.build_samples()
        expected = {
            "duplicate.zip": ["same.txt", "same.txt"],
            "file-before-child.zip": ["node", "node/child"],
            "child-before-file.zip": ["node/child", "node"],
            "case-distinct.zip": ["Readme.txt", "README.TXT"],
            "linux-names.zip": ["CON", "name:stream"],
        }
        for name, entries in expected.items():
            with self.subTest(name=name), zipfile.ZipFile(io.BytesIO(samples[name])) as zipped:
                self.assertEqual(entries, zipped.namelist())
                self.assertIsNone(zipped.testzip())

    def test_link_metadata_without_live_filesystem_links(self):
        samples = FACTORY.build_samples()
        with zipfile.ZipFile(io.BytesIO(samples["symlink.zip"])) as zipped:
            self.assertTrue(stat.S_ISLNK(zipped.infolist()[0].external_attr >> 16))
        with zipfile.ZipFile(io.BytesIO(samples["special-fifo.zip"])) as zipped:
            self.assertTrue(stat.S_ISFIFO(zipped.infolist()[0].external_attr >> 16))
        with zipfile.ZipFile(io.BytesIO(samples["unix-hardlink.zip"])) as zipped:
            item = zipped.infolist()[0]
            self.assertTrue(stat.S_ISREG(item.external_attr >> 16))
            tag, size = struct.unpack_from("<HH", item.extra)
            self.assertEqual(0x000D, tag)
            self.assertEqual(12 + len(b"../sentinel.txt"), size)
            self.assertEqual(b"../sentinel.txt", item.extra[16:])
            self.assertEqual(0, item.file_size)

    def test_crc_damage_is_independent_of_lengths(self):
        samples = FACTORY.build_samples()
        for name in ("bad-payload-crc.zip", "bad-header-crc.zip"):
            with self.subTest(name=name), zipfile.ZipFile(io.BytesIO(samples[name])) as zipped:
                self.assertEqual(len(FACTORY.PAYLOAD), zipped.infolist()[0].file_size)
                self.assertEqual("payload.txt", zipped.testzip())

    def test_length_damage_preserves_decoded_payload_and_crc(self):
        samples = FACTORY.build_samples()
        for name, declared in (("length-smaller.zip", len(FACTORY.PAYLOAD) - 1),
                               ("length-larger.zip", len(FACTORY.PAYLOAD) + 1)):
            data = samples[name]
            central = data.index(b"PK\x01\x02")
            with self.subTest(name=name):
                self.assertEqual(declared, struct.unpack_from("<I", data, 22)[0])
                self.assertEqual(declared, struct.unpack_from("<I", data, central + 24)[0])
                compressed = struct.unpack_from("<I", data, 18)[0]
                start = 30 + sum(struct.unpack_from("<HH", data, 26))
                decoded = zlib.decompress(data[start:start + compressed], -15)
                self.assertEqual(FACTORY.PAYLOAD, decoded)
                self.assertNotEqual(declared, len(decoded))
                self.assertEqual(zlib.crc32(decoded), struct.unpack_from("<I", data, 14)[0])
                self.assertEqual(zlib.crc32(decoded), struct.unpack_from("<I", data, central + 16)[0])

    def test_header_disagreement_and_truncation_are_separate(self):
        samples = FACTORY.build_samples()
        data = samples["local-central-length.zip"]
        central = data.index(b"PK\x01\x02")
        self.assertNotEqual(struct.unpack_from("<I", data, 22)[0], struct.unpack_from("<I", data, central + 24)[0])
        with self.assertRaises(zipfile.BadZipFile):
            zipfile.ZipFile(io.BytesIO(samples["truncated.zip"]))

    def test_cumulative_sample_needs_aggregate_budget(self):
        with zipfile.ZipFile(io.BytesIO(FACTORY.build_samples()["cumulative.zip"])) as zipped:
            self.assertEqual([8, 8], [item.file_size for item in zipped.infolist()])
            self.assertEqual(16, sum(len(zipped.read(item)) for item in zipped.infolist()))


if __name__ == "__main__":
    unittest.main()

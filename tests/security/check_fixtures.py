import importlib.util
import io
from pathlib import Path
import stat
import unittest
import zipfile


DIRECTORY = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("zipelo_fixture_factory", DIRECTORY / "fixture_factory.py")
FACTORY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FACTORY)


class SafetyFixtureTests(unittest.TestCase):
    def test_samples_match_reproducible_generator(self):
        for name, expected in FACTORY.build_samples().items():
            with self.subTest(name=name):
                path = DIRECTORY / "fixtures" / name
                self.assertFalse(path.is_symlink())
                self.assertEqual(path.read_bytes(), expected)
                self.assertLessEqual(path.stat().st_size, FACTORY.MAX_SAMPLE_BYTES)

    def test_valid_samples_have_bounded_content_without_extraction(self):
        for name, data in FACTORY.build_samples().items():
            if name in ("bad-crc.zip", "truncated.zip"):
                continue
            with self.subTest(name=name), zipfile.ZipFile(io.BytesIO(data)) as zipped:
                self.assertLessEqual(len(zipped.infolist()), 2)
                self.assertLessEqual(sum(item.file_size for item in zipped.infolist()), 128)
                self.assertIsNone(zipped.testzip())

    def test_normal_sample_preserves_unicode_and_empty_file(self):
        with zipfile.ZipFile(io.BytesIO(FACTORY.build_samples()["normal.zip"])) as zipped:
            self.assertEqual(zipped.read("hello.txt"), FACTORY.PAYLOAD)
            self.assertEqual(zipped.read("한글/빈파일.txt"), b"")

    def test_traversal_samples_retain_unsafe_names_as_metadata(self):
        samples = FACTORY.build_samples()
        for name, entry in (
            ("traversal.zip", "../escape.txt"),
            ("backslash-traversal.zip", "..\\escape.txt"),
        ):
            with self.subTest(name=name), zipfile.ZipFile(io.BytesIO(samples[name])) as zipped:
                self.assertEqual(zipped.namelist(), [entry])

    def test_link_sample_is_archive_metadata_not_a_live_link(self):
        data = FACTORY.build_samples()["symlink.zip"]
        with zipfile.ZipFile(io.BytesIO(data)) as zipped:
            entry = zipped.infolist()[0]
            self.assertTrue(stat.S_ISLNK(entry.external_attr >> 16))
            self.assertEqual(zipped.read(entry), b"../sentinel.txt")

    def test_case_collision_contains_distinct_entries(self):
        with zipfile.ZipFile(io.BytesIO(FACTORY.build_samples()["case-collision.zip"])) as zipped:
            names = zipped.namelist()
            self.assertEqual(len(set(names)), 2)
            self.assertEqual(len({name.casefold() for name in names}), 1)

    def test_parent_collision_has_file_then_child(self):
        with zipfile.ZipFile(io.BytesIO(FACTORY.build_samples()["parent-collision.zip"])) as zipped:
            self.assertEqual(zipped.namelist(), ["folder", "folder/child.txt"])
            self.assertTrue(all(not item.is_dir() for item in zipped.infolist()))

    def test_corrupt_sample_fails_crc(self):
        with zipfile.ZipFile(io.BytesIO(FACTORY.build_samples()["bad-crc.zip"])) as zipped:
            self.assertEqual(zipped.testzip(), "hello.txt")

    def test_truncated_sample_has_no_valid_central_directory(self):
        with self.assertRaises(zipfile.BadZipFile):
            zipfile.ZipFile(io.BytesIO(FACTORY.build_samples()["truncated.zip"]))


if __name__ == "__main__":
    unittest.main()

import gzip
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest
import zipfile


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "large_fixture_generator", ROOT / "tests/large-archive-fixtures/generate.py"
)
GENERATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GENERATOR)


class LargeArchiveFixtureTests(unittest.TestCase):
    def test_boundary_archives_are_valid_and_expectations_straddle_limit(self):
        with tempfile.TemporaryDirectory(dir=ROOT / "tests/ci") as scratch:
            directory = Path(scratch) / "corpus"
            manifest = GENERATOR.build(directory)
            self.assertEqual(len(manifest["cases"]), 6)
            self.assertFalse(manifest["worker_execution_verified"])
            for name, artifact in manifest["artifacts"].items():
                path = directory / name
                self.assertEqual(GENERATOR.digest(path), artifact["sha256"])
                if name.endswith(".zip"):
                    with zipfile.ZipFile(path) as archive:
                        self.assertIsNone(archive.testzip())
                        payload = archive.read(GENERATOR.NAME)
                    decoded = len(payload)
                else:
                    data = gzip.decompress(path.read_bytes())
                    decoded = len(data)
                    with tarfile.open(fileobj=io.BytesIO(data), mode="r:") as archive:
                        payload = archive.extractfile(GENERATOR.NAME).read()
                self.assertEqual(payload, bytes(artifact["payload_bytes"]))
                self.assertEqual(decoded, artifact["decoded_bytes"])
                self.assertEqual(decoded, 65536)
                self.assertGreater(decoded / artifact["archive_bytes"], 100)
            for case in manifest["cases"]:
                decoded = manifest["artifacts"][case["file"]]["decoded_bytes"]
                failed = decoded > case["limits"]["output"]
                self.assertEqual(case["expected"]["error"], "Limit" if failed else "None")
                self.assertEqual(case["expected"]["created_count"], 0 if failed else 1)

    def test_generator_rejects_existing_destination_without_changes(self):
        with tempfile.TemporaryDirectory(dir=ROOT / "tests/ci") as scratch:
            directory = Path(scratch)
            marker = directory / "keep"
            marker.write_bytes(b"preserve")
            with self.assertRaises(FileExistsError):
                GENERATOR.build(directory)
            self.assertEqual(list(directory.iterdir()), [marker])
            self.assertEqual(marker.read_bytes(), b"preserve")

    def test_generator_requires_project_test_directory(self):
        with self.assertRaises(ValueError):
            GENERATOR.build(ROOT / "outside-tests")

    def test_payload_source_enforces_bounded_reads(self):
        source = GENERATOR.ZeroStream(GENERATOR.GIB)
        with self.assertRaises(ValueError):
            source.read()
        with self.assertRaises(ValueError):
            source.read(GENERATOR.CHUNK + 1)
        self.assertEqual(source.read(GENERATOR.CHUNK), bytes(GENERATOR.CHUNK))
        self.assertEqual(source.remaining, GENERATOR.GIB - GENERATOR.CHUNK)


if __name__ == "__main__":
    unittest.main()

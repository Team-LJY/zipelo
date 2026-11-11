"""원문 무결성과 고지 누락 시 패키지 생성 차단을 검증함."""

import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


LICENSES = load("license_tests", "packaging/licenses.py")
WINDOWS = load("windows_license_tests", "packaging/windows/rust/build.py")
UBUNTU = load("ubuntu_license_tests", "packaging/ubuntu/rust/build.py")


class LicenseTests(unittest.TestCase):
    def setUp(self):
        runs = ROOT / "tests/release-packages/runs"
        runs.mkdir(parents=True, exist_ok=True)
        temporary = tempfile.TemporaryDirectory(dir=runs)
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.manifest = self.root / "Cargo.toml"
        self.manifest.write_text("")
        (self.root / "LICENSE").write_text("Project fixture license\n")
        self.registry = self.root / "registry/src/index/sample-1.0.0"
        self.registry.mkdir(parents=True)
        self.package = {
            "name": "sample", "version": "1.0.0", "source": "registry+https://example.invalid/index",
            "license": "MIT", "license_file": None, "manifest_path": str(self.registry / "Cargo.toml"),
        }
        self.archive = self.root / "registry/cache/index/sample-1.0.0.crate"
        self.archive.parent.mkdir(parents=True)

    def fixture(self, files):
        with tarfile.open(self.archive, "w:gz") as archive:
            for relative, content in files.items():
                destination = self.registry / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_bytes(content)
                member = tarfile.TarInfo("sample-1.0.0/" + relative)
                member.size = len(content)
                archive.addfile(member, io.BytesIO(content))
        checksum = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        self.manifest.with_name("Cargo.lock").write_text(
            'version = 4\n[[package]]\nname = "sample"\nversion = "1.0.0"\n'
            'source = "registry+https://example.invalid/index"\n' + f'checksum = "{checksum}"\n'
        )
        return {"checksum": checksum}

    def test_inventory_matches_metadata_and_preserves_all_texts(self):
        originals = {"LICENSE-MIT": b"Fixture MIT text\r\n", "data/NOTICE": b"Fixture attribution\n"}
        self.fixture(originals)
        with patch.object(LICENSES, "metadata", return_value=[self.package]):
            records = LICENSES.inventory([self.manifest], self.root)
        text = LICENSES.render(records)
        self.assertEqual([(r["name"], r["version"]) for r in records], [("sample", "1.0.0")])
        self.assertEqual(text.count("Crate: sample 1.0.0\n"), 1)
        for content in originals.values():
            self.assertIn(content.decode(), text)
            self.assertIn(hashlib.sha256(content).hexdigest(), text)

    def test_missing_license_prevents_output(self):
        self.fixture({"README.md": b"No license text in fixture"})
        output = self.root / "THIRD-PARTY-LICENSES.txt"
        with patch.object(LICENSES, "metadata", return_value=[self.package]):
            with self.assertRaisesRegex(ValueError, "원문 누락 1개"):
                LICENSES.generate([self.manifest], output)
        self.assertFalse(output.exists())

    def test_metadata_omission_is_not_silently_ignored(self):
        self.fixture({"LICENSE": b"Fixture license"})
        with patch.object(LICENSES, "metadata", return_value=[]):
            with self.assertRaisesRegex(ValueError, "Cargo.lock/metadata 불일치"):
                LICENSES.inventory([self.manifest], self.root)

    def test_modified_registry_license_is_rejected(self):
        locked = self.fixture({"LICENSE": b"Fixture license"})
        (self.registry / "LICENSE").write_bytes(b"Modified license")
        with self.assertRaisesRegex(ValueError, "원문 변경"):
            LICENSES.registry_files(self.package, locked)

    def test_wrong_archive_checksum_is_rejected(self):
        self.fixture({"LICENSE": b"Fixture license"})
        with self.assertRaisesRegex(ValueError, "체크섬 불일치"):
            LICENSES.registry_files(self.package, {"checksum": "0" * 64})

    def test_license_file_cannot_escape_package(self):
        locked = self.fixture({"LICENSE": b"Fixture license"})
        self.package["license_file"] = str(self.root / "LICENSE")
        with self.assertRaises(ValueError):
            LICENSES.registry_files(self.package, locked)

    def test_metadata_command_is_locked_and_offline(self):
        with patch.object(LICENSES.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, '{"packages": []}')) as run:
            self.assertEqual(LICENSES.metadata(self.manifest), [])
        command = run.call_args.args[0]
        self.assertIn("--locked", command)
        self.assertIn("--offline", command)
        self.assertIn("--all-features", command)

    def test_both_packagers_stop_before_build_on_missing_licenses(self):
        for builder, binaries in [(WINDOWS, WINDOWS), (UBUNTU, UBUNTU.WINDOWS)]:
            with self.subTest(builder=builder.__name__):
                output = self.root / builder.__name__
                args = SimpleNamespace(output=output, target_dir=self.root / "target", profile="release",
                                       app_manifest=self.manifest, worker_manifest=self.manifest)
                with patch.object(binaries.LICENSES, "generate", side_effect=ValueError("원문 누락")):
                    with patch.object(binaries, "build_binary") as build_binary:
                        with self.assertRaisesRegex(ValueError, "원문 누락"):
                            builder.build(args)
                        build_binary.assert_not_called()


if __name__ == "__main__":
    unittest.main()

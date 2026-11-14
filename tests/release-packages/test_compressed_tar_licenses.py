"""압축 TAR 의존성의 원문 고지와 Rust 백엔드를 검증함."""

import importlib.util
from pathlib import Path
import unittest
import tomllib


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("compressed_tar_licenses", ROOT / "packaging/licenses.py")
LICENSES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(LICENSES)


class CompressedTarLicenses(unittest.TestCase):
    def test_seven_zip_encoder_reuses_pure_rust_package(self):
        manifest = ROOT / "experiments/rust-archive-worker/Cargo.toml"
        dependency = tomllib.loads(manifest.read_text())["dependencies"]["lzma-rust2"]
        self.assertEqual(dependency["version"], "=0.25.0")
        self.assertFalse(dependency["default-features"])
        self.assertEqual(set(dependency["features"]), {"std", "xz", "encoder"})
        package = next(p for p in LICENSES.metadata(manifest) if p["name"] == "lzma-rust2")
        original = tomllib.loads((Path(package["manifest_path"]).parent / "Cargo.toml.orig").read_text())
        self.assertEqual(original["features"]["encoder"], [])
        self.assertEqual(original["package"]["license"], "Apache-2.0")

    def test_registry_originals_are_in_generated_worker_notice(self):
        manifest = ROOT / "experiments/rust-archive-worker/Cargo.toml"
        records = LICENSES.inventory([manifest])
        selected = {record["name"]: record for record in records}
        expected = {
            "bzip2": ("0.6.1", "MIT OR Apache-2.0"),
            "libbz2-rs-sys": ("0.2.6", "bzip2-1.0.6"),
            "lzma-rust2": ("0.25.0", "Apache-2.0"),
        }
        notice = LICENSES.render(records)
        for name, (version, license_id) in expected.items():
            with self.subTest(name=name):
                record = selected[name]
                self.assertEqual(record["version"], version)
                self.assertEqual(record["license"], license_id)
                self.assertTrue(record["files"])
                self.assertIn(f"Crate: {name} {version}\n", notice)
                for original in record["files"]:
                    self.assertIn(original["text"], notice)
                    self.assertIn(original["sha256"], notice)
        self.assertTrue({"bzip2-sys", "lzma-sys", "liblzma-sys", "xz2"}.isdisjoint(selected))


if __name__ == "__main__":
    unittest.main()

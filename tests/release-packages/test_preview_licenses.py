"""미리보기 디코더의 선택한 라이선스와 고지 원문을 검증함."""
import importlib.util
from pathlib import Path
import tempfile
from unittest.mock import patch
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("preview_licenses", ROOT / "packaging/licenses.py")
LICENSES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(LICENSES)


class PreviewLicenses(unittest.TestCase):
    def test_path_package_preserves_original_and_refuses_license_escape(self):
        runs = ROOT / "tests/release-packages/runs"
        runs.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=runs) as temporary:
            root = Path(temporary)
            (root / "LICENSE").write_text("Project MIT fixture\n")
            package_root = root / "vendor"
            package_root.mkdir()
            manifest = package_root / "Cargo.toml"
            manifest.write_text("")
            manifest.with_name("Cargo.lock").write_text(
                'version = 4\n[[package]]\nname = "sample"\nversion = "1.0.0"\n'
            )
            original = package_root / "LICENSE-MIT"
            original.write_text("Upstream MIT fixture\n")
            package = {
                "name": "sample", "version": "1.0.0", "source": None,
                "license": "MIT", "license_file": "LICENSE-MIT",
                "manifest_path": str(manifest),
            }
            with patch.object(LICENSES, "metadata", return_value=[package]):
                records = LICENSES.inventory([manifest], root)
                self.assertEqual(records[0]["files"][0]["text"], original.read_text())
                package["license_file"] = "../LICENSE"
                with self.assertRaisesRegex(ValueError, "라이선스 경로 이탈"):
                    LICENSES.inventory([manifest], root)
                linked = package_root / "linked-license"
                linked.symlink_to(original)
                package["license_file"] = "linked-license"
                with self.assertRaisesRegex(ValueError, "라이선스 경로 이탈"):
                    LICENSES.inventory([manifest], root)

    def test_decoder_notices_include_selected_mit_and_verified_originals(self):
        records = LICENSES.inventory([ROOT / "experiments/rust-archive-worker/Cargo.toml"])
        selected = {record["name"]: record for record in records}
        for name, version in [("png", "0.18.1"), ("zipelo-jpeg", "0.5.15"), ("zune-core", "0.5.3")]:
            with self.subTest(name=name):
                record = selected[name]
                self.assertEqual(record["version"], version)
                self.assertEqual(record["selected_license"], "MIT")
                self.assertIn("MIT", record["license"].split(" OR "))
                self.assertTrue(record["files"])
                notice = LICENSES.render([record])
                self.assertIn("Selected license: MIT", notice)
                for original in record["files"]:
                    self.assertIn(original["text"], notice)
                    self.assertIn(original["sha256"], notice)
        local = selected["zipelo-jpeg"]
        original = (ROOT / "experiments/rust-archive-worker/vendor/zune-jpeg/LICENSE-MIT").read_text()
        self.assertEqual(local["files"][0]["text"], original)
        self.assertIn("zune-image developers", original)
        self.assertTrue({"libpng-sys", "libjpeg-sys", "mozjpeg-sys", "libz-sys"}.isdisjoint(selected))


if __name__ == "__main__":
    unittest.main()

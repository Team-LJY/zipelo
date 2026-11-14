"""미리보기 디코더의 선택한 라이선스와 고지 원문을 검증함."""
import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("preview_licenses", ROOT / "packaging/licenses.py")
LICENSES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(LICENSES)


class PreviewLicenses(unittest.TestCase):
    def test_decoder_notices_include_selected_mit_and_verified_originals(self):
        records = LICENSES.inventory([ROOT / "experiments/rust-archive-worker/Cargo.toml"])
        selected = {record["name"]: record for record in records}
        for name, version in [("png", "0.18.1"), ("zune-jpeg", "0.5.15"), ("zune-core", "0.5.3")]:
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
        self.assertTrue({"libpng-sys", "libjpeg-sys", "mozjpeg-sys", "libz-sys"}.isdisjoint(selected))


if __name__ == "__main__":
    unittest.main()

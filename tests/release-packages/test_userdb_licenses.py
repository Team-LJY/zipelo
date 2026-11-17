"""userdb JSON 해석기의 배포 고지 원문을 검증함."""

import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("userdb_licenses", ROOT / "packaging/licenses.py")
LICENSES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(LICENSES)


class UserdbLicenses(unittest.TestCase):
    def test_json_dependencies_preserve_selected_mit_notices(self):
        records = LICENSES.inventory([ROOT / "experiments/rust-archive-worker/Cargo.toml"])
        selected = {record["name"]: record for record in records}
        for name, version in [("serde", "1.0.229"), ("serde_core", "1.0.229"), ("serde_json", "1.0.154")]:
            with self.subTest(name=name):
                record = selected[name]
                self.assertEqual(record["version"], version)
                self.assertEqual(record["selected_license"], "MIT")
                self.assertIn("MIT", record["license"].split(" OR "))
                self.assertTrue(record["files"])
                notice = LICENSES.render([record])
                for original in record["files"]:
                    self.assertIn(original["text"], notice)
                    self.assertIn(original["sha256"], notice)
        self.assertTrue({"itoa", "memchr", "zmij"}.issubset(selected))


if __name__ == "__main__":
    unittest.main()

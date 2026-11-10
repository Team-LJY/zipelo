import hashlib
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

import validate as v


class ManifestTests(unittest.TestCase):
    def row(self, path="a/b", value="0" * 64):
        return (value + "  " + path + "\n").encode()

    def test_valid_manifest(self):
        self.assertEqual(v.parse_manifest(self.row(), {"a/b"}), {"a/b": "0" * 64})

    def test_duplicate_path(self):
        with self.assertRaises(ValueError):
            v.parse_manifest(self.row() * 2, {"a/b"})

    def test_missing_path(self):
        with self.assertRaises(ValueError):
            v.parse_manifest(self.row(), {"a/b", "a/c"})

    def test_extra_path(self):
        with self.assertRaises(ValueError):
            v.parse_manifest(self.row() + self.row("a/c"), {"a/b"})

    def test_malformed_hash_rows(self):
        for value in ["0" * 63, "0" * 65, "g" * 64, "A" * 64]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                v.parse_manifest(self.row(value=value), {"a/b"})

    def test_noncanonical_paths(self):
        for path in ["/a/b", "../b", "a/../b", "a//b", "./a/b", "a/./b", "a\\b"]:
            with self.subTest(path=path), self.assertRaises(ValueError):
                v.parse_manifest(self.row(path), {path})

    def test_blank_or_missing_final_newline(self):
        for data in [b"", self.row().rstrip(b"\n"), self.row() + b"\n"]:
            with self.subTest(data=data), self.assertRaises(ValueError):
                v.parse_manifest(data, {"a/b"})

    def test_symlink_component_rejected(self):
        base = v.ROOT / "tests/rust-ui-operation-reducer/artifacts"
        base.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=base) as name:
            root = Path(name)
            (root / "real").mkdir()
            (root / "real/b").write_text("fixture")
            (root / "a").symlink_to("real", target_is_directory=True)
            with self.assertRaises(ValueError):
                v.checked_path(root, "a/b")


class PreCargoTests(unittest.TestCase):
    def setUp(self):
        base = v.ROOT / "tests/rust-ui-operation-reducer/artifacts"
        base.mkdir(exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=base)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for name in v.BASE_PATHS | v.REDUCER_PATHS | {v.BASE_MANIFEST, v.REDUCER_MANIFEST}:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(v.ROOT / name, path)
        self.expected = hashlib.sha256((self.root / v.REDUCER_MANIFEST).read_bytes()).hexdigest()

    def reject_before_cargo(self, expected=None):
        with patch.object(v, "run") as cargo:
            with self.assertRaises(ValueError):
                v.main(["--source-head", "1" * 40, "--reducer-manifest-sha256", expected or self.expected], root=self.root)
            cargo.assert_not_called()
        self.assertFalse((self.root / "tests/rust-ui-operation-reducer/artifacts").exists())

    def test_valid_preflight_without_cargo(self):
        self.assertTrue(v.preflight(self.root, self.expected))

    def test_baseline_already_tampered_rejected(self):
        path = self.root / "experiments/rust-ui-probe/src/lib.rs"
        path.write_bytes(path.read_bytes() + b"\n")
        self.reject_before_cargo()

    def test_reducer_already_tampered_rejected(self):
        path = self.root / "experiments/rust-ui-operation-reducer/src/lib.rs"
        path.write_bytes(path.read_bytes() + b"\n")
        self.reject_before_cargo()

    def test_changed_baseline_manifest_rejected(self):
        path = self.root / v.BASE_MANIFEST
        lines = path.read_text().splitlines()
        lines[0] = "0" * 64 + lines[0][64:]
        path.write_text("\n".join(lines) + "\n")
        self.reject_before_cargo()

    def test_reducer_manifest_anchor_mismatch_rejected(self):
        self.reject_before_cargo("0" * 64)

    def test_helper_already_tampered_rejected(self):
        path = self.root / "tests/rust-ui-operation-reducer/validate.py"
        path.write_bytes(path.read_bytes() + b"\n")
        self.reject_before_cargo()

    def test_missing_source_rejected(self):
        (self.root / "experiments/rust-ui-operation-reducer/src/lib.rs").unlink()
        self.reject_before_cargo()


if __name__ == "__main__":
    unittest.main(verbosity=2)

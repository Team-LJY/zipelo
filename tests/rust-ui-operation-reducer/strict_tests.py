import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
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


class OptimizationTests(unittest.TestCase):
    def check_optimized_mode(self, flags, optimize):
        base = v.ROOT / "tests/rust-ui-operation-reducer/artifacts"
        base.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=base) as name:
            env = os.environ.copy()
            env.pop("PYTHONOPTIMIZE", None)
            env["PYTHONDONTWRITEBYTECODE"] = "1"
            if optimize is not None:
                env["PYTHONOPTIMIZE"] = optimize
            runner = """
import json
from pathlib import Path
import sys
from unittest.mock import patch
import validate

with patch.object(validate, "run") as cargo:
    try:
        validate.main([], root=Path(sys.argv[1]))
    finally:
        print(json.dumps({"cargo_calls": cargo.call_count, "optimize": sys.flags.optimize}))
"""
            result = subprocess.run(
                [sys.executable, *flags, "-c", runner, name],
                cwd=Path(v.__file__).parent,
                env=env,
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("RuntimeError: 최적화 모드에서는 검증을 실행할 수 없음", result.stderr)
            evidence = json.loads(result.stdout)
            self.assertGreater(evidence["optimize"], 0)
            self.assertEqual(evidence["cargo_calls"], 0)
            self.assertEqual(list(Path(name).iterdir()), [])

    def test_dash_o_rejected_before_artifacts_and_cargo(self):
        self.check_optimized_mode(["-O"], None)

    def test_environment_optimization_rejected_before_artifacts_and_cargo(self):
        self.check_optimized_mode([], "1")


if __name__ == "__main__":
    unittest.main(verbosity=2)

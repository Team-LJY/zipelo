import importlib.util
import os
import subprocess
from pathlib import Path
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "native_smoke", ROOT / "tests/rust-app-ui/native_smoke.py"
)
SMOKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SMOKE)


class NativeSmokePathsTests(unittest.TestCase):
    def test_default_binary_uses_common_work_target(self):
        common = Path(subprocess.check_output(
            ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
            cwd=ROOT, text=True,
        ).strip()).resolve()
        expected = common.parent / "tests/work/rust-local"
        with patch.dict(os.environ):
            os.environ.pop("ZIPELO_CI_JOB", None)
            os.environ.pop("CARGO_TARGET_DIR", None)
            work = SMOKE.work_directory()
            self.assertEqual(work, expected)
            self.assertEqual(SMOKE.binary_path(work),
                             work / "target/debug/zipelo-ui-memory-app")

    def test_job_and_target_overrides_are_honored(self):
        work = SMOKE.work_directory().parent / "native-smoke-paths"
        target = work / "custom-target"
        with patch.dict(os.environ, {"ZIPELO_CI_JOB": "native-smoke-paths",
                                     "CARGO_TARGET_DIR": str(target)}):
            self.assertEqual(SMOKE.work_directory(), work)
            self.assertEqual(SMOKE.binary_path(work),
                             target / "debug/zipelo-ui-memory-app")


if __name__ == "__main__":
    unittest.main()

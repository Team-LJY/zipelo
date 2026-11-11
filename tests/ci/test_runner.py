from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

import run_rust


class RunnerTests(unittest.TestCase):
    def test_all_manifests_have_locks(self):
        paths = run_rust.manifests(run_rust.ROOT)
        expected = set(run_rust.ROOT.glob("experiments/*/Cargo.toml"))
        expected |= set(run_rust.ROOT.glob("tests/*/Cargo.toml"))
        self.assertEqual(set(paths), expected)

    def test_failure_does_not_hide_later_crates(self):
        paths = [
            Path("/repo/experiments/a/Cargo.toml"),
            Path("/repo/experiments/b/Cargo.toml"),
        ]
        with (
            patch.object(run_rust, "manifests", return_value=paths),
            patch.object(run_rust.subprocess, "run", side_effect=[
                subprocess.CompletedProcess([], 7),
                subprocess.CompletedProcess([], 0),
            ]),
        ):
            results = run_rust.run(Path("/repo"), "build")
        self.assertEqual([row["status"] for row in results], ["failed", "passed"])
        self.assertEqual(results[0]["exit_code"], 7)

    def test_windows_exclusion_is_explicit_and_only_for_linux_tests(self):
        paths = [
            Path("/repo/tests/rust-file-publication/Cargo.toml"),
            Path("/repo/experiments/rust-file-publication/Cargo.toml"),
        ]
        with (
            patch.object(run_rust, "manifests", return_value=paths),
            patch.object(run_rust.sys, "platform", "win32"),
            patch.object(
                run_rust.subprocess, "run",
                return_value=subprocess.CompletedProcess([], 0),
            ) as execute,
        ):
            results = run_rust.run(Path("/repo"), "test")
        self.assertEqual(
            [row["status"] for row in results], ["not-applicable", "passed"]
        )
        self.assertEqual(execute.call_count, 1)

    def test_fixture_bytes_match_git_blobs(self):
        names = subprocess.check_output(
            ["git", "ls-files", "-z", "tests/**/fixtures/*"], cwd=run_rust.ROOT
        ).decode("utf-8").split("\0")
        names = [name for name in names if name]
        self.assertTrue(names, "검증할 fixture가 없음")
        for name in names:
            with self.subTest(path=name):
                expected = subprocess.check_output(
                    ["git", "show", f"HEAD:{name}"], cwd=run_rust.ROOT
                )
                self.assertEqual((run_rust.ROOT / name).read_bytes(), expected)

    def test_cross_build_never_executes_tests(self):
        args = run_rust.command(Path("tests/example/Cargo.toml"), "cross-build")
        self.assertEqual(args[2], "build")
        self.assertIn(run_rust.WINDOWS_TARGET, args)
        self.assertNotIn("--test-threads=1", args)

    def test_preflight_failure_is_reported_without_hiding_other_phase(self):
        with (
            patch.object(run_rust, "prepare_cross_build", side_effect=ValueError("missing linker")),
            patch.object(run_rust, "run", return_value=[{"manifest": "sample", "status": "passed"}]),
        ):
            results = run_rust.execute_phases(Path("/repo"), ["build", "cross-build"])
        self.assertEqual([row["status"] for row in results], ["passed", "failed"])
        self.assertEqual(results[1]["reason"], "missing linker")

    def test_cargo_contract(self):
        args = run_rust.command(Path("tests/example/Cargo.toml"), "test")
        self.assertIn("--locked", args)
        self.assertIn("--all-features", args)
        self.assertEqual(args[-2:], ["--", "--test-threads=1"])


if __name__ == "__main__":
    unittest.main()

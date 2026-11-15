from contextlib import redirect_stdout
from io import StringIO
import json
import os
from pathlib import Path, PurePosixPath, PureWindowsPath
import tempfile
import subprocess
import sys
import unittest
from unittest.mock import ANY, patch

import run_rust


TEST_WORK = run_rust.work_directory(
    run_rust.ROOT, os.environ.get("ZIPELO_CI_JOB", "rust-local")
)


class RunnerTests(unittest.TestCase):
    def test_job_directory_is_restricted_to_one_work_child(self):
        for name in ("../escape", "other/job", "", ".hidden", "bad name"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                run_rust.work_directory(run_rust.ROOT, name)
        self.assertEqual(run_rust.work_directory(
            run_rust.ROOT, os.environ.get("ZIPELO_CI_JOB", "rust-local")
        ), TEST_WORK)

    def test_work_directory_rejects_symlinked_parents_before_creation(self):
        for link_name in ("tests", "tests/work"):
            with self.subTest(link_name=link_name), tempfile.TemporaryDirectory(
                dir=TEST_WORK
            ) as directory:
                base = Path(directory)
                project = base / "project"
                project.mkdir()
                outside = base / "outside"
                outside.mkdir()
                link = project / link_name
                link.parent.mkdir(parents=True, exist_ok=True)
                link.symlink_to(outside, target_is_directory=True)
                with patch.object(
                    run_rust.subprocess, "check_output", return_value=str(project / ".git")
                ):
                    with self.assertRaisesRegex(ValueError, "안전한 작업 디렉터리"):
                        run_rust.work_directory(project, "task")
                self.assertFalse((outside / "task").exists())
                self.assertFalse((outside / "work").exists())

    def test_fixture_parent_symlink_is_rejected_before_creating_children(self):
        with tempfile.TemporaryDirectory(dir=TEST_WORK) as directory:
            base = Path(directory)
            work = base / "work"
            work.mkdir()
            outside = base / "outside"
            outside.mkdir()
            (work / "fixtures").symlink_to(outside, target_is_directory=True)
            with (
                patch.object(run_rust, "test_work_directory", return_value=work),
                self.assertRaisesRegex(ValueError, "작업 폴더 밖"),
            ):
                run_rust.prepare_fixtures(run_rust.ROOT)
            self.assertEqual(list(outside.iterdir()), [])

    def test_windows_setup_rejects_symlinked_parent_before_creating_tools(self):
        with tempfile.TemporaryDirectory(dir=TEST_WORK) as directory:
            base = Path(directory)
            project = base / "project"
            ci = project / "tests/ci"
            ci.mkdir(parents=True)
            work = project / "tests/work/task"
            work.mkdir(parents=True)
            script = ci / "setup_windows.sh"
            script.write_bytes((run_rust.ROOT / "tests/ci/setup_windows.sh").read_bytes())
            commands = base / "bin"
            commands.mkdir()
            git = commands / "git"
            git.write_text('#!/bin/sh\nprintf "%s\n" "$FAKE_GIT_COMMON"\n')
            git.chmod(0o700)
            outside = base / "outside"
            outside.mkdir()
            (work / "linked").symlink_to(outside, target_is_directory=True)
            environment = {
                **os.environ,
                "PATH": str(commands) + os.pathsep + os.environ["PATH"],
                "FAKE_GIT_COMMON": str(project / ".git"),
                "ZIPELO_CI_JOB": "task",
                "ZIPELO_CI_TOOLS": str(work / "linked/tools"),
            }
            result = subprocess.run(["bash", str(script)], env=environment,
                                    capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("작업 폴더 밖", result.stderr)
            self.assertEqual(list(outside.iterdir()), [])

    def test_output_paths_reject_symlinks_outside_job(self):
        with tempfile.TemporaryDirectory(dir=TEST_WORK) as directory:
            work = Path(directory)
            outside = TEST_WORK / "outside"
            for name in ("tmp", "target"):
                with self.subTest(name=name):
                    link = work / name
                    link.symlink_to(outside, target_is_directory=True)
                    with self.assertRaisesRegex(ValueError, "작업 폴더 밖"):
                        run_rust.output_directory(work, name, create=True)
                    self.assertFalse(outside.exists())
                    link.unlink()

    def test_output_paths_allow_symlinks_within_job(self):
        with tempfile.TemporaryDirectory(dir=TEST_WORK) as directory:
            work = Path(directory)
            inner = work / "inner"
            inner.mkdir()
            link = work / "tmp"
            link.symlink_to(inner, target_is_directory=True)
            self.assertEqual(run_rust.output_directory(work, "tmp", create=True), link)

    def test_cargo_and_temp_paths_are_kept_in_job_directory(self):
        if "ZIPELO_TEST_WORK_DIR" not in os.environ:
            self.skipTest("러너가 설정한 작업 경로에서 검사함")
        for key in ("CARGO_TARGET_DIR", "TMPDIR", "TMP", "TEMP", "ZIPELO_TEST_WORK_DIR"):
            with self.subTest(key=key):
                self.assertTrue(Path(os.environ[key]).resolve().is_relative_to(TEST_WORK))

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

    def test_self_check_failure_replaces_previous_success_report(self):
        for phase, platform in (("local", "linux"), ("test", "win32")):
            with self.subTest(phase=phase), tempfile.TemporaryDirectory(
                dir=TEST_WORK
            ) as directory:
                root = Path(directory)
                (root / "work/tmp").mkdir(parents=True)
                report_path = root / f"work/{phase}-results.json"
                with (
                    patch.object(run_rust, "ROOT", root),
                    patch.object(run_rust, "work_directory", return_value=root / "work"),
                    patch.object(run_rust, "prepare_tools"),
                    patch.object(run_rust, "prepare_local_tools"),
                    patch.object(run_rust.sys, "argv", ["run_rust.py", phase]),
                    patch.object(run_rust.sys, "platform", platform),
                    patch.dict(run_rust.os.environ, {"GITHUB_STEP_SUMMARY": ""}),
                    patch.object(run_rust.subprocess, "run", side_effect=[
                        subprocess.CompletedProcess([], 0),
                        subprocess.CompletedProcess([], 7),
                    ]),
                    patch.object(run_rust.subprocess, "check_output", side_effect=[
                        "successful-head\n", "failed-head\n",
                    ]),
                    patch.object(run_rust, "execute_phases", return_value=[
                        {"phase": "test", "manifest": "sample", "status": "passed"},
                    ]) as execute,
                    redirect_stdout(StringIO()),
                ):
                    self.assertEqual(run_rust.main(), 0)
                    successful = json.loads(report_path.read_text(encoding="utf-8"))
                    self.assertEqual(successful["results"][0]["status"], "passed")
                    self.assertEqual(run_rust.main(), 7)
                    failed = json.loads(report_path.read_text(encoding="utf-8"))
                self.assertEqual(failed["head"], "failed-head")
                self.assertEqual(failed["results"], [{
                    "phase": "self-check",
                    "manifest": "tests/ci",
                    "status": "failed",
                    "exit_code": 7,
                }])
                self.assertFalse(failed["windows_runtime_tested"])
                execute.assert_called_once()

    @unittest.skipUnless(sys.platform == "linux", "Linux MinGW 실행 파일 탐색 시험임")
    def test_linked_worktree_finds_setup_tools_without_path_export(self):
        with tempfile.TemporaryDirectory(dir=TEST_WORK) as directory:
            base = Path(directory)
            root = base / "linked"
            tools = base / "tests/ci/tools"
            binaries = tools / "mingw/usr/bin"
            binaries.mkdir(parents=True)
            for name in ("gcc", "g++"):
                executable = binaries / f"x86_64-w64-mingw32-{name}-posix"
                executable.write_text("#!/bin/sh\nexit 0\n")
                executable.chmod(0o700)
            rustup = tools / "rustup"
            (rustup / "toolchains/1.99.1-x86_64-unknown-linux-gnu").mkdir(parents=True)
            with (
                patch.dict(run_rust.os.environ, {"PATH": ""}, clear=True),
                patch.object(run_rust.subprocess, "check_output", return_value=str(base / ".git")),
            ):
                run_rust.prepare_tools(root)
                run_rust.prepare_cross_build()
                self.assertEqual(run_rust.os.environ["RUSTUP_HOME"], str(rustup))
                self.assertEqual(
                    run_rust.os.environ["CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER"],
                    str(binaries / "x86_64-w64-mingw32-gcc-posix"),
                )

    @unittest.skipUnless(sys.platform == "linux", "Linux MinGW 실행 파일 탐색 시험임")
    def test_default_checkout_reuses_existing_setup_tools(self):
        with tempfile.TemporaryDirectory(dir=TEST_WORK) as directory:
            root = Path(directory)
            tools = root / "tests/ci/tools"
            binaries = tools / "mingw/usr/bin"
            binaries.mkdir(parents=True)
            executable = binaries / "x86_64-w64-mingw32-gcc-posix"
            executable.write_text("#!/bin/sh\nexit 0\n")
            executable.chmod(0o700)
            rustup = tools / "rustup"
            (rustup / "toolchains/1.99.1-x86_64-unknown-linux-gnu").mkdir(parents=True)
            with (
                patch.dict(run_rust.os.environ, {"PATH": ""}, clear=True),
                patch.object(run_rust.subprocess, "check_output", return_value=str(root / ".git")),
            ):
                run_rust.prepare_tools(root)
                self.assertEqual(run_rust.os.environ["RUSTUP_HOME"], str(rustup))
                self.assertTrue(run_rust.os.environ["PATH"].startswith(str(binaries)))
                self.assertEqual(run_rust.shutil.which("x86_64-w64-mingw32-gcc-posix"),
                                 str(executable))

    def test_unconfigured_work_directory_uses_common_project(self):
        with patch.dict(os.environ), patch.object(
            run_rust, "work_directory", return_value=TEST_WORK
        ) as directory:
            os.environ.pop("ZIPELO_TEST_WORK_DIR", None)
            os.environ.pop("ZIPELO_CI_JOB", None)
            self.assertEqual(run_rust.test_work_directory(run_rust.ROOT), TEST_WORK)
            directory.assert_called_once_with(run_rust.ROOT, "rust-local")

    def test_local_bootstraps_missing_cross_tools_and_reloads_paths(self):
        for root in (Path("/repo"), PurePosixPath("/repo"), PureWindowsPath("C:/repo")):
            with self.subTest(root=root):
                with (
                    patch.object(run_rust, "prepare_cross_build", side_effect=[ValueError("missing"), None]) as detect,
                    patch.object(run_rust.subprocess, "run") as setup,
                    patch.object(run_rust, "prepare_tools") as reload,
                    patch.object(run_rust, "test_work_directory", return_value=root / "tests/work/rust-local"),
                ):
                    run_rust.prepare_local_tools(root)
                setup.assert_called_once_with(
                    ["bash", str(root / "tests/ci/setup_windows.sh")], cwd=root, env=ANY, check=True,
                )
                reload.assert_called_once_with(root)
                self.assertEqual(detect.call_count, 2)

    def test_existing_cross_tools_do_not_trigger_downloads(self):
        with (
            patch.object(run_rust, "prepare_cross_build"),
            patch.object(run_rust.subprocess, "run") as setup,
        ):
            run_rust.prepare_local_tools(Path("/repo"))
        setup.assert_not_called()

    def test_explicit_tools_directory_does_not_fall_back_to_other_worktree(self):
        with (
            patch.dict(run_rust.os.environ, {"PATH": "", "ZIPELO_CI_TOOLS": "/missing"}, clear=True),
            patch.object(run_rust.subprocess, "check_output") as git,
        ):
            run_rust.prepare_tools(Path("/repo"))
            git.assert_not_called()
            with self.assertRaisesRegex(ValueError, "setup_windows.sh"):
                run_rust.prepare_cross_build()

    @unittest.skipUnless(sys.platform == "linux", "Linux 파일시스템과 소유권 검사 시험임")
    def test_filesystem_skip_is_limited_to_publication_test_crates(self):
        with tempfile.TemporaryDirectory(dir=TEST_WORK) as directory:
            root = Path(directory)
            work = root / "work"
            work.mkdir()
            paths = [
                root / "tests/rust-file-publication/Cargo.toml",
                root / "tests/rust-gzip-file-job/Cargo.toml",
                root / "tests/rust-file-snapshot/Cargo.toml",
                root / "experiments/rust-file-publication/Cargo.toml",
            ]
            with (
                patch.object(run_rust, "manifests", return_value=paths),
                patch.dict(run_rust.os.environ, {"ZIPELO_TEST_WORK_DIR": str(work)}),
                patch.object(run_rust.subprocess, "check_output", return_value="1021994\n"),
                patch.object(run_rust.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)) as cargo,
            ):
                results = run_rust.run(root, "test")
                self.assertEqual([row["status"] for row in results], [
                    "not-applicable", "not-applicable", "passed", "passed",
                ])
                self.assertIn("0x1021994", results[0]["reason"])
                self.assertEqual(cargo.call_count, 2)
                cargo.reset_mock()
                results = run_rust.run(root, "cross-build")
                self.assertTrue(all(row["status"] == "passed" for row in results))
                self.assertEqual(cargo.call_count, 4)

    @unittest.skipUnless(sys.platform == "linux", "Linux 소켓 경로 회귀 시험임")
    def test_long_checkout_does_not_block_file_fixtures(self):
        with tempfile.TemporaryDirectory(dir=TEST_WORK) as directory:
            root = Path(directory) / ("long-checkout-" * 10)
            work = Path(directory) / "work"
            socket = root / "tests/rust-file-publication/runs/case-1-1/parent/output"
            self.assertGreaterEqual(len(run_rust.os.fsencode(socket)), 108)
            with (
                patch.dict(run_rust.os.environ, {"ZIPELO_TEST_WORK_DIR": str(work)}),
                patch.object(run_rust.subprocess, "check_output", return_value="ef53\n"),
            ):
                self.assertEqual(run_rust.prepare_fixtures(root), {})
            for name in run_rust.FILE_FIXTURES:
                self.assertEqual(list((work / "fixtures" / name).iterdir()), [])
                self.assertFalse((root / "tests" / name / "runs").exists())

    def test_unknown_filesystem_probe_error_is_failure(self):
        with (
            patch.object(run_rust, "prepare_fixtures", side_effect=subprocess.CalledProcessError(1, "stat")),
            patch.object(run_rust.sys, "platform", "linux"),
        ):
            results = run_rust.execute_phases(run_rust.ROOT, ["test"])
        self.assertEqual(results[0]["status"], "failed")

    def test_cargo_contract(self):
        args = run_rust.command(Path("tests/example/Cargo.toml"), "test")
        self.assertIn("--locked", args)
        self.assertIn("--all-features", args)
        self.assertEqual(args[-2:], ["--", "--test-threads=1"])


if __name__ == "__main__":
    unittest.main()

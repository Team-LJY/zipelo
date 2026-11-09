import contextlib
import importlib.util
import io
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("zipelo_runner_under_test", ROOT / "tests/run_checks.py")
RUNNER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNNER)


class RunnerTests(unittest.TestCase):
    def invoke(self, run, runtime=False):
        with (
            patch.object(RUNNER.shutil, "which", return_value="dotnet"),
            patch.object(RUNNER.subprocess, "run", side_effect=run),
            contextlib.redirect_stdout(io.StringIO()),
        ):
            return RUNNER.runtime_checks() if runtime else RUNNER.core_checks()

    def create_assembly(self, command):
        directory = Path(command[command.index("--output") + 1])
        directory.mkdir()
        (directory / "Zipelo.SecurityTests.dll").write_bytes(b"runner-test-only")

    def test_build_options_are_separate_from_two_application_arguments(self):
        calls = []

        def run(command, **options):
            calls.append((command, options))
            sandbox = options["cwd"]
            self.assertTrue(sandbox.is_relative_to(RUNNER.SECURITY / ".work"))
            self.assertTrue(sandbox.is_dir())
            self.assertFalse(options["check"])
            for variable in (
                "DOTNET_CLI_HOME", "NUGET_PACKAGES", "NUGET_HTTP_CACHE_PATH", "TMPDIR", "TMP", "TEMP"
            ):
                self.assertTrue(Path(options["env"][variable]).is_relative_to(sandbox))
            if len(calls) == 1:
                self.assertEqual(command[:3], [
                    "dotnet", "build", str(RUNNER.SECURITY / "Zipelo.SecurityTests.csproj")
                ])
                self.assertEqual(command[command.index("--configfile") + 1],
                                 str(RUNNER.SECURITY / "NuGet.Config"))
                self.assertTrue(Path(command[command.index("--artifacts-path") + 1]).is_relative_to(sandbox))
                self.assertEqual(options["timeout"], RUNNER.BUILD_TIMEOUT)
                self.create_assembly(command)
            else:
                self.assertEqual(command, [
                    "dotnet", str(sandbox / "bin/Zipelo.SecurityTests.dll"),
                    str(ROOT), str(sandbox),
                ])
                self.assertEqual(options["timeout"], RUNNER.CORE_TIMEOUT)
            return subprocess.CompletedProcess(command, 0)

        self.assertEqual(self.invoke(run), 0)
        self.assertEqual(len(calls), 2)
        self.assertFalse(calls[0][1]["cwd"].exists())

    def test_runtime_builds_worker_and_keeps_only_two_app_arguments(self):
        calls = []

        def run(command, **options):
            calls.append(command)
            sandbox = options["cwd"]
            if len(calls) == 1:
                self.assertEqual(command[2], str(ROOT / "tests/runtime/Zipelo.RuntimeTests.csproj"))
                self.assertEqual(command[command.index("--configfile") + 1], str(RUNNER.SECURITY / "NuGet.Config"))
                directory = Path(command[command.index("--output") + 1])
                self.assertTrue(directory.is_relative_to(sandbox))
                directory.mkdir()
                for name in ("Zipelo.RuntimeTests.dll", "Zipelo.Worker.dll", "Zipelo.Worker.deps.json",
                             "Zipelo.Worker.runtimeconfig.json"):
                    (directory / name).write_bytes(b"runner-test-only")
            else:
                self.assertEqual(command, ["dotnet", str(sandbox / "bin/Zipelo.RuntimeTests.dll"), str(ROOT), str(sandbox)])
                self.assertEqual(options["timeout"], 120)
            return subprocess.CompletedProcess(command, 0)

        self.assertEqual(self.invoke(run, runtime=True), 0)
        self.assertEqual(len(calls), 2)

    def test_runtime_missing_worker_stops_before_application_launch(self):
        calls = []

        def run(command, **options):
            calls.append(command)
            directory = Path(command[command.index("--output") + 1])
            directory.mkdir()
            (directory / "Zipelo.RuntimeTests.dll").write_bytes(b"runner-test-only")
            return subprocess.CompletedProcess(command, 0)

        self.assertEqual(self.invoke(run, runtime=True), 2)
        self.assertEqual(len(calls), 1)

    def test_build_failure_stops_before_application_launch(self):
        calls = []

        def run(command, **options):
            calls.append((command, options["cwd"]))
            return subprocess.CompletedProcess(command, 17)

        self.assertEqual(self.invoke(run), 17)
        self.assertEqual(len(calls), 1)
        self.assertFalse(calls[0][1].exists())

    def test_application_failure_is_preserved(self):
        calls = []

        def run(command, **options):
            calls.append(command)
            if len(calls) == 1:
                self.create_assembly(command)
            return subprocess.CompletedProcess(command, 0 if len(calls) == 1 else 9)

        self.assertEqual(self.invoke(run), 9)
        self.assertEqual(len(calls), 2)

    def test_missing_assembly_is_not_reported_as_success(self):
        calls = []

        def run(command, **options):
            calls.append(command)
            return subprocess.CompletedProcess(command, 0)

        self.assertEqual(self.invoke(run), 2)
        self.assertEqual(len(calls), 1)

    def test_timeouts_fail_and_remove_owned_sandbox(self):
        for stage in (1, 2):
            with self.subTest(stage=stage):
                calls = []

                def run(command, **options):
                    calls.append(options["cwd"])
                    if len(calls) == stage:
                        raise subprocess.TimeoutExpired(command, options["timeout"])
                    self.create_assembly(command)
                    return subprocess.CompletedProcess(command, 0)

                self.assertEqual(self.invoke(run), 124)
                self.assertEqual(len(calls), stage)
                self.assertFalse(calls[0].exists())

    def test_missing_sdk_does_not_launch_commands(self):
        with (
            patch.object(RUNNER.shutil, "which", return_value=None),
            patch.object(RUNNER.subprocess, "run") as run,
            contextlib.redirect_stdout(io.StringIO()),
        ):
            self.assertEqual(RUNNER.core_checks(), 2)
            run.assert_not_called()

    def test_outside_or_linked_sandbox_is_rejected(self):
        work = RUNNER.SECURITY / ".work"
        for outside in (ROOT / "tests/security/unowned", work / "../escape"):
            with self.subTest(path=outside), self.assertRaises(ValueError):
                RUNNER.check_work_path(outside)
        with patch.object(Path, "is_symlink", autospec=True, side_effect=lambda path: path == work):
            with self.assertRaises(ValueError):
                RUNNER.check_work_path(work / "run-test")


if __name__ == "__main__":
    unittest.main()

"""기본 앱 보존과 설치 실패 정리를 격리 검증함."""

import importlib.util
import os
import re
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("ubuntu_smoke", Path(__file__).with_name("ubuntu_smoke.py"))
SMOKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SMOKE)


def test_work_root():
    job = os.environ.get("ZIPELO_CI_JOB", "rust-local")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]{0,63}", job):
        raise ValueError("안전한 작업명이 필요함")
    return SMOKE.work_root() / job / "tmp"


class LifecycleTests(unittest.TestCase):
    def setUp(self):
        work = test_work_root()
        work.mkdir(parents=True, exist_ok=True)
        temporary = tempfile.TemporaryDirectory(dir=work)
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        self.previous = self.directory / "mimeapps.list"
        self.previous.write_bytes(b"previous-app.desktop\n")
        self.created = self.directory / "added-mimeapps.list"
        self.original = {self.previous: self.previous.read_bytes()}
        settings = patch.object(SMOKE, "CONFIGS", (self.directory,))
        settings.start()
        self.addCleanup(settings.stop)

    def test_default_work_path_uses_common_job_directory(self):
        with patch.dict(os.environ, {"ZIPELO_CI_JOB": "lifecycle-regression"}):
            os.environ.pop("TMPDIR", None)
            os.environ.pop("ZIPELO_TEST_WORK_DIR", None)
            self.assertEqual(
                test_work_root(), SMOKE.work_root() / "lifecycle-regression/tmp"
            )

    def test_work_directory_rejects_nested_checkout_paths(self):
        allowed = SMOKE.work_root().resolve()
        self.assertEqual(SMOKE.selected_work_directory(None, allowed),
                         allowed / "ubuntu-package-smoke")
        self.assertEqual(SMOKE.selected_work_directory(allowed / "custom", allowed),
                         allowed / "custom")
        for requested in (allowed, allowed / "review/r/tests/work/nested",
                          allowed / "review/r/tests/work", allowed.parent / "outside"):
            with self.subTest(requested=requested), self.assertRaisesRegex(
                ValueError, "바로 아래"
            ):
                SMOKE.selected_work_directory(requested, allowed)

    def test_readme_commands_use_checkout_and_common_work_root(self):
        readme = (ROOT / "tests/release-packages/README.md").read_text()
        commands = readme.split("```sh\n", 1)[1].split("\n```", 1)[0]
        result = subprocess.run(
            ["bash", "-c", 'python3() { printf "<%s>\n" "$@"; }; ' + commands],
            cwd=ROOT / "tests/release-packages", check=True, capture_output=True, text=True,
        )
        output = result.stdout
        self.assertIn(str(ROOT / "tests/ci/run_rust.py"), output)
        self.assertIn(str(ROOT / "tests/release-packages/ubuntu_smoke.py"), output)
        self.assertIn(str(SMOKE.work_root() / "v021-build-out/zipelo_0.2.1_amd64.deb"),
                      output)
        self.assertIn(str(SMOKE.work_root() / "ubuntu-package-smoke"), output)

    def test_existing_installation_is_blocked_with_python_optimization(self):
        script = """import importlib.util
import subprocess
import sys
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("ubuntu_smoke", sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
with patch.object(module.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, b"installed")):
    try:
        module.ensure_no_existing_installation()
    except RuntimeError:
        print("blocked")
    else:
        raise SystemExit("기존 설치를 통과시킴")
"""
        environment = dict(os.environ, PYTHONDONTWRITEBYTECODE="1")
        result = subprocess.run(
            [sys.executable, "-O", "-c", script, str(SMOKE.ROOT / "tests/release-packages/ubuntu_smoke.py")],
            env=environment, cwd=ROOT, capture_output=True, text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), "blocked")

    def test_window_guards_stay_active_with_python_optimization(self):
        script = """import importlib.util
import subprocess
import sys
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location("ubuntu_smoke", sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
app = Mock(pid=123)
app.poll.return_value = 1
with patch.object(module.subprocess, "run") as search:
    try:
        module.wait_for_window(app, {}, timeout=0.5)
    except RuntimeError as error:
        if "조기에 종료" not in str(error):
            raise
    else:
        raise SystemExit("종료된 앱을 통과시킴")
    search.assert_not_called()
app.poll.return_value = None
with patch.object(module.subprocess, "run", return_value=subprocess.CompletedProcess([], 1)):
    with patch.object(module.time, "monotonic", side_effect=[0, 1]):
        with patch.object(module.time, "sleep") as sleep:
            try:
                module.wait_for_window(app, {}, timeout=0.5)
            except RuntimeError as error:
                if "시간 초과" not in str(error):
                    raise
            else:
                raise SystemExit("창 탐색 제한 시간을 통과시킴")
            sleep.assert_not_called()
print("guards active")
"""
        result = subprocess.run(
            [sys.executable, "-B", "-O", "-c", script, str(SMOKE.ROOT / "tests/release-packages/ubuntu_smoke.py")],
            env=dict(os.environ, PYTHONDONTWRITEBYTECODE="1"), cwd=ROOT,
            capture_output=True, text=True, timeout=10,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), "guards active")

    def test_existing_installation_files_are_blocked(self):
        with (
            patch.object(SMOKE.subprocess, "run", return_value=subprocess.CompletedProcess([], 1, b"")),
            patch.object(SMOKE.Path, "exists", return_value=True),
        ):
            with self.assertRaisesRegex(RuntimeError, "기존 파일"):
                SMOKE.ensure_no_existing_installation()

    def test_only_first_configuration_sets_defaults(self):
        commands = self.directory / "bin"
        commands.mkdir()
        log = self.directory / "calls"
        scripts = {
            "update-desktop-database": "#!/bin/sh\nexit 0\n",
            "gtk-update-icon-cache": "#!/bin/sh\nexit 0\n",
            "getent": '#!/bin/sh\nprintf "tester:x:1000:1000::%s:/bin/sh\\n" "$TEST_HOME"\n',
            "runuser": '#!/bin/sh\nprintf "%s\\n" "$*" >> "$TEST_LOG"\n',
        }
        for name, content in scripts.items():
            executable = commands / name
            executable.write_text(content)
            executable.chmod(0o755)
        env = {
            **os.environ,
            "PATH": str(commands) + os.pathsep + os.environ["PATH"],
            "SUDO_USER": "tester",
            "TEST_HOME": str(self.directory),
            "TEST_LOG": str(log),
        }
        for arguments, count in ((["configure"], 1), (["configure", "0.1.0"], 0),
                                 (["triggered", "desktop"], 0)):
            with self.subTest(arguments=arguments):
                log.write_text("")
                subprocess.run(["sh", str(ROOT / "packaging/ubuntu/rust/postinst"), *arguments],
                               env=env, check=True)
                self.assertEqual(len(log.read_text().splitlines()), count)
        env["SUDO_USER"] = "root"
        log.write_text("")
        subprocess.run(["sh", str(ROOT / "packaging/ubuntu/rust/postinst"), "configure"],
                       env=env, check=True)
        self.assertEqual(log.read_text(), "")

    def test_prerm_restores_only_on_remove_for_known_user(self):
        commands = self.directory / "bin"
        commands.mkdir()
        log = self.directory / "calls"
        for name, content in {
            "getent": '#!/bin/sh\nprintf "tester:x:1000:1000::%s:/bin/sh\\n" "$TEST_HOME"\n',
            "runuser": '#!/bin/sh\nprintf "%s\\n" "$*" >> "$TEST_LOG"\n',
        }.items():
            executable = commands / name
            executable.write_text(content)
            executable.chmod(0o755)
        env = {**os.environ, "PATH": str(commands) + os.pathsep + os.environ["PATH"],
               "SUDO_USER": "tester", "TEST_HOME": str(self.directory), "TEST_LOG": str(log)}
        for action, count in (("upgrade", 0), ("failed-upgrade", 0), ("remove", 1)):
            with self.subTest(action=action):
                log.write_text("")
                subprocess.run(["sh", str(ROOT / "packaging/ubuntu/rust/prerm"), action],
                               env=env, check=True, capture_output=True)
                calls = log.read_text().splitlines()
                self.assertEqual(len(calls), count)
                if count:
                    self.assertIn("HOME=" + str(self.directory), calls[0])
                    self.assertTrue(calls[0].endswith("/usr/lib/zipelo/default-apps remove"))
        env["SUDO_USER"] = "root"
        log.write_text("")
        subprocess.run(["sh", str(ROOT / "packaging/ubuntu/rust/prerm"), "remove"],
                       env=env, check=True, capture_output=True)
        self.assertEqual(log.read_text(), "")
        env.pop("SUDO_USER")
        log.write_text("")
        result = subprocess.run(["sh", str(ROOT / "packaging/ubuntu/rust/prerm"), "remove"],
                                env=env, check=True, capture_output=True, text=True)
        self.assertEqual(log.read_text(), "")
        self.assertIn("사용자를 확인할 수 없어", result.stdout)

    def check_smoke(self, install_fails=False, defaults_fail=False, purge_fails=False, body_fails=False):
        calls = []
        home = self.directory / "isolated-home"
        home.mkdir()
        isolated_config = home / ".config/mimeapps.list"
        isolated_config.parent.mkdir()
        isolated_config.write_bytes(b"previous-app.desktop\n")

        def command(arguments, **options):
            calls.append(arguments)
            if arguments[0] == "sudo":
                self.assertIn("SUDO_USER=root", arguments)
                self.assertIn("HOME=" + str(home), arguments)
                if "-i" in arguments and install_fails:
                    raise subprocess.CalledProcessError(1, arguments)
                if "-P" in arguments and purge_fails:
                    raise subprocess.CalledProcessError(2, arguments)
            else:
                self.assertEqual(arguments[0], "/usr/lib/zipelo/default-apps")
                self.assertEqual(options["env"]["HOME"], str(home))
                self.assertEqual(options["env"]["XDG_CONFIG_HOME"], str(home / ".config"))
                self.assertEqual(options["env"]["XDG_DATA_HOME"], str(home / ".local/share"))
                self.assertEqual(options["env"]["XDG_STATE_HOME"], str(home / ".local/state"))
                if arguments[1] == "install":
                    isolated_config.write_bytes(b"zipelo.desktop\n")
                    if defaults_fail:
                        raise subprocess.CalledProcessError(3, arguments)
                else:
                    isolated_config.write_bytes(b"previous-app.desktop\n")
            return subprocess.CompletedProcess(arguments, 0)

        def exercise():
            with patch.object(SMOKE.subprocess, "run", side_effect=command):
                with SMOKE.installed_package(self.directory / "sample.deb", home) as isolated:
                    self.assertEqual(isolated["HOME"], str(home))
                    if body_fails:
                        raise RuntimeError("창 실행 실패")

        if install_fails or defaults_fail or purge_fails or body_fails:
            with self.assertRaises(BaseExceptionGroup) as raised:
                exercise()
            self.assertEqual(len(raised.exception.exceptions),
                             int(install_fails) + int(defaults_fail) + int(purge_fails) + int(body_fails))
        else:
            exercise()
        self.assertEqual(SMOKE.configs(), self.original)
        self.assertEqual(self.previous.read_bytes(), b"previous-app.desktop\n")
        self.assertFalse(self.created.exists())
        if install_fails:
            self.assertEqual([arguments[-2:] for arguments in calls],
                             [["-i", str(self.directory / "sample.deb")], ["-P", "zipelo"]])
        else:
            self.assertEqual([arguments[-2:] for arguments in calls if arguments[0] == "sudo"],
                             [["-i", str(self.directory / "sample.deb")], ["-P", "zipelo"]])
            self.assertEqual([arguments[-1] for arguments in calls if arguments[0] != "sudo"],
                             ["install", "remove"])
            self.assertEqual(isolated_config.read_bytes(), b"previous-app.desktop\n")

    def test_success_keeps_real_user_settings_unchanged(self):
        self.check_smoke()

    def test_partial_install_failure_keeps_real_user_settings_unchanged(self):
        self.check_smoke(install_fails=True)

    def test_default_app_failure_still_cleans_isolated_settings(self):
        self.check_smoke(defaults_fail=True)

    def test_purge_failure_keeps_real_user_settings_unchanged(self):
        self.check_smoke(purge_fails=True)

    def test_install_and_purge_failures_are_reported(self):
        self.check_smoke(install_fails=True, purge_fails=True)

    def test_window_failure_keeps_real_user_settings_unchanged(self):
        self.check_smoke(body_fails=True)

    def test_host_change_is_detected_without_overwrite(self):
        home = self.directory / "isolated-home"
        home.mkdir()
        calls = []

        def command(arguments, **options):
            calls.append(arguments)
            if "-i" in arguments:
                self.previous.write_bytes(b"external-choice.desktop\n")
            return subprocess.CompletedProcess(arguments, 0)

        with patch.object(SMOKE.subprocess, "run", side_effect=command):
            with self.assertRaises(BaseExceptionGroup) as raised:
                with SMOKE.installed_package(self.directory / "sample.deb", home):
                    pass
        self.assertTrue(any("실제 사용자 MIME 설정" in str(error)
                            for error in raised.exception.exceptions))
        self.assertEqual(self.previous.read_bytes(), b"external-choice.desktop\n")
        self.assertEqual(len(calls), 2)


if __name__ == "__main__":
    unittest.main()

"""기본 앱 보존과 설치 실패 정리를 격리 검증함."""

import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("ubuntu_smoke", Path(__file__).with_name("ubuntu_smoke.py"))
SMOKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SMOKE)


class LifecycleTests(unittest.TestCase):
    def setUp(self):
        (ROOT / "tests/release-packages/runs").mkdir(parents=True, exist_ok=True)
        temporary = tempfile.TemporaryDirectory(dir=ROOT / "tests/release-packages/runs")
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        self.previous = self.directory / "mimeapps.list"
        self.previous.write_bytes(b"previous-app.desktop\n")
        self.created = self.directory / "added-mimeapps.list"
        self.original = {self.previous: self.previous.read_bytes()}
        settings = patch.object(SMOKE, "CONFIGS", (self.directory,))
        settings.start()
        self.addCleanup(settings.stop)

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
        env.pop("SUDO_USER")
        log.write_text("")
        result = subprocess.run(["sh", str(ROOT / "packaging/ubuntu/rust/prerm"), "remove"],
                                env=env, check=True, capture_output=True, text=True)
        self.assertEqual(log.read_text(), "")
        self.assertIn("사용자를 확인할 수 없어", result.stdout)

    def check_smoke(self, install_fails=False, purge_fails=False, body_fails=False):
        calls = []

        def command(arguments, **options):
            calls.append(arguments)
            if "-i" in arguments:
                self.previous.write_bytes(b"zipelo.desktop\n")
                self.created.write_bytes(b"zipelo.desktop\n")
                if install_fails:
                    raise subprocess.CalledProcessError(1, arguments)
            elif purge_fails:
                raise subprocess.CalledProcessError(2, arguments)
            return subprocess.CompletedProcess(arguments, 0)

        def exercise():
            with patch.object(SMOKE.subprocess, "run", side_effect=command):
                with SMOKE.installed_package(self.directory / "sample.deb"):
                    self.assertFalse(install_fails)
                    if body_fails:
                        raise RuntimeError("창 실행 실패")

        if install_fails or purge_fails or body_fails:
            with self.assertRaises(ExceptionGroup) as raised:
                exercise()
            self.assertEqual(len(raised.exception.exceptions),
                             int(install_fails) + int(purge_fails) + int(body_fails))
        else:
            exercise()
        self.assertEqual(SMOKE.configs(), self.original)
        self.assertEqual(len(calls), 2)
        self.assertIn("-P", calls[1])

    def test_success_restores_settings(self):
        self.check_smoke()

    def test_partial_install_failure_restores_settings(self):
        self.check_smoke(install_fails=True)

    def test_purge_failure_still_restores_settings(self):
        self.check_smoke(purge_fails=True)

    def test_both_failures_are_reported(self):
        self.check_smoke(install_fails=True, purge_fails=True)

    def test_window_failure_restores_settings(self):
        self.check_smoke(body_fails=True)

    def test_external_change_does_not_skip_other_restores(self):
        self.previous.write_bytes(b"zipelo.desktop\n")
        self.created.write_bytes(b"zipelo.desktop\n")
        after = SMOKE.configs()
        self.created.write_bytes(b"external-choice.desktop\n")
        with self.assertRaises(ExceptionGroup):
            SMOKE.restore_configs(self.original, after)
        self.assertEqual(self.previous.read_bytes(), self.original[self.previous])
        self.assertEqual(self.created.read_bytes(), b"external-choice.desktop\n")


if __name__ == "__main__":
    unittest.main()

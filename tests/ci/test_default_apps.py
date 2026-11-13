"""제거 시 기존 기본 앱과 사용자의 새 선택 보존을 검증함."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
HELPER = ROOT / "packaging/ubuntu/rust/default-apps"


class DefaultAppsTests(unittest.TestCase):
    def setUp(self):
        work = ROOT / "tests/work"
        work.mkdir(exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(dir=work)
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.data = self.root / "data/applications"
        self.data.mkdir(parents=True)
        for name in ("before.desktop", "changed.desktop"):
            (self.data / name).write_text("[Desktop Entry]\nType=Application\nName=Archive\nExec=true %f\n")
        self.db = self.root / "choices.json"
        self.db.write_text(json.dumps({"application/zip": "before.desktop"}))
        executable = self.bin / "xdg-mime"
        executable.write_text("""#!/usr/bin/python3
import json, os, sys
from pathlib import Path
p = Path(os.environ["CHOICES"])
d = json.loads(p.read_text())
if sys.argv[1] == "query":
    print(d.get(sys.argv[3], ""))
elif sys.argv[1] == "default":
    if os.environ.get("FAIL_RESTORE") and sys.argv[2] != "zipelo.desktop":
        sys.exit(1)
    d[sys.argv[3]] = sys.argv[2]
    p.write_text(json.dumps(d))
""")
        executable.chmod(0o755)
        self.env = dict(os.environ, HOME=str(self.root),
                        XDG_STATE_HOME=str(self.root / "state"),
                        XDG_DATA_HOME=str(self.root / "data"),
                        XDG_DATA_DIRS=str(self.root / "empty"),
                        CHOICES=str(self.db), PATH=str(self.bin) + ":" + os.environ["PATH"])
        self.state = self.root / "state/zipelo-default-apps-v1"

    def run_helper(self, action):
        return subprocess.run(["sh", str(HELPER), action], env=self.env, check=True,
                              capture_output=True, text=True)

    def choice(self):
        return json.loads(self.db.read_text())["application/zip"]

    def test_previous_app_is_restored(self):
        self.run_helper("install")
        self.assertEqual(self.choice(), "zipelo.desktop")
        self.run_helper("remove")
        self.assertEqual(self.choice(), "before.desktop")
        self.assertFalse(self.state.exists())

    def test_nodisplay_previous_app_is_restored(self):
        with (self.data / "before.desktop").open("a") as desktop:
            desktop.write("NoDisplay=true\n")
        self.run_helper("install")
        self.run_helper("remove")
        self.assertEqual(self.choice(), "before.desktop")
        self.assertFalse(self.state.exists())

    def test_nodisplay_failed_restore_keeps_record_for_retry(self):
        with (self.data / "before.desktop").open("a") as desktop:
            desktop.write("NoDisplay=true\n")
        self.run_helper("install")
        self.env["FAIL_RESTORE"] = "1"
        self.run_helper("remove")
        self.assertEqual((self.state / "zip").read_text(), "before.desktop\n")
        self.assertEqual(self.choice(), "zipelo.desktop")
        del self.env["FAIL_RESTORE"]
        self.run_helper("remove")
        self.assertEqual(self.choice(), "before.desktop")
        self.assertFalse(self.state.exists())

    def test_nodisplay_restore_with_real_xdg_mime(self):
        (self.bin / "xdg-mime").unlink()
        (self.data / "before.desktop").write_text(
            "[Desktop Entry]\nType=Application\nName=Archive\n"
            "Exec=/usr/bin/true %f\nNoDisplay=true\nMimeType=application/zip;\n"
        )
        (self.data / "zipelo.desktop").write_text(
            "[Desktop Entry]\nType=Application\nName=Zipelo\n"
            "Exec=/usr/bin/true %f\nMimeType=application/zip;\n"
        )
        (self.root / "config").mkdir()
        self.env.update(
            XDG_CONFIG_HOME=str(self.root / "config"),
            XDG_CONFIG_DIRS=str(self.root / "system-config"),
            XDG_CURRENT_DESKTOP="X-Generic",
            DE="generic",
        )
        for key in ("DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS"):
            self.env.pop(key, None)
        subprocess.run(
            ["xdg-mime", "default", "before.desktop", "application/zip"],
            env=self.env, check=True,
        )

        def selected():
            return subprocess.check_output(
                ["xdg-mime", "query", "default", "application/zip"],
                env=self.env, text=True,
            ).strip()

        self.assertEqual(selected(), "before.desktop")
        self.run_helper("install")
        self.assertEqual(selected(), "zipelo.desktop")
        self.run_helper("remove")
        self.assertEqual(selected(), "before.desktop")
        self.assertFalse(self.state.exists())

    def test_changed_choice_is_preserved(self):
        self.run_helper("install")
        self.db.write_text(json.dumps({"application/zip": "changed.desktop"}))
        self.run_helper("remove")
        self.assertEqual(self.choice(), "changed.desktop")

    def test_missing_previous_app_has_guidance(self):
        self.run_helper("install")
        (self.data / "before.desktop").unlink()
        result = self.run_helper("remove")
        self.assertEqual(self.choice(), "zipelo.desktop")
        self.assertIn("다른 프로그램으로 열기", result.stderr)
        self.assertEqual((self.state / "zip").read_text(), "before.desktop\n")

    def assert_unavailable_keeps_record(self):
        result = self.run_helper("remove")
        self.assertEqual(self.choice(), "zipelo.desktop")
        self.assertEqual((self.state / "zip").read_text(), "before.desktop\n")
        self.assertIn("다른 프로그램으로 열기", result.stderr)

    def test_hidden_with_whitespace_keeps_record(self):
        self.run_helper("install")
        with (self.data / "before.desktop").open("a") as desktop:
            desktop.write("Hidden = true\n")
        self.assert_unavailable_keeps_record()

    def test_deleted_tryexec_keeps_record(self):
        executable = self.bin / "previous-app"
        executable.write_text("#!/bin/sh\nexit 0\n")
        executable.chmod(0o755)
        with (self.data / "before.desktop").open("a") as desktop:
            desktop.write(f"TryExec = {executable}\n")
        self.run_helper("install")
        executable.unlink()
        self.assert_unavailable_keeps_record()

    def test_tryexec_requires_executable_file_and_keeps_record_until_fixed(self):
        executable = self.bin / "previous-app"
        executable.write_text("#!/bin/sh\nexit 0\n")
        with (self.data / "before.desktop").open("a") as desktop:
            desktop.write("TryExec=previous-app\n")
        self.run_helper("install")
        self.assert_unavailable_keeps_record()
        executable.chmod(0o755)
        self.run_helper("remove")
        self.assertEqual(self.choice(), "before.desktop")
        self.assertFalse((self.state / "zip").exists())

    def test_tryexec_absolute_escaped_space_is_available(self):
        executable = self.bin / "previous app"
        executable.write_text("#!/bin/sh\nexit 0\n")
        executable.chmod(0o755)
        value = str(executable).replace(" ", r"\s")
        with (self.data / "before.desktop").open("a") as desktop:
            desktop.write(f"TryExec={value}\n")
        self.run_helper("install")
        self.run_helper("remove")
        self.assertEqual(self.choice(), "before.desktop")

    def test_action_group_does_not_override_application_availability(self):
        with (self.data / "before.desktop").open("a") as desktop:
            desktop.write("[Desktop Action Other]\nHidden=true\nTryExec=missing-app\n")
        self.run_helper("install")
        self.run_helper("remove")
        self.assertEqual(self.choice(), "before.desktop")

    def test_missing_record_does_not_change_defaults(self):
        before = self.db.read_bytes()
        self.run_helper("remove")
        self.assertEqual(self.db.read_bytes(), before)

    def test_repeated_install_does_not_replace_first_record(self):
        self.run_helper("install")
        original = (self.state / "zip").read_bytes()
        self.run_helper("install")
        self.assertEqual((self.state / "zip").read_bytes(), original)
        self.run_helper("remove")
        self.assertEqual(self.choice(), "before.desktop")

    def test_symlinked_state_is_untouched(self):
        self.state.parent.mkdir()
        self.state.symlink_to(self.data, target_is_directory=True)
        before = self.db.read_bytes()
        self.run_helper("install")
        self.assertEqual(self.db.read_bytes(), before)
        self.assertEqual(len(list(self.data.iterdir())), 2)

    def test_failed_restore_retains_record_for_retry(self):
        self.run_helper("install")
        self.env["FAIL_RESTORE"] = "1"
        self.run_helper("remove")
        self.assertTrue((self.state / "zip").exists())
        self.assertEqual(self.choice(), "zipelo.desktop")
        del self.env["FAIL_RESTORE"]
        self.run_helper("remove")
        self.assertEqual(self.choice(), "before.desktop")

    def test_hidden_user_entry_does_not_fall_through_to_system_copy(self):
        self.run_helper("install")
        (self.data / "before.desktop").write_text("[Desktop Entry]\nHidden=true\n")
        system = self.root / "system/applications"
        system.mkdir(parents=True)
        (system / "before.desktop").write_text("[Desktop Entry]\nType=Application\n")
        self.env["XDG_DATA_DIRS"] = str(system.parent)
        self.assert_unavailable_keeps_record()

    def test_hidden_previous_app_is_not_restored(self):
        self.run_helper("install")
        (self.data / "before.desktop").write_text("[Desktop Entry]\nHidden=true\n")
        self.assert_unavailable_keeps_record()


if __name__ == "__main__":
    unittest.main()

"""MUI 중단 hook과 생성 스크립트의 정리 경계를 검사함."""

import importlib.util
from pathlib import Path
import re
import tempfile
import unittest

PROJECT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("installer_prepare", PROJECT / "packaging/prepare.py")
PREPARE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PREPARE)
TEMPLATE = (PROJECT / "packaging/windows/installer.nsi.in").read_text()


def function(source, name):
    match = re.search(r"^Function " + re.escape(name) + r"\n(.*?)^FunctionEnd$",
                      source, re.MULTILINE | re.DOTALL)
    if match is None:
        raise AssertionError("필수 함수 누락: " + name)
    return match.group(1)


class AbortHookTests(unittest.TestCase):
    def check_hook(self, source):
        hook = "!define MUI_CUSTOMFUNCTION_ABORT ZipeloUserAbort"
        self.assertEqual(source.count(hook), 1)
        self.assertLess(source.index(hook), source.index("!insertmacro MUI_PAGE_WELCOME"))
        self.assertNotRegex(source, r"(?m)^Function \.onUserAbort\s*$")
        self.assertEqual(source.count("Function ZipeloUserAbort\n"), 1)
        self.assertIn("Call Rollback", function(source, "ZipeloUserAbort"))

    def test_template_uses_mui_hook_before_pages(self):
        self.check_hook(TEMPLATE)

    def test_renderer_without_and_with_zip_registration(self):
        for formats in [[], [{"extension": ".zip"}]]:
            with self.subTest(formats=formats):
                with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as temporary:
                    output = Path(temporary)
                    PREPARE.windows(output, {"version": "0.1.0", "formats": formats},
                                    ["synthetic.txt"])
                    source = (output / "installer.nsi").read_text(encoding="utf-8-sig")
                    self.check_hook(source)
                    self.assertNotRegex(source, r"@[A-Z_]+@")
                    self.assertIn('File "payload\\synthetic.txt"', source)
                    self.assertNotRegex(source, r"(?im)^\s*RMDir\s+/r\b")
                    self.assertNotIn("UserChoice", source)

    def test_failure_and_abort_share_guarded_rollback(self):
        abort = function(TEMPLATE, "ZipeloUserAbort")
        failure = function(TEMPLATE, ".onInstFailed")
        for body in [abort, failure]:
            self.assertEqual(body.count("Call Rollback"), 1)
            self.assertIn("SetErrorLevel 1", body)
        self.assertIn('StrCpy $FailureStep "사용자 취소"', abort)
        rollback = function(TEMPLATE, "Rollback")
        guard_end = rollback.index("${EndIf}")
        self.assertIn("$InstallComplete == 1", rollback[:guard_end])
        self.assertIn("$CleanupDone == 1", rollback[:guard_end])
        self.assertIn("Return", rollback[:guard_end])
        self.assertLess(rollback.index("StrCpy $CleanupDone 1"),
                        rollback.index("@ROLLBACK_REGISTER@"))

    def test_preinstall_and_completed_paths_keep_ownership_gates(self):
        rollback = function(TEMPLATE, "Rollback")
        self.assertIn("${If} $MenuOwned == 1", rollback)
        self.assertIn("${If} $RootOwned == 1", rollback)
        self.assertIn("${If} $${FLAG} == 1", TEMPLATE)
        failed = TEMPLATE.split("install_failed:\n", 1)[1].split("install_done:", 1)[0]
        self.assertEqual(failed.count("Call Rollback"), 1)
        self.assertLess(failed.index("Call Rollback"), failed.index("Abort"))
        self.assertIn("StrCpy $InstallComplete 1\n    Goto install_done", TEMPLATE)
        self.assertNotIn("Call Rollback", function(TEMPLATE, "FinishCreate"))
        self.assertNotIn("Call Rollback", function(TEMPLATE, "OpenSettings"))


if __name__ == "__main__":
    unittest.main()

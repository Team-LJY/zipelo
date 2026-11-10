import json
import os
import pathlib
import subprocess
import sys
import unittest

BASE = pathlib.Path(__file__).resolve().parent
CHECKER = BASE / "static_check.py"
GUARD_MESSAGE = "최적화 모드에서는 정적 검사를 실행할 수 없음"
PROBE = """
import contextlib
import json
import pathlib
import runpy
import sys
from unittest import mock

namespace = runpy.run_path(sys.argv[1])
main = namespace["main"]
calls = []

def forbidden(name):
    def fail(*args, **kwargs):
        calls.append(name)
        raise RuntimeError("reached:" + name)
    return fail

with contextlib.ExitStack() as stack:
    stack.enter_context(mock.patch(
        "argparse.ArgumentParser", side_effect=forbidden("argument_parser")
    ))
    stack.enter_context(mock.patch.object(
        pathlib.Path, "read_bytes", side_effect=forbidden("read_bytes")
    ))
    stack.enter_context(mock.patch.object(
        pathlib.Path, "read_text", side_effect=forbidden("read_text")
    ))
    stack.enter_context(mock.patch.object(
        runpy, "run_path", side_effect=forbidden("fixture_generator")
    ))
    stack.enter_context(mock.patch.dict(main.__globals__, {
        "verify_manifest": forbidden("verify_manifest"),
        "verify_pin": forbidden("verify_pin"),
    }))
    try:
        main()
    except RuntimeError as error:
        result = {
            "optimize": sys.flags.optimize,
            "message": str(error),
            "calls": calls,
        }
    else:
        raise RuntimeError("조기 거부가 발생하지 않음")
print(json.dumps(result, ensure_ascii=False))
"""


class OptimizationGuardTests(unittest.TestCase):
    def run_child(self, mode, arguments):
        environment = os.environ.copy()
        environment.pop("PYTHONOPTIMIZE", None)
        environment["PYTHONDONTWRITEBYTECODE"] = "1"
        command = [sys.executable, "-B"]
        if mode == "flag":
            command.append("-O")
        elif mode == "environment":
            environment["PYTHONOPTIMIZE"] = "1"
        command.extend(arguments)
        return subprocess.run(
            command,
            cwd=BASE,
            env=environment,
            capture_output=True,
            text=True,
            timeout=10,
            check=False,
        )

    def check_probe(self, mode, expected_calls, expected_message, optimize):
        result = self.run_child(mode, ["-c", PROBE, str(CHECKER)])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")
        self.assertEqual(json.loads(result.stdout), {
            "optimize": optimize,
            "message": expected_message,
            "calls": expected_calls,
        })

    def check_cli(self, mode):
        result = self.run_child(mode, [str(CHECKER)])
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn("RuntimeError: " + GUARD_MESSAGE, result.stderr)
        self.assertNotIn("the following arguments are required", result.stderr)

    def test_flag_rejected_before_validation_or_fixture_generation(self):
        self.check_probe("flag", [], GUARD_MESSAGE, 1)

    def test_environment_rejected_before_validation_or_fixture_generation(self):
        self.check_probe("environment", [], GUARD_MESSAGE, 1)

    def test_normal_mode_reaches_argument_parser(self):
        self.check_probe("normal", ["argument_parser"], "reached:argument_parser", 0)

    def test_flag_cli_rejected_before_required_arguments(self):
        self.check_cli("flag")

    def test_environment_cli_rejected_before_required_arguments(self):
        self.check_cli("environment")


if __name__ == "__main__":
    unittest.main(verbosity=2)

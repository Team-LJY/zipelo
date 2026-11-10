import json
import os
import pathlib
import runpy
import subprocess
import sys
import unittest

BASE = pathlib.Path(__file__).resolve().parent
CHECKER = BASE / "static_check.py"
CODE = """
import json, pathlib, runpy, sys
from unittest.mock import patch
scope = runpy.run_path(sys.argv[1])
calls = []
def blocked(*args, **kwargs):
    calls.append("side_effect")
    raise RuntimeError("side_effect")
with patch("argparse.ArgumentParser", blocked), \
     patch.object(pathlib.Path, "read_bytes", blocked), \
     patch.object(pathlib.Path, "read_text", blocked), \
     patch.object(runpy, "run_path", blocked):
    try:
        scope["main"]()
    except RuntimeError as error:
        print(json.dumps({"message": str(error), "calls": calls, "optimize": sys.flags.optimize}))
"""


class StaticHelperTests(unittest.TestCase):
    def child(self, environment_mode=False, flag=False, cli=False):
        environment = os.environ.copy()
        environment.pop("PYTHONOPTIMIZE", None)
        environment["PYTHONDONTWRITEBYTECODE"] = "1"
        if environment_mode:
            environment["PYTHONOPTIMIZE"] = "1"
        command = [sys.executable, "-B"] + (["-O"] if flag else [])
        command += [str(CHECKER)] if cli else ["-c", CODE, str(CHECKER)]
        return subprocess.run(command, env=environment, cwd=BASE,
                              capture_output=True, text=True, timeout=10)

    def probe(self, **mode):
        result = self.child(**mode)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")
        self.assertEqual(json.loads(result.stdout), {
            "message": "최적화 모드에서는 정적 검사를 실행할 수 없음",
            "calls": [], "optimize": 1,
        })

    def test_flag_stops_before_reads_arguments_and_generation(self):
        self.probe(flag=True)

    def test_environment_stops_before_reads_arguments_and_generation(self):
        self.probe(environment_mode=True)

    def test_normal_mode_reaches_parser(self):
        result = self.child()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), {
            "message": "side_effect", "calls": ["side_effect"], "optimize": 0,
        })

    def test_flag_cli_fails_before_required_arguments(self):
        result = self.child(flag=True, cli=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn("RuntimeError: 최적화 모드", result.stderr)
        self.assertNotIn("arguments are required", result.stderr)

    def test_environment_cli_fails_before_required_arguments(self):
        result = self.child(environment_mode=True, cli=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn("RuntimeError: 최적화 모드", result.stderr)

    def reject(self, content, paths):
        parse = runpy.run_path(str(CHECKER))["parse_manifest"]
        with self.assertRaises(RuntimeError):
            parse(content, paths)

    def test_duplicate_manifest_rows_rejected(self):
        line = "a" * 64 + "  main.rs\n"
        self.reject(line + line, {"main.rs"})

    def test_missing_manifest_path_rejected(self):
        self.reject("a" * 64 + "  main.rs\n", {"main.rs", "support.rs"})

    def test_parent_path_rejected(self):
        self.reject("a" * 64 + "  ../main.rs\n", {"../main.rs"})

    def test_repository_relative_source_manifest_path(self):
        parse = runpy.run_path(str(CHECKER))["parse_manifest"]
        name = "experiments/rust-file-publication/src/lib.rs"
        self.assertEqual(parse("a" * 64 + "  " + name + "\n", {name}), {name: "a" * 64})

    def test_malformed_digest_rejected(self):
        self.reject("g" * 64 + "  main.rs\n", {"main.rs"})


if __name__ == "__main__":
    unittest.main(verbosity=2)

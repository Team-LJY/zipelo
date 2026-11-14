from pathlib import Path
import subprocess
import sys
import unittest


class ZipRecoveryFixtureTests(unittest.TestCase):
    def test_corpus_and_recovered_output_checks(self):
        verifier = Path(__file__).resolve().parents[1] / "zip-recovery-fixtures" / "verify.py"
        result = subprocess.run(
            [sys.executable, "-B", str(verifier)],
            capture_output=True,
            text=True,
            timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()

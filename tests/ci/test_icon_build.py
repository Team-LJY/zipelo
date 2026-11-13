import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class IconBuildTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        (ROOT / "tests/work").mkdir(parents=True, exist_ok=True)
        cls.scratch = tempfile.TemporaryDirectory(dir=ROOT / "tests/work")
        cls.addClassCleanup(cls.scratch.cleanup)
        cls.root = Path(cls.scratch.name)
        cls.builder = cls.root / "build.exe"
        cls.compiler = cls.root / "fake-rc.exe"
        fake = cls.root / "fake.rs"
        fake.write_text('''fn main() {
    if std::env::var_os("FAKE_RC_FAIL").is_some() {
        std::process::exit(1);
    }
    let output = std::env::args().find_map(|arg| arg.strip_prefix("/fo").map(str::to_owned)).unwrap();
    std::fs::write(output, std::env::current_exe().unwrap().to_string_lossy().as_bytes()).unwrap();
}
''')
        for source, output in ((ROOT / "experiments/rust-ui-memory-app/build.rs", cls.builder),
                               (fake, cls.compiler)):
            subprocess.run(["rustc", "+1.99.1", "--edition=2024", str(source), "-o", str(output)],
                           check=True, capture_output=True, text=True, encoding="utf-8")

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(dir=self.root)
        self.addCleanup(self.directory.cleanup)
        self.work = Path(self.directory.name)
        self.output = self.work / "out"
        self.output.mkdir()
        self.programs = self.work / "Program Files (x86)"

    def sdk(self, version):
        path = self.programs / "Windows Kits/10/bin" / version / "x64/rc.exe"
        path.parent.mkdir(parents=True)
        shutil.copy2(self.compiler, path)
        return path

    def run_build(self, target="msvc", **overrides):
        excluded = {key.casefold() for key in ("RC", "ProgramFiles", "ProgramFiles(x86)", "FAKE_RC_FAIL", "PATH")}
        environment = {key: value for key, value in os.environ.items() if key.casefold() not in excluded}
        environment.update({"CARGO_CFG_TARGET_OS": "windows", "CARGO_CFG_TARGET_ENV": target,
                            "CARGO_FEATURE_UI": "1", "OUT_DIR": str(self.output),
                            "CARGO_MANIFEST_DIR": str(ROOT / "experiments/rust-ui-memory-app"),
                            "ProgramFiles(x86)": str(self.programs), "PATH": ""})
        environment.update(overrides)
        return subprocess.run([str(self.builder)], env=environment, capture_output=True, text=True, encoding="utf-8")

    def test_sdk_uses_highest_numeric_version_with_x64_compiler(self):
        self.sdk("10.0.9.0")
        latest = self.sdk("10.0.10.0")
        (self.programs / "Windows Kits/10/bin/10.0.99.0/x64").mkdir(parents=True)
        result = self.run_build()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("cargo:rustc-link-arg-bin=", result.stdout)
        self.assertTrue(Path((self.output / "zipelo.res").read_text()).samefile(latest))

    def test_explicit_rc_overrides_sdk(self):
        self.sdk("10.0.99.0")
        result = self.run_build(RC=str(self.compiler))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(Path((self.output / "zipelo.res").read_text()).samefile(self.compiler))

    def test_path_compiler_remains_available_without_sdk(self):
        path = self.work / "bin/rc.exe"
        path.parent.mkdir()
        shutil.copy2(self.compiler, path)
        result = self.run_build(PATH=str(path.parent))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(Path((self.output / "zipelo.res").read_text()).samefile(path))

    def test_missing_msvc_compiler_warns_without_linking(self):
        result = self.run_build()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("cargo:warning=아이콘 없이 빌드", result.stdout)
        self.assertNotIn("cargo:rustc-link-arg-bin=", result.stdout)
        self.assertFalse((self.output / "zipelo.res").exists())

    def test_msvc_compiler_failure_warns_without_linking(self):
        result = self.run_build(RC=str(self.compiler), FAKE_RC_FAIL="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("cargo:warning=아이콘 없이 빌드", result.stdout)
        self.assertNotIn("cargo:rustc-link-arg-bin=", result.stdout)

    def test_invalid_explicit_rc_does_not_fall_back_to_sdk(self):
        self.sdk("10.0.99.0")
        result = self.run_build(RC=str(self.work / "missing.exe"))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("cargo:warning=아이콘 없이 빌드", result.stdout)
        self.assertNotIn("cargo:rustc-link-arg-bin=", result.stdout)

    def test_missing_gnu_compiler_remains_a_build_failure(self):
        result = self.run_build(target="gnu", RC=str(self.work / "missing.exe"))
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("cargo:warning=아이콘 없이 빌드", result.stdout)

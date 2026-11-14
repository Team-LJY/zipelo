import argparse
import importlib.util
from pathlib import Path
import struct
import re
import shutil
import subprocess
import tempfile
import sys
from unittest.mock import patch
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("windows_installer", ROOT / "packaging/windows/rust/build.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class InstallerInputTests(unittest.TestCase):
    def setUp(self):
        runs = ROOT / "tests/windows-installer/runs"
        runs.mkdir(parents=True, exist_ok=True)
        self.directory = tempfile.TemporaryDirectory(dir=runs)
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)

    def test_truncated_and_non_pe_payloads_are_rejected(self):
        for content in (b"", b"MZ", b"not an executable" * 8,
                        b"MZ" + bytes(58) + struct.pack("<I", 65535)):
            payload = self.root / "invalid.exe"
            payload.write_bytes(content)
            with self.assertRaises(ValueError):
                MODULE.pe_machine(payload)

    def test_x64_coff_machine_is_read_from_pe_offset(self):
        payload = self.root / "header.exe"
        payload.write_bytes(b"MZ" + bytes(58) + struct.pack("<I", 128)
                            + bytes(64) + b"PE\0\0" + struct.pack("<H", 0x8664))
        self.assertEqual(MODULE.pe_machine(payload), 0x8664)
        payload.write_bytes(payload.read_bytes()[:128] + b"badPE!")
        with self.assertRaises(ValueError):
            MODULE.pe_machine(payload)

    def test_existing_output_is_preserved_before_any_build(self):
        output = self.root / "output"
        output.mkdir()
        sentinel = output / "user-data"
        sentinel.write_bytes(b"preserve")
        args = argparse.Namespace(output=output, target_dir=self.root / "target")
        with self.assertRaises(FileExistsError):
            MODULE.build(args)
        self.assertEqual(sentinel.read_bytes(), b"preserve")
        self.assertEqual(list(output.iterdir()), [sentinel])
        self.assertFalse(args.target_dir.exists())

    @unittest.skipUnless(shutil.which("makensis"), "NSIS가 설치되지 않음")
    def test_nsis_compiles_payloads_and_finish_callbacks(self):
        payload = self.root / "payload"
        payload.mkdir()
        for name in ("Zipelo.exe", "zipelo-archive-worker.exe", "LICENSE", "BUILD-INFO.json", "THIRD-PARTY-LICENSES.txt"):
            (payload / name).write_bytes(b"compile-only test fixture")
        shutil.copyfile(ROOT / "packaging/windows/rust/installer.nsi", self.root / "installer.nsi")
        shutil.copyfile(ROOT / "experiments/rust-ui-memory-app/src/assets/icons/zipelo.ico", self.root / "zipelo.ico")
        completed = subprocess.run(["makensis", "-WX", "installer.nsi"], cwd=self.root,
                                   stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        self.assertEqual(completed.returncode, 0, completed.stdout)
        installer = self.root / "Zipelo-0.2.0-windows-x64-setup.exe"
        self.assertIn(MODULE.pe_machine(installer), (0x14C, 0x8664))
        self.assertGreater(installer.stat().st_size, 100000)

    def test_command_line_directory_is_reset_before_install_side_effects(self):
        source = (ROOT / "packaging/windows/rust/installer.nsi").read_text()
        section = source.split('Section "Zipelo"', 1)[1].split("SectionEnd", 1)[0]
        commands = [line.strip() for line in section.splitlines()
                    if line.strip() and not line.strip().startswith(";")]
        self.assertEqual(commands[0], 'StrCpy $INSTDIR "${INSTALLROOT}"')
        root = re.search(r'^!define INSTALLROOT "([^"\n]+)"$', source, re.MULTILINE)
        self.assertIsNotNone(root)
        self.assertEqual(root.group(1), "$LOCALAPPDATA\\Programs\\Zipelo")
        self.assertIn('InstallDir "${INSTALLROOT}"', source)
        uninstaller = source.split("Function un.onInit", 1)[1].split("FunctionEnd", 1)[0]
        self.assertIn('StrCmp $INSTDIR "${INSTALLROOT}" 0 invalid', uninstaller)

    def test_source_commit_change_cannot_reuse_an_old_binary(self):
        repo = self.root / "source"
        repo.mkdir()
        manifest = repo / "Cargo.toml"
        manifest.write_text("fixture")
        real_run = subprocess.run
        environment = dict(MODULE.os.environ, GIT_AUTHOR_NAME="Zipelo test",
                           GIT_AUTHOR_EMAIL="tests@example.invalid", GIT_COMMITTER_NAME="Zipelo test",
                           GIT_COMMITTER_EMAIL="tests@example.invalid")

        def git(*args):
            real_run(["git", *args], cwd=repo, env=environment, check=True,
                     stdout=subprocess.PIPE, stderr=subprocess.PIPE)

        git("init", "-b", "features/installer-fixture")
        git("add", ".")
        git("commit", "-m", "첫 시험 소스를 기록함")

        def cached_compiler(command, **options):
            if command[0] != "cargo":
                return real_run(command, **options)
            target = Path(command[command.index("--target-dir") + 1])
            output = target / MODULE.TARGET / "debug/fixture.exe"
            output.parent.mkdir(parents=True, exist_ok=True)
            if not output.exists():
                header = b"MZ" + bytes(58) + struct.pack("<I", 128) + bytes(64)
                output.write_bytes(header + b"PE\0\0" + struct.pack("<H", 0x8664)
                                   + manifest.read_bytes())
            return subprocess.CompletedProcess(command, 0)

        with patch.dict(MODULE.os.environ, {"CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER": sys.executable}):
            with patch.object(MODULE.subprocess, "run", side_effect=cached_compiler):
                first, first_record = MODULE.build_binary(manifest, "fixture", self.root / "target", "dev")
            manifest.write_text("changed fixture")
            git("add", ".")
            git("commit", "-m", "변경된 시험 소스를 기록함")
            with patch.object(MODULE.subprocess, "run", side_effect=cached_compiler):
                second, second_record = MODULE.build_binary(manifest, "fixture", self.root / "target", "dev")
        self.assertNotEqual(first_record["commit"], second_record["commit"])
        self.assertNotEqual(first_record["sha256"], second_record["sha256"])
        self.assertTrue(second.read_bytes().endswith(b"changed fixture"))
        self.assertTrue(first.read_bytes().endswith(b"fixture"))

import argparse
import importlib.util
from pathlib import Path
import struct
import tempfile
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

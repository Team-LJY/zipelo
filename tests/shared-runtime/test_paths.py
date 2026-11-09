"""조립기의 실제 경로 경계를 임시 루트에서 확인함."""

import importlib.util
import os
from pathlib import Path
import tempfile
import unittest

PROJECT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "shared_runtime_assemble", PROJECT / "packaging/shared-runtime/assemble.py")
ASSEMBLER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ASSEMBLER)


class PathBoundaryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(dir=Path(__file__).parent)
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name).resolve()
        self.repo = self.base / "repo"
        self.src = self.repo / "src"
        self.src.mkdir(parents=True)
        (self.repo / "existingdir").mkdir()
        self.archives = self.base / "archives"
        self.archives.mkdir()
        self.sdk = self.base / "sdk"
        self.sdk.mkdir()
        self.dotnet = self.sdk / "dotnet"
        self.dotnet.write_bytes(b"test-only SDK marker")
        self.sentinel = self.src / "keep.txt"
        self.sentinel.write_bytes(b"source preserved")
        self.addCleanup(self.check_preserved)

    def check_preserved(self):
        self.assertEqual(self.sentinel.read_bytes(), b"source preserved")
        self.assertEqual(self.dotnet.read_bytes(), b"test-only SDK marker")
        self.assertEqual(list(self.src.iterdir()), [self.sentinel])

    def validate(self, output, archives=None):
        return ASSEMBLER.validate_paths(output, archives or self.archives, self.dotnet, self.repo)

    def rejected(self, output, archives=None):
        with self.assertRaises((ValueError, OSError)):
            self.validate(output, archives)

    def test_normal_new_output(self):
        output = self.base / "candidate"
        self.assertEqual(self.validate(output), (output, self.archives, self.dotnet))
        self.assertFalse(output.exists())

    def test_dotdot_into_source(self):
        output = self.repo / "existingdir" / ".." / "src" / "new-output"
        self.assertFalse(output.absolute().is_relative_to(self.src))
        self.rejected(output)

    def test_direct_source_output(self):
        self.rejected(self.src / "new-output")

    def test_project_metadata_output(self):
        self.rejected(self.repo / "new-output")

    def test_dotdot_normal_external_output(self):
        parent = self.base / "external"
        parent.mkdir()
        self.assertEqual(self.validate(parent / ".." / "candidate")[0], self.base / "candidate")

    def test_symlink_parent_into_source(self):
        alias = self.base / "source-link"
        alias.symlink_to(self.src, target_is_directory=True)
        self.rejected(alias / "candidate")

    def test_symlink_then_dotdot_into_source(self):
        alias = self.base / "repo-link"
        alias.symlink_to(self.repo / "existingdir", target_is_directory=True)
        self.rejected(alias / ".." / "src" / "candidate")

    def test_safe_symlink_parent_is_canonicalized(self):
        parent = self.base / "external"
        parent.mkdir()
        alias = self.base / "external-link"
        alias.symlink_to(parent, target_is_directory=True)
        self.assertEqual(self.validate(alias / "candidate")[0], parent / "candidate")

    def test_archive_child_output(self):
        self.rejected(self.archives / "candidate")

    def test_archive_alias_child_output(self):
        alias = self.base / "archive-link"
        alias.symlink_to(self.archives, target_is_directory=True)
        self.rejected(self.archives / "candidate", alias)

    def test_sdk_child_output(self):
        self.rejected(self.sdk / "candidate")

    def test_existing_directory_preserved(self):
        output = self.base / "candidate"
        output.mkdir()
        marker = output / "keep"
        marker.write_bytes(b"existing output")
        self.rejected(output)
        self.assertEqual(marker.read_bytes(), b"existing output")

    def test_existing_file_preserved(self):
        output = self.base / "candidate"
        output.write_bytes(b"existing file")
        self.rejected(output)
        self.assertEqual(output.read_bytes(), b"existing file")

    def test_existing_output_ancestor(self):
        self.rejected(self.base)

    def test_dangling_leaf_symlink_preserved(self):
        output = self.base / "candidate"
        output.symlink_to(self.base / "missing")
        self.rejected(output)
        self.assertTrue(output.is_symlink())

    def test_missing_parent(self):
        self.rejected(self.base / "missing" / "candidate")

    def test_missing_parent_with_dotdot(self):
        self.rejected(self.base / "missing" / ".." / "candidate")

    def test_missing_archives(self):
        self.rejected(self.base / "candidate", self.base / "missing")

    def test_relative_output(self):
        previous = Path.cwd()
        try:
            os.chdir(self.base)
            self.assertEqual(self.validate(Path("candidate"))[0], self.base / "candidate")
        finally:
            os.chdir(previous)


if __name__ == "__main__":
    unittest.main()

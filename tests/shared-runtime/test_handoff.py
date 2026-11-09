"""분리 게시의 변조/낡은 결과 거부를 작은 대역 파일로 검사함."""

import json
from pathlib import Path
import sys
import tempfile
import unittest

PROJECT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(PROJECT / "packaging/shared-runtime"))
import handoff


class HandoffTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(dir=Path(__file__).parent)
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.bundle = self.base / "bundle"
        self.bundle.mkdir()
        self.results = self.base / "results"
        self.results.mkdir()
        self.plan_hash = "a" * 64
        self.plan = {"planId": "b" * 32, "rid": "linux-x64", "version": "10.0.14"}
        header = bytearray(64)
        header[:6] = b"\x7fELF\x02\x01"
        header[18:20] = b"\x3e\0"
        self.template = bytes(header) + handoff.APP_PLACEHOLDER + bytes(1100) + handoff.SEARCH_PLACEHOLDER + bytes(600)
        (self.bundle / "apphost").mkdir()
        (self.bundle / "apphost/apphost").write_bytes(self.template)
        for prefix, stem, relative in [("ui", "Zipelo.Ubuntu", "runtime"),
                                       ("worker", "Zipelo.Worker", "../runtime")]:
            directory = self.results / prefix
            directory.mkdir()
            data = bytearray(self.template)
            for placeholder, replacement in [(handoff.APP_PLACEHOLDER, stem.encode() + b".dll\0"),
                                              (handoff.SEARCH_PLACEHOLDER, b"\x02\0" + relative.encode() + b"\0")]:
                offset = self.template.index(placeholder)
                data[offset:offset + len(replacement)] = replacement
            (directory / stem).write_bytes(data)
            for assembly in [stem + ".dll", "Zipelo.Core.dll", "Zipelo.Runtime.dll"]:
                (directory / assembly).write_bytes(b"ZipeloPublishPlan" + self.plan_hash.encode())
        self.receipt()

    def receipt(self, **updates):
        path = self.results / "publish-receipt.json"
        if path.exists():
            path.unlink()
        receipt = {"schema": 2, "planId": self.plan["planId"], "planSha256": self.plan_hash,
                   "rid": "linux-x64", "version": "10.0.14", "sdkVersion": "10.0.403",
                   "completed": True, "files": handoff.inventory(self.results)}
        receipt.update(updates)
        path.write_text(json.dumps(receipt))

    def verify(self):
        return handoff.verify_result(self.results, self.bundle, self.plan, self.plan_hash)

    def test_matching_result(self):
        self.assertTrue(self.verify()["completed"])

    def test_wrong_plan(self):
        self.receipt(planSha256="c" * 64)
        with self.assertRaises(ValueError):
            self.verify()

    def test_wrong_rid(self):
        self.receipt(rid="win-x64")
        with self.assertRaises(ValueError):
            self.verify()

    def test_wrong_version(self):
        self.receipt(version="10.0.13")
        with self.assertRaises(ValueError):
            self.verify()

    def test_unfinished_publish(self):
        self.receipt(completed=False)
        with self.assertRaises(ValueError):
            self.verify()

    def test_changed_output(self):
        (self.results / "ui/Zipelo.Ubuntu.dll").write_bytes(b"modified")
        with self.assertRaises(ValueError):
            self.verify()

    def test_extra_output(self):
        (self.results / "extra").write_bytes(b"unexpected")
        with self.assertRaises(ValueError):
            self.verify()

    def test_receipt_does_not_allow_external_output(self):
        (self.results / "extra").write_bytes(b"unexpected")
        self.receipt()
        with self.assertRaises(ValueError):
            self.verify()

    def test_old_assembly_with_rehashed_receipt(self):
        (self.results / "ui/Zipelo.Core.dll").write_bytes(b"old assembly")
        self.receipt()
        with self.assertRaises(ValueError):
            self.verify()

    def test_apphost_fallback_rejected(self):
        path = self.results / "worker/Zipelo.Worker"
        data = bytearray(path.read_bytes())
        data[self.template.index(handoff.SEARCH_PLACEHOLDER)] = 14
        path.write_bytes(data)
        self.receipt()
        with self.assertRaises(ValueError):
            self.verify()

    def test_apphost_code_change_with_rehashed_receipt(self):
        path = self.results / "ui/Zipelo.Ubuntu"
        data = bytearray(path.read_bytes())
        data[-1] = 1
        path.write_bytes(data)
        self.receipt()
        with self.assertRaises(ValueError):
            self.verify()

    def test_output_beside_notice_file(self):
        notice = self.base / "THIRD_PARTY_NOTICES.txt"
        notice.write_text("test-only notice")
        output = self.base / "candidate"
        self.assertEqual(handoff.new_output(output, [self.bundle, notice]), output)
        self.assertFalse(output.exists())

    def test_wrong_apphost_target(self):
        path = self.results / "ui/Zipelo.Ubuntu"
        data = bytearray(path.read_bytes())
        data[self.template.index(handoff.APP_PLACEHOLDER)] = 88
        path.write_bytes(data)
        self.receipt()
        with self.assertRaises(ValueError):
            self.verify()

    def test_wrong_architecture(self):
        path = self.results / "ui/Zipelo.Ubuntu"
        data = bytearray(path.read_bytes())
        data[18] = 183
        path.write_bytes(data)
        self.receipt()
        with self.assertRaises(ValueError):
            self.verify()

    def test_missing_receipt(self):
        (self.results / "publish-receipt.json").unlink()
        with self.assertRaises(ValueError):
            self.verify()

    def test_symlink_output(self):
        path = self.results / "ui/Zipelo.Core.dll"
        path.unlink()
        path.symlink_to(self.results / "worker/Zipelo.Core.dll")
        with self.assertRaises(ValueError):
            self.verify()

    def test_wrong_pinned_plan_hash(self):
        (self.bundle / "plan.json").write_text("{}")
        with self.assertRaises(ValueError):
            handoff.load_plan(self.bundle, self.plan_hash)

    def test_new_output_overlaps_input(self):
        with self.assertRaises(ValueError):
            handoff.new_output(self.bundle / "candidate", [self.bundle])

    def test_new_output_symlink_into_input(self):
        link = self.base / "input-link"
        link.symlink_to(self.bundle, target_is_directory=True)
        with self.assertRaises(ValueError):
            handoff.new_output(link / "candidate", [self.bundle])

    def test_normal_new_output(self):
        output = self.base / "candidate"
        self.assertEqual(handoff.new_output(output, [self.bundle]), output)
        self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()

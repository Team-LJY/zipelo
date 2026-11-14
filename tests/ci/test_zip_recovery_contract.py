import json
from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]


def enum_tags(source, name):
    match = re.search(r"enumeration!\(\s*" + name + r"\s*\{([^}]+)\}", source)
    if match is None:
        raise ValueError("활성 enum을 찾지 못함: " + name)
    tags = {key: int(value) for key, value in re.findall(r"(\w+)\s*=\s*(\d+)", match[1])}
    if not tags or len(set(tags.values())) != len(tags):
        raise ValueError("활성 enum의 태그가 비어 있거나 충돌함")
    return tags


def check_extension(active, proposed):
    occupied = {value: name for name, value in active.items()}
    for name, value in proposed.items():
        if name in active and active[name] != value:
            raise ValueError("기존 태그를 바꾸는 제안임: " + name)
        if value in occupied and occupied[value] != name:
            raise ValueError("기존 태그와 충돌함: " + name)
        occupied[value] = name


class ZipRecoveryContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        worker = ROOT / "experiments" / "rust-archive-worker"
        cls.wire = (worker / "src" / "wire.rs").read_text(encoding="utf-8")
        cls.contract = json.loads((worker / "contract.json").read_text(encoding="utf-8"))
        cls.proposal = cls.contract["zip_recovery_proposal"]

    def test_error_tags_are_checked_against_live_wire(self):
        active = enum_tags(self.wire, "ErrorCode")
        self.assertEqual(active["MissingVolume"], 14)
        check_extension(active, self.contract["error_tags"])
        check_extension(active, self.proposal["proposed_tags"]["ErrorCode"])

    def test_original_error_tag_collision_is_rejected(self):
        active = enum_tags(self.wire, "ErrorCode")
        with self.assertRaises(ValueError):
            check_extension(active, {"RecoveryIncomplete": 14, "RecoveryImpossible": 15})

    def test_action_tag_does_not_alias_existing_action(self):
        active = enum_tags(self.wire, "ActionKind")
        request = self.proposal["request"]
        check_extension(active, {request["action"]: request["proposed_action_tag"]})

    def test_event_kinds_do_not_alias_existing_messages(self):
        decoder = self.wire.split("let m = match kind {", 1)[1]
        branches = re.findall(
            r"^ {12}(\d+) => (.*?)(?=^ {12}(?:\d+|_) =>)",
            decoder,
            flags=re.MULTILINE | re.DOTALL,
        )
        active = {}
        for tag, branch in branches:
            name = re.search(r"Self::(\w+)", branch)
            self.assertIsNotNone(name)
            active[name[1]] = int(tag)
        self.assertEqual(active["VolumeProblem"], 114)
        self.assertEqual(active["PreviewEnd"], 113)
        check_extension(active, self.proposal["proposed_tags"]["event_kinds"])


if __name__ == "__main__":
    unittest.main()

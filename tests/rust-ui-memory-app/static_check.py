import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import sys

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    if sys.flags.optimize:
        raise RuntimeError("최적화 모드에서는 검사할 수 없음")
    if len(sys.argv) != 2 or re.fullmatch(r"[0-9a-f]{64}", sys.argv[1]) is None:
        raise ValueError("별도 확인한 manifest SHA256 인수가 필요함")
    manifest = HERE / "SHA256SUMS"
    if sha(manifest) != sys.argv[1]:
        raise ValueError("manifest 기대 hash 불일치")
    data = manifest.read_bytes()
    if not data.endswith(b"\n"):
        raise ValueError("manifest 마지막 줄바꿈 누락")
    names = set()
    for row in data.decode("ascii").splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_./-]+)", row)
        if match is None:
            raise ValueError("잘못된 hash 행")
        expected, name = match.groups()
        path = PurePosixPath(name)
        if path.is_absolute() or str(path) != name or any(x in ("", ".", "..") for x in name.split("/")) or name in names:
            raise ValueError("잘못된 경로 또는 중복")
        names.add(name)
        current = ROOT
        for part in path.parts:
            current = current / part
            if current.is_symlink():
                raise ValueError("symlink 거부")
        if sha(current) != expected:
            raise ValueError("파일 hash 불일치: " + name)
    expected_names = set(json.loads((HERE / "files.json").read_text()))
    if names != expected_names:
        raise ValueError("누락 또는 추가 경로")
    sources = json.loads((HERE / "sources.json").read_text())
    for source in sources["files"]:
        if sha(ROOT / source["path"]) != source["sha256"]:
            raise ValueError("고정 source 불일치: " + source["path"])
    contract = json.loads((HERE / "contract.json").read_text())
    names = re.findall(r"#\[test\]\s*fn (\w+)", (HERE / "regressions.rs").read_text())
    if len(names) != len(set(names)) or names != contract["validation"]["names"]:
        raise ValueError("선언된 회귀 이름 불일치")
    print(json.dumps({"static_files": len(expected_names), "upstream_files": len(sources["files"]), "declared_tests": len(names), "executed_tests": 0}))


if __name__ == "__main__":
    main()

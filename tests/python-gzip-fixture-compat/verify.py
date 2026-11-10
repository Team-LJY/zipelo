import argparse
import gzip
import hashlib
import json
from pathlib import Path, PurePosixPath
import runpy
import stat
import sys
import zlib

BASE_HEAD = "f7fc8cbece281e2f9a6374b4640ca4dd9d321fbc"
BASE_TREE = "ab5574289f537a4028a3abf092e4634cb9b89282"
CHANGED = {
    "tests/rust-app-core-memory/generate_fixtures.py",
    "tests/rust-app-core-memory/SHA256SUMS",
}


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def git_hash(kind, data):
    return hashlib.sha1(kind.encode() + b" " + str(len(data)).encode() + b"\0" + data).hexdigest()


def tree_hash(entries):
    tree = {}
    for item in entries:
        parts = PurePosixPath(item["path"]).parts
        node = tree
        for part in parts[:-1]:
            node = node.setdefault(part, {})
        require(parts[-1] not in node, "중복 tree 경로임")
        node[parts[-1]] = (item["mode"], item["sha"])

    def calculate(node):
        data = bytearray()
        for name, value in sorted(node.items(), key=lambda row: (row[0] + ("/" if isinstance(row[1], dict) else "")).encode()):
            mode, digest = ("40000", calculate(value)) if isinstance(value, dict) else value
            data.extend(mode.encode() + b" " + name.encode() + b"\0" + bytes.fromhex(digest))
        return git_hash("tree", bytes(data))

    return calculate(tree)


def main():
    require(__debug__ and not sys.flags.optimize, "최적화 모드에서는 검증할 수 없음")
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    args = parser.parse_args()
    root = args.root.resolve()
    overlay_path = root / "tests/python-gzip-fixture-compat/overlay.json"
    overlay = json.loads(overlay_path.read_bytes())
    require(overlay["base_head"] == BASE_HEAD and overlay["base_tree"] == BASE_TREE, "기준 head/tree 불일치")
    require(set(overlay["replacements"]) == CHANGED, "허용 변경 경로 불일치")
    source_manifest = root / overlay["historical_sources"]["path"]
    require(hashlib.sha256(source_manifest.read_bytes()).hexdigest() == overlay["historical_sources"]["sha256"], "과거 출처 pin 변경됨")
    entries = json.loads(source_manifest.read_bytes())["entries"]
    require(len(entries) == 302, "원본 출처 항목 수 불일치")
    entries = [{key: item[key] for key in ["path", "mode", "type", "sha"]} for item in entries]
    entries.extend(overlay["historical_metadata"])
    require(len(entries) == len({item["path"] for item in entries}) == 305, "기준 tree 경로 수 불일치")
    require(tree_hash(entries) == BASE_TREE, "고정 원본 tree 불일치")
    before = {}
    for item in entries:
        relative = PurePosixPath(item["path"])
        require(not relative.is_absolute() and ".." not in relative.parts and str(relative) == item["path"], "경로 이탈 거부함")
        path = root / relative
        require(path.is_file() and not path.is_symlink() and path.resolve().is_relative_to(root), "일반 파일 경로가 아님")
        require(stat.S_IMODE(path.stat().st_mode) == 0o644 and item["mode"] == "100644" and item["type"] == "blob", "고정 파일 mode 불일치")
        raw = path.read_bytes()
        replacement = overlay["replacements"].get(item["path"])
        expected = item["sha"]
        if replacement is not None:
            require(replacement["before_git_blob"] == expected, "overlay 부모 pin 불일치")
            expected = replacement["after_git_blob"]
            require(hashlib.sha256(raw).hexdigest() == replacement["after_sha256"], "overlay SHA256 불일치")
        require(git_hash("blob", raw) == expected, "고정 blob 불일치: " + item["path"])
        before[item["path"]] = hashlib.sha256(raw).hexdigest()
    fixture_dir = root / "tests/rust-app-core-memory"
    generated_hex, generated_json = runpy.run_path(str(fixture_dir / "generate_fixtures.py"))["build"]()
    require(generated_hex.encode() == (fixture_dir / "fixtures.hex").read_bytes(), "기존 fixture bytes와 다름")
    require(generated_json.encode() == (fixture_dir / "fixtures.json").read_bytes(), "기존 fixture JSON bytes와 다름")
    values = dict(line.split(" ", 1) for line in generated_hex.splitlines())
    for name, payload in [("gzip_digits", b"123456789"), ("gzip_empty", b"")]:
        member = bytes.fromhex(values[name])
        require(member[:10] == bytes.fromhex("1f8b08000000000000ff"), "고정 GZip header와 다름")
        require(gzip.decompress(member) == payload, "표준 GZip 해제 결과가 다름")
    for relative, digest in before.items():
        require(hashlib.sha256((root / relative).read_bytes()).hexdigest() == digest, "검증 중 입력 변경됨")
    print(json.dumps({
        "state": "passed",
        "scope": "Python-only fixture bytes/provenance and fixed overlay verification; no Rust compile/run",
        "base_head": BASE_HEAD,
        "base_tree": BASE_TREE,
        "python": sys.version,
        "zlib_runtime": zlib.ZLIB_RUNTIME_VERSION,
        "base_paths_verified": len(before),
        "unchanged_base_paths": len(before) - len(CHANGED),
        "changed_paths": sorted(CHANGED),
        "fixtures_hex_sha256": hashlib.sha256(generated_hex.encode()).hexdigest(),
        "fixtures_json_sha256": hashlib.sha256(generated_json.encode()).hexdigest(),
        "exact_bytes_and_json_equal": True,
        "gzip_members_verified": 2,
        "inputs_unchanged": True,
        "rust_compiled": False,
        "rust_executed": False,
    }, ensure_ascii=False))


if __name__ == "__main__":
    main()

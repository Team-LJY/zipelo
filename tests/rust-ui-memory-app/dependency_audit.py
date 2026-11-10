import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
MANIFEST = ROOT / "experiments/rust-ui-memory-app/Cargo.toml"
TARGETS = ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-gnu", "x86_64-pc-windows-msvc"]


def sha(data):
    return hashlib.sha256(data).hexdigest()


def cargo(*args):
    return subprocess.check_output(["cargo", "+1.99.1", *args, "--locked", "--offline", "--manifest-path", str(MANIFEST)], cwd=ROOT, text=True)


def main():
    if sys.flags.optimize:
        raise RuntimeError("최적화 모드에서는 감사할 수 없음")
    locks = [tomllib.loads((ROOT / path).read_text())["package"] for path in ["tests/rust-ui-memory-app/reference-ui.lock", "experiments/rust-app-core-memory/Cargo.lock"]]
    allowed = {(p["name"], p["version"], p.get("checksum")) for lock in locks for p in lock if p.get("source")}
    current = tomllib.loads(MANIFEST.with_name("Cargo.lock").read_text())["package"]
    records = {(p["name"], p["version"], p.get("checksum")) for p in current if p.get("source")}
    if not records <= allowed:
        raise RuntimeError("기존 lock 외 registry 항목")
    results = []
    for target in TARGETS:
        for ui in [False, True]:
            flags = ["--features", "ui"] if ui else ["--no-default-features"]
            metadata = json.loads(cargo("metadata", "--format-version", "1", "--filter-platform", target, *flags))
            packages = {(p["name"], p["version"]): p for p in metadata["packages"]}
            tree = cargo("tree", "--target", target, "--prefix", "none", "--format", "{p}|{f}", "-e", "normal,build", *flags)
            active = {}
            for line in tree.splitlines():
                match = re.fullmatch(r"([^ ]+) v([^ ]+)(?: \([^|]*\))?\|([^|]*?)(?: \(\*\))?", line)
                if match is None:
                    raise RuntimeError("Cargo tree 행 해석 실패: " + line)
                name, version, features = match.groups()
                active.setdefault((name, version), set()).update(filter(None, features.split(",")))
            selected = []
            for key, features in sorted(active.items()):
                package = packages[key]
                item = {"name": key[0], "version": key[1], "features": sorted(features), "license": package["license"], "source": package["source"]}
                if package["source"]:
                    lock = next(p for p in current if (p["name"], p["version"]) == key)
                    source = Path(package["manifest_path"]).parent
                    registry = source.parent.parent.parent
                    archive = registry / "cache" / source.parent.name / (key[0] + "-" + key[1] + ".crate")
                    if sha(archive.read_bytes()) != lock["checksum"]:
                        raise RuntimeError("캐시 checksum 불일치: " + str(key))
                    item["checksum"] = lock["checksum"]
                    item["license_files"] = [{"path": str(p.relative_to(source)), "sha256": sha(p.read_bytes())} for p in sorted(source.rglob("*")) if p.is_file() and not p.is_symlink() and p.name.lower().startswith(("license", "licence", "copying", "notice"))]
                else:
                    item["source_path"] = str(Path(package["manifest_path"]).parent.relative_to(ROOT))
                selected.append(item)
            results.append({"target": target, "ui": ui, "selected_packages": len(selected), "packages": selected})
    report = {"scope": "플랫폼별 locked/offline 선택 분석임. Linux 외 빌드나 실행 검증이 아님", "original_ui_head": "63b80e3158c08b71ec70666a2a39216cd36752b1", "core_head": "23b8d1c615f24f42469138fcc63e7fce7cda4f24", "registry_records": len(records), "new_registry_outside_original_union": 0, "selections": results}
    (HERE / "dependencies.json").write_text(json.dumps(report, ensure_ascii=False, indent=4) + "\n")
    print(json.dumps([{k: x[k] for k in ["target", "ui", "selected_packages"]} for x in results]))


if __name__ == "__main__":
    main()

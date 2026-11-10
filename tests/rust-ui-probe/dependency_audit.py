import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "experiments/rust-ui-probe/Cargo.toml"
HERE = Path(__file__).resolve().parent
CARGO = Path(os.environ.get("CARGO_HOME", str(Path.home() / ".cargo")))
SOURCE = "registry+https://github.com/rust-lang/crates.io-index"
TARGETS = ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-gnu", "x86_64-pc-windows-msvc"]


def cargo(*args):
    return subprocess.check_output([
        "cargo", *args, "--locked", "--offline", "--manifest-path", str(MANIFEST)
    ], cwd=ROOT, text=True)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def collect(target, ui):
    features = ["--features", "ui"] if ui else ["--no-default-features"]
    metadata = json.loads(cargo("metadata", "--format-version", "1", "--filter-platform", target, *features))
    packages = {(p["name"], p["version"]): p for p in metadata["packages"]}
    tree = cargo("tree", "--target", target, "--prefix", "none", "--format", "{p}|{f}", "-e", "normal,build", *features)
    active = {}
    for line in tree.splitlines():
        match = re.fullmatch(r"([^ ]+) v([^ ]+)(?: \([^|]*\))?\|([^|]*?)(?: \(\*\))?", line)
        if not match:
            raise RuntimeError("해석할 수 없는 Cargo tree 행: " + line)
        name, version, flags = match.groups()
        active.setdefault((name, version), set()).update(filter(None, flags.split(",")))
    result = []
    ids = {packages[key]["id"] for key in active}
    build_roots = set()
    for node in metadata["resolve"]["nodes"]:
        if node["id"] in ids:
            for dep in node["deps"]:
                if dep["pkg"] in ids and any(k["kind"] == "build" for k in dep["dep_kinds"]):
                    build_roots.add(dep["pkg"])
    for key, flags in sorted(active.items()):
        package = packages[key]
        source = package["source"]
        if source not in (None, SOURCE):
            raise RuntimeError("허용되지 않은 source: " + source)
        item = {
            "name": key[0], "version": key[1], "features": sorted(flags),
            "license": package["license"], "source": source,
            "direct_build_dependency": package["id"] in build_roots,
            "has_build_script": any("custom-build" in t["kind"] for t in package["targets"]),
            "proc_macro": any("proc-macro" in t["kind"] for t in package["targets"]),
        }
        if source:
            entries = [p for p in LOCK["package"] if p["name"] == key[0] and p["version"] == key[1]]
            assert len(entries) == 1
            expected = entries[0]["checksum"]
            archive = next((CARGO / "registry/cache").glob("*/" + key[0] + "-" + key[1] + ".crate"))
            assert digest(archive.read_bytes()) == expected, key
            index = next((CARGO / "registry/index").glob("*/.cache"))
            name = key[0]
            relative = ("1/" + name if len(name) == 1 else "2/" + name if len(name) == 2 else "3/" + name[0] + "/" + name if len(name) == 3 else name[:2] + "/" + name[2:4] + "/" + name)
            records = [json.loads(x) for x in (index / relative).read_bytes().split(b"\0") if x.startswith(b"{")]
            assert any(x["vers"] == key[1] and x["cksum"] == expected for x in records), key
            notices = []
            with tarfile.open(archive) as contents:
                for member in contents.getmembers():
                    if member.isfile() and any(x in Path(member.name).name.upper() for x in ["LICENSE", "LICENCE", "COPYING", "NOTICE"]):
                        assert member.size <= 4 * 1024 * 1024
                        notices.append({"path": member.name, "sha256": digest(contents.extractfile(member).read())})
            item.update({"checksum": expected, "archive_and_index_verified": True, "notice_files": notices})
        result.append(item)
    return {
        "target": target, "ui": ui, "package_count_including_root": len(result),
        "direct_build_dependency_count": len(build_roots),
        "build_script_package_count": sum(x["has_build_script"] for x in result),
        "packages": result,
    }


LOCK = tomllib.loads(MANIFEST.with_name("Cargo.lock").read_text())
REPORT = {
    "schema": 1,
    "manifest_sha256": digest(MANIFEST.read_bytes()),
    "lock_sha256": digest(MANIFEST.with_name("Cargo.lock").read_bytes()),
    "lock_package_count": len(LOCK["package"]),
    "method": "Cargo tree normal/build active packages and features; metadata supplies license and build edges, whose conditional edge counts are conservative",
    "limits": ["metadata alone includes inactive optional packages", "no builds or build scripts executed", "Windows graphs do not prove Windows builds", "notice inventory is not final distribution notice compliance"],
    "graphs": [collect(target, ui) for target in TARGETS for ui in [False, True]],
}
(HERE / "dependency-inventory.json").write_text(json.dumps(REPORT, ensure_ascii=False, indent=4) + "\n")
for graph in REPORT["graphs"]:
    print(graph["target"], "ui" if graph["ui"] else "model", graph["package_count_including_root"], graph["build_script_package_count"])

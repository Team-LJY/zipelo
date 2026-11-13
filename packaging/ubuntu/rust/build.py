"""Rust 앱과 작업자를 Ubuntu amd64 패키지로 생성함."""

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("windows_build", ROOT / "packaging/windows/rust/build.py")
WINDOWS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(WINDOWS)
TARGET = "x86_64-unknown-linux-gnu"


def build(args):
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    stage = output / "stage"
    lib = stage / "usr/lib/zipelo"
    lib.mkdir(parents=True)
    records = {}
    docs = stage / "usr/share/doc/zipelo"
    docs.mkdir(parents=True)
    WINDOWS.LICENSES.generate(
        [ROOT / "experiments/rust-ui-memory-app/Cargo.toml", ROOT / "experiments/rust-archive-worker/Cargo.toml"],
        docs / "THIRD-PARTY-LICENSES.txt",
    )
    for name, manifest, binary, features in (
        ("Zipelo", ROOT / "experiments/rust-ui-memory-app/Cargo.toml", "zipelo-ui-memory-app", ("ui",)),
        ("zipelo-archive-worker", ROOT / "experiments/rust-archive-worker/Cargo.toml", "zipelo-archive-worker", ()),
    ):
        source, records[name] = WINDOWS.build_binary(manifest, binary, Path(args.target_dir).resolve(), args.profile, features, TARGET)
        shutil.copy2(source, lib / name)
        if (lib / name).read_bytes()[:4] != b"\x7fELF":
            raise ValueError("Linux 실행 파일이 아님")
    (lib / "BUILD-INFO.json").write_text(json.dumps({"target": TARGET, "profile": args.profile, "files": records}, indent=4) + "\n")
    bindir = stage / "usr/bin"
    bindir.mkdir()
    (bindir / "zipelo").symlink_to("../lib/zipelo/Zipelo")
    desktop = stage / "usr/share/applications"
    desktop.mkdir(parents=True)
    shutil.copyfile(Path(__file__).with_name("zipelo.desktop"), desktop / "zipelo.desktop")
    icons = stage / "usr/share/icons/hicolor/256x256/apps"
    icons.mkdir(parents=True)
    shutil.copyfile(ROOT / "experiments/rust-ui-memory-app/src/assets/icons/zipelo-256.png", icons / "zipelo.png")
    subprocess.run(["desktop-file-validate", str(desktop / "zipelo.desktop")], check=True)
    shutil.copyfile(ROOT / "LICENSE", docs / "copyright")
    control = stage / "DEBIAN"
    control.mkdir()
    (control / "control").write_text("""Package: zipelo
Version: 0.1.0
Architecture: amd64
Maintainer: Zipelo contributors <hoshinohinata.dev@gmail.com>
Section: utils
Priority: optional
Depends: libc6 (>= 2.39), libgcc-s1, libx11-6, libxkbcommon0, libwayland-client0, libegl1, libgl1, libfontconfig1, fonts-noto-cjk, xdg-utils, desktop-file-utils
Description: Lightweight Rust archive application
 ZIP, 7z, TAR, GZ and TAR.GZ archive application with a separate worker.
""")
    for script in ("postinst", "postrm"):
        shutil.copyfile(Path(__file__).with_name(script), control / script)
        (control / script).chmod(0o755)
    package = output / "zipelo_0.1.0_amd64.deb"
    subprocess.run(["dpkg-deb", "--root-owner-group", "--build", str(stage), str(package)], check=True)
    print(package)
    print("SHA-256 " + hashlib.sha256(package.read_bytes()).hexdigest())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True)
    parser.add_argument("--target-dir", required=True)
    parser.add_argument("--profile", choices=("dev", "release"), default="release")
    build(parser.parse_args())


if __name__ == "__main__":
    main()

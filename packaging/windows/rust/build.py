"""Rust 앱과 작업자를 교차 빌드해 Windows 개발용 설치 파일을 생성함."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess

ROOT = Path(__file__).resolve().parents[3]
TARGET = "x86_64-pc-windows-gnu"


def pe_machine(path):
    with Path(path).open("rb") as stream:
        header = stream.read(64)
        if len(header) != 64 or header[:2] != b"MZ":
            raise ValueError("PE 실행 파일이 아님")
        stream.seek(struct.unpack_from("<I", header, 60)[0])
        signature = stream.read(6)
    if len(signature) != 6 or signature[:4] != b"PE\0\0":
        raise ValueError("PE 헤더가 잘못됨")
    return struct.unpack_from("<H", signature, 4)[0]


def build_binary(manifest, binary, target_dir, profile, features=(), target=TARGET):
    manifest = Path(manifest).resolve(strict=True)
    commit = subprocess.check_output(
        ["git", "-C", str(manifest.parent), "rev-parse", "HEAD"], text=True
    ).strip()
    dirty = subprocess.check_output(
        ["git", "-C", str(manifest.parent), "status", "--porcelain"], text=True
    )
    if dirty.strip():
        raise ValueError("소스에 커밋하지 않은 변경이 있음")
    # 작업 복사본 간 mtime 기반 캐시 혼합을 방지함.
    target_dir = target_dir / binary / commit
    command = ["cargo", "+1.99.1", "build", "--locked", "--manifest-path", str(manifest),
               "--target", target, "--target-dir", str(target_dir), "--profile", profile,
               "--bin", binary]
    if features:
        command += ["--features", ",".join(features)]
    environment = dict(os.environ)
    linker = environment.get("CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER")
    linker = linker or shutil.which("x86_64-w64-mingw32-gcc") or shutil.which("x86_64-w64-mingw32-gcc-posix")
    if target == TARGET and (not linker or not shutil.which(linker)):
        raise ValueError("MinGW-w64 교차 링커를 찾지 못함")
    if target == TARGET:
        environment["CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER"] = linker
    subprocess.run(command, env=environment, check=True)
    output = target_dir / target / ("debug" if profile == "dev" else profile) / (binary + (".exe" if target == TARGET else ""))
    if target == TARGET and pe_machine(output) != 0x8664:
        raise ValueError("x64 Windows 실행 파일이 아님")
    current_commit = subprocess.check_output(
        ["git", "-C", str(manifest.parent), "rev-parse", "HEAD"], text=True
    ).strip()
    dirty = subprocess.check_output(
        ["git", "-C", str(manifest.parent), "status", "--porcelain"], text=True
    )
    if current_commit != commit or dirty.strip():
        raise ValueError("빌드 중 소스가 바뀜")
    return output, {"commit": commit, "sha256": hashlib.sha256(output.read_bytes()).hexdigest(),
                    "binary": binary, "features": list(features)}


def build(args):
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    target_dir = Path(args.target_dir).resolve()
    records = {}
    payload = output / "payload"
    payload.mkdir()
    for name, manifest, binary, features in (
        ("Zipelo.exe", args.app_manifest, "zipelo-ui-memory-app", ("ui",)),
        ("zipelo-archive-worker.exe", args.worker_manifest, "zipelo-archive-worker", ()),
    ):
        source, records[name] = build_binary(manifest, binary, target_dir, args.profile, features)
        shutil.copyfile(source, payload / name)
    shutil.copyfile(ROOT / "LICENSE", payload / "LICENSE")
    records = {"development_only": True, "windows_execution_verified": False,
               "profile": args.profile, "target": TARGET, "files": records}
    (payload / "BUILD-INFO.json").write_text(json.dumps(records, indent=4) + "\n", encoding="utf-8")
    shutil.copyfile(Path(__file__).with_name("installer.nsi"), output / "installer.nsi")
    subprocess.run(["makensis", "-WX", "installer.nsi"], cwd=output, check=True)
    installer = output / "Zipelo-0.1.0-dev-windows-x64-setup.exe"
    if pe_machine(installer) not in (0x14C, 0x8664):
        raise ValueError("NSIS 설치 실행 파일이 아님")
    print(installer)
    print("SHA-256 " + hashlib.sha256(installer.read_bytes()).hexdigest())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app-manifest", required=True)
    parser.add_argument("--worker-manifest", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--target-dir", required=True)
    parser.add_argument("--profile", choices=("dev", "release"), default="release")
    build(parser.parse_args())


if __name__ == "__main__":
    main()

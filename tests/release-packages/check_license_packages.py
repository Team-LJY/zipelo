"""실제 두 패키지의 고지 원문과 Cargo 항목 일치를 확인함."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[2]


def deb_notice(path):
    matches = []
    with subprocess.Popen(["dpkg-deb", "--fsys-tarfile", str(path)], stdout=subprocess.PIPE) as process:
        with tarfile.open(fileobj=process.stdout, mode="r|") as archive:
            for member in archive:
                if member.name.removeprefix("./") == "usr/share/doc/zipelo/THIRD-PARTY-LICENSES.txt":
                    if not member.isfile():
                        raise ValueError("deb 고지가 일반 파일이 아님")
                    matches.append(archive.extractfile(member).read())
        if process.wait() != 0:
            raise ValueError("deb 읽기 실패")
    if len(matches) != 1:
        raise ValueError("deb 고지 누락 또는 중복")
    return matches[0]


def windows_notice(path, sevenzip):
    listing = subprocess.check_output([sevenzip, "l", "-slt", "-ba", str(path)], text=True)
    paths = [line.removeprefix("Path = ") for line in listing.splitlines() if line.startswith("Path = ")]
    matches = [name for name in paths if name.replace("\\", "/").split("/")[-1] == "THIRD-PARTY-LICENSES.txt"]
    if len(matches) != 1:
        raise ValueError("NSIS 고지 누락 또는 중복")
    return subprocess.check_output([sevenzip, "x", "-so", str(path), matches[0]])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["windows", "ubuntu", "notice", "inventory", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--sevenzip", required=True)
    args = parser.parse_args()
    if not args.output.resolve().is_relative_to(ROOT / "tests"):
        raise ValueError("검증 결과는 tests 안에 저장해야 함")
    expected = args.notice.read_bytes()
    inventory = json.loads(args.inventory.read_text())
    headings = {line.removeprefix("Crate: ") for line in expected.decode("utf-8").splitlines()
                if line.startswith("Crate: ")}
    expected_headings = {row["name"] + " " + row["version"] for row in inventory}
    if headings != expected_headings or not all(row["files"] for row in inventory):
        raise ValueError("고지 항목과 inventory 불일치")
    if expected.count(b"\nCrate: ") != len(inventory):
        raise ValueError("고지 항목 중복")
    for label, actual in [("windows", windows_notice(args.windows, args.sevenzip)),
                          ("ubuntu", deb_notice(args.ubuntu))]:
        if actual != expected:
            raise ValueError(label + " 패키지 고지 원문 불일치")
    report = {
        "crates": len(inventory),
        "missing": 0,
        "notice_sha256": hashlib.sha256(expected).hexdigest(),
        "notice_bytes": len(expected),
        "standard_supplements": expected.decode("utf-8").count("upstream에 원문이 없어 표준 원문으로 보충함"),
        "packages": {label: {"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                             "bytes": path.stat().st_size}
                     for label, path in [("windows", args.windows), ("ubuntu", args.ubuntu)]},
    }
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=4) + "\n")
    print(json.dumps(report, ensure_ascii=False))


if __name__ == "__main__":
    main()

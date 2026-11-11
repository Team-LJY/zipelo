"""잠금 의존성의 로컬 원문을 모으고 누락 시 배포를 중단함."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def identity(package):
    return package["name"], package["version"], package.get("source") or "workspace"


def metadata(manifest):
    result = subprocess.run(
        ["cargo", "+1.99.1", "metadata", "--format-version", "1", "--locked", "--offline",
         "--all-features", "--manifest-path", str(manifest)],
        check=True, capture_output=True, text=True,
    )
    return json.loads(result.stdout)["packages"]


def is_license(path):
    path = Path(path)
    name = path.name.upper()
    return (
        name.startswith(("LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE", "UNLICENSE", "OFL"))
        or any(part.lower() in {"license", "licenses", "licence", "licences"} for part in path.parts[:-1])
    ) and path.suffix.lower() not in {".rs", ".c", ".h", ".cpp", ".py", ".json"}


def registry_files(package, locked):
    root = Path(package["manifest_path"]).parent.resolve()
    archive = root.parents[2] / "cache" / root.parent.name / (root.name + ".crate")
    if hashlib.sha256(archive.read_bytes()).hexdigest() != locked["checksum"]:
        raise ValueError(f"crate 체크섬 불일치: {root.name}")
    explicit = package.get("license_file")
    if explicit:
        explicit = Path(explicit)
        if not explicit.is_absolute():
            explicit = root / explicit
        explicit = explicit.resolve().relative_to(root).as_posix()
    records = []
    with tarfile.open(archive, "r:gz") as source:
        for member in sorted(source.getmembers(), key=lambda member: member.name):
            name = Path(member.name)
            if name.parts[0] != root.name or ".." in name.parts:
                raise ValueError(f"crate 경로 오류: {root.name}")
            relative = Path(*name.parts[1:])
            if not (is_license(relative) or relative.as_posix() == explicit):
                continue
            if not member.isfile():
                if member.isdir():
                    continue
                raise ValueError(f"라이선스가 일반 파일이 아님: {member.name}")
            local = root / relative
            if local.is_symlink() or not local.resolve().is_relative_to(root):
                raise ValueError(f"라이선스 경로 이탈: {relative}")
            content = local.read_bytes()
            if content != source.extractfile(member).read():
                raise ValueError(f"라이선스 원문 변경: {root.name}/{relative}")
            records.append({
                "path": relative.as_posix(),
                "sha256": hashlib.sha256(content).hexdigest(),
                "text": content.decode("utf-8"),
            })
    return records


def supplemental_files(package, supplement_path):
    data = json.loads(Path(supplement_path).read_text(encoding="utf-8"))
    candidates = [row for row in data["packages"]
                  if (row["name"], row["version"]) == (package["name"], package["version"])]
    if not candidates:
        return []
    if len(candidates) != 1:
        raise ValueError("보충 항목 중복")
    row = candidates[0]
    if row["license"] != package["license"]:
        raise ValueError(f"보충 SPDX 불일치: {package['name']}")
    root = Path(package["manifest_path"]).parent.resolve()
    archive = root.parents[2] / "cache" / root.parent.name / (root.name + ".crate")
    with tarfile.open(archive, "r:gz") as source:
        vcs = json.load(source.extractfile(root.name + "/.cargo_vcs_info.json"))
    if vcs["git"]["sha1"] != row["rev"]:
        raise ValueError(f"보충 버전 커밋 불일치: {package['name']}")
    records = []
    for url in row["files"]:
        file = data["files"][url]
        parts = url.split("/")
        if (len(parts) < 7 or parts[:3] != ["https:", "", "raw.githubusercontent.com"]
                or parts[5] != row["upstream_ref"]
                or row["upstream_ref"] not in {row["rev"], row["version"]}):
            raise ValueError(f"보충 출처 고정 오류: {url}")
        content = file["text"].encode("utf-8")
        if file["url"] != url or hashlib.sha256(content).hexdigest() != file["sha256"]:
            raise ValueError(f"보충 원문 해시 불일치: {url}")
        records.append({"path": url, "sha256": file["sha256"], "text": file["text"]})
    return records


def inventory(manifests, project_root=ROOT, supplement_path=ROOT / "packaging/license-supplements.json"):
    locked = {}
    packages = {}
    for manifest in manifests:
        manifest = Path(manifest).resolve(strict=True)
        lock = tomllib.loads(manifest.with_name("Cargo.lock").read_text())
        locked.update({identity(package): package for package in lock["package"]})
        packages.update({identity(package): package for package in metadata(manifest)})
    if set(locked) != set(packages):
        raise ValueError(f"Cargo.lock/metadata 불일치: 누락 {sorted(set(locked) - set(packages))}, "
                         f"추가 {sorted(set(packages) - set(locked))}")
    records = []
    for key, package in sorted(packages.items()):
        if package.get("source"):
            if not package["source"].startswith("registry+"):
                raise ValueError(f"지원하지 않는 원문 저장소: {key}")
            files = registry_files(package, locked[key])
            if not files and Path(supplement_path).is_file():
                files = supplemental_files(package, supplement_path)
        else:
            root = Path(project_root).resolve()
            if package["license"] != "MIT" or not Path(package["manifest_path"]).resolve().is_relative_to(root):
                raise ValueError(f"프로젝트 라이선스 범위 확인 필요: {key}")
            content = (root / "LICENSE").read_bytes()
            files = [{"path": "project/LICENSE", "sha256": hashlib.sha256(content).hexdigest(),
                      "text": content.decode("utf-8")}]
        records.append({"name": key[0], "version": key[1], "source": key[2],
                        "license": package["license"], "files": files})
    return records


def render(records):
    missing = [f'{record["name"]}@{record["version"]}' for record in records if not record["files"]]
    if missing:
        raise ValueError(f"라이선스 원문 누락 {len(missing)}개: " + ", ".join(missing))
    lines = ["Zipelo third-party licenses", "Cargo.lock 전체 의존성의 원문 고지 (로컬 registry 및 고정 upstream 보충)", ""]
    for record in records:
        lines.extend(["=" * 72, f'Crate: {record["name"]} {record["version"]}',
                      f'Source: {record["source"]}', f'License: {record["license"]}', f'Version: {record["version"]}', ""])
        for file in record["files"]:
            lines.extend([f'File: {file["path"]}', f'SHA-256: {file["sha256"]}', "", file["text"], ""])
    return "\n".join(lines)


def generate(manifests, output, audit=None):
    records = inventory(manifests)
    if audit:
        Path(audit).write_text(json.dumps(records, ensure_ascii=False, indent=4) + "\n", encoding="utf-8")
    text = render(records)
    Path(output).write_text(text, encoding="utf-8", newline="")
    return records


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, action="append", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--audit", type=Path)
    args = parser.parse_args()
    try:
        records = generate(args.manifest, args.output, args.audit)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, str(error) + "\n")
    print(f"라이선스 {len(records)}개, 누락 0개")


if __name__ == "__main__":
    main()

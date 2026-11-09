"""Python 준비/최종화와 별도 Windows SDK 게시를 연결함."""

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import tempfile
import uuid

import assemble as core

HERE = Path(__file__).resolve().parent
ROOT = core.ROOT
TOOLS = ["assemble.py", "handoff.py", "inputs.json", "publish.ps1", "Plan.targets"]
SOURCE_SUFFIXES = {".cs", ".csproj", ".xaml", ".axaml", ".targets", ".pubxml", ".manifest"}
APP_PLACEHOLDER = b"c3ab8ff13720e8ad9047dd39466b3c8974e592c2fa383d4a3960714caef0c4f2"
SEARCH_PLACEHOLDER = b"\0\x0019ff3e9c3602ae8e841925bb461a0adb064a1f1903667a5e0d87e8f608f425ac"


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def inventory(root):
    result = {}
    for path in sorted(root.rglob("*")):
        core.require(not path.is_symlink(), "링크 입력 거부")
        if path.is_dir():
            continue
        core.require(path.is_file() and path.stat().st_nlink == 1, "일반 단일 링크 파일 필요")
        name = path.relative_to(root).as_posix()
        core.safe_parts(name)
        result[name] = {"sha256": core.digest(path), "bytes": path.stat().st_size}
    core.require(result, "빈 파일 목록 거부")
    return result


def source_paths():
    paths = []
    for path in sorted((ROOT / "src").rglob("*")):
        relative = path.relative_to(ROOT)
        if any(part in {"bin", "obj"} for part in relative.parts):
            continue
        core.require(not path.is_symlink(), "소스 링크 거부")
        if path.is_file():
            core.require(path.suffix in SOURCE_SUFFIXES, "소스 snapshot 범위를 확인해야 함: " + str(relative))
            paths.append(relative)
    return paths


def new_output(output, protected):
    core.require(output.name not in {"", ".", ".."} and not os.path.lexists(output), "기존 출력 거부")
    output = output.parent.resolve(strict=True) / output.name
    core.require(output.parent.is_dir() and not os.path.lexists(output), "새 출력 부모 필요")
    for path in protected:
        path = path.resolve(strict=True)
        core.require(not output.is_relative_to(path) and not path.is_relative_to(output), "입력/소스/출력 중첩 거부")
    for path in [output, *protected]:
        core.require(not any(c in str(path) for c in ";,%$\n\r"), "MSBuild 경로 해석 문자 거부")
    return output


def move_new(payload, output, protected):
    core.require(new_output(output, protected) == output, "출력 부모 경로 변경")
    output.mkdir()
    try:
        for path in payload.iterdir():
            shutil.move(str(path), output / path.name)
    except BaseException:
        shutil.rmtree(output)
        raise


def prepare(args):
    archives = args.archives.resolve(strict=True)
    notices = args.notices.resolve(strict=True)
    core.require(archives.is_dir() and notices.is_file() and notices.stat().st_size > 0 and not args.notices.is_symlink(), "공식 입력/검토된 고지 필요")
    output = new_output(args.output, [ROOT, archives, notices])
    config = read_json(HERE / "inputs.json")
    core.require(config["version"] == core.VERSION, "공식 입력 버전 불일치")
    inputs = config["platforms"][args.rid]
    for item in inputs:
        path = archives / item["file"]
        core.require(path.is_file() and not path.is_symlink(), "공식 아카이브 누락")
        core.require(core.digest(path, "sha512") == item["sha512"], "공식 아카이브 SHA-512 불일치")
    with tempfile.TemporaryDirectory(dir=output.parent, prefix="zipelo-plan-") as temporary:
        staging = Path(temporary)
        payload = staging / "bundle"
        payload.mkdir()
        apphost = core.prepare_runtime(archives, args.rid, staging, payload, inputs)
        shutil.copytree(apphost.parent, payload / "apphost")
        source = payload / "source"
        for relative in source_paths():
            target = source / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, target)
        core.require((ROOT / "LICENSE").is_file() and (ROOT / "LICENSE").stat().st_size > 0, "제품 MIT 라이선스 필요")
        shutil.copyfile(ROOT / "LICENSE", source / "LICENSE")
        shutil.copyfile(notices, source / "THIRD_PARTY_NOTICES.txt")
        for name in TOOLS:
            shutil.copyfile(HERE / name, payload / name)
        plan = {"schema": 2, "planId": uuid.uuid4().hex, "rid": args.rid,
                "version": core.VERSION, "configuration": "Release", "inputs": inputs,
                "extractionValidation": args.extraction_validation,
                "apphostMode": apphost.stat().st_mode & 0o777,
                "files": inventory(payload)}
        (payload / "plan.json").write_text(json.dumps(plan, sort_keys=True, indent=4) + "\n", encoding="utf-8")
        plan_hash = core.digest(payload / "plan.json")
        move_new(payload, output, [ROOT, archives, notices])
    print(json.dumps({"plan": str(output / "plan.json"), "planSha256": plan_hash,
                      "rid": args.rid, "planId": plan["planId"]}))


def check_files(root, expected, exact=False):
    core.require(isinstance(expected, dict) and expected, "파일 해시 목록 필요")
    for name, entry in expected.items():
        parts = core.safe_parts(name)
        path = root.joinpath(*parts)
        core.require(path.resolve(strict=True).is_relative_to(root.resolve(strict=True)), "파일 경계 이탈")
        core.require(all(not root.joinpath(*parts[:index]).is_symlink()
                         for index in range(1, len(parts) + 1)), "파일/부모 링크 거부")
        core.require(path.is_file() and path.stat().st_nlink == 1, "단일 링크 파일 필요")
        core.require(path.stat().st_size == entry["bytes"] and core.digest(path) == entry["sha256"],
                     "계획/출력 파일 해시 불일치: " + name)
    if exact:
        core.require(inventory(root) == expected, "예상하지 않은 게시 파일 거부")


def load_plan(bundle, expected_hash):
    core.require(re.fullmatch(r"[a-f0-9]{64}", expected_hash) is not None, "준비 시 기록한 계획 SHA-256 필요")
    core.require(not (bundle / "plan.json").is_symlink(), "계획 링크 거부")
    core.require(core.digest(bundle / "plan.json") == expected_hash, "계획 해시 불일치")
    plan = read_json(bundle / "plan.json")
    core.require(plan["schema"] == 2 and plan["version"] == core.VERSION
                 and plan["rid"] in {"win-x64", "linux-x64"}
                 and re.fullmatch(r"[a-f0-9]{32}", plan["planId"]) is not None, "계획 계약 불일치")
    core.require(plan["inputs"] == read_json(HERE / "inputs.json")["platforms"][plan["rid"]], "공식 입력 계약 변경")
    check_files(bundle, plan["files"])
    for name in TOOLS:
        core.require(core.digest(HERE / name) == plan["files"][name]["sha256"], "조립 도구 버전 변경")
    expected_sources = {str(p).replace(os.sep, "/"): core.digest(ROOT / p) for p in source_paths()}
    planned_sources = {name.removeprefix("source/"): entry["sha256"]
                       for name, entry in plan["files"].items() if name.startswith("source/src/")}
    core.require(expected_sources == planned_sources, "원본 소스가 준비 이후 변경됨")
    return plan


def verify_apphost(path, template, stem, relative, windows):
    core.require_x64(path, windows)
    source = template.read_bytes()
    data = path.read_bytes()
    for placeholder, expected in [(APP_PLACEHOLDER, stem.encode() + b".dll\0"),
                                  (SEARCH_PLACEHOLDER, b"\x02\0" + relative.encode() + b"\0")]:
        core.require(source.count(placeholder) == 1, "공식 apphost placeholder 계약 불일치")
        offset = source.index(placeholder)
        core.require(data[offset:offset + len(expected)] == expected, "apphost 대상/검색 경로 불일치")
    if not windows:
        core.require(len(data) == len(source), "공식 Linux apphost 크기 불일치")
        original = bytearray(source)
        published = bytearray(data)
        for placeholder, size in [(APP_PLACEHOLDER, 1024), (SEARCH_PLACEHOLDER, 512)]:
            offset = source.index(placeholder)
            original[offset:offset + size] = bytes(size)
            published[offset:offset + size] = bytes(size)
        core.require(original == published, "공식 apphost 코드/다른 영역 변조")
    else:
        def code_section(binary):
            pe = core.struct.unpack_from("<I", binary, 60)[0]
            count = core.struct.unpack_from("<H", binary, pe + 6)[0]
            optional_size = core.struct.unpack_from("<H", binary, pe + 20)[0]
            table = pe + 24 + optional_size
            for index in range(count):
                section = table + index * 40
                if binary[section:section + 8].rstrip(b"\0") == b".text":
                    length, position = core.struct.unpack_from("<II", binary, section + 16)
                    core.require(position + length <= len(binary), "PE 코드 영역 경계 오류")
                    return binary[position:position + length]
            raise ValueError("PE 코드 영역 누락")
        core.require(code_section(source) == code_section(data), "공식 Windows apphost 코드 불일치")



def verify_result(results, bundle, plan, plan_hash):
    receipt_path = results / "publish-receipt.json"
    core.require(receipt_path.is_file() and not receipt_path.is_symlink(), "SDK 게시 영수증 필요")
    receipt = read_json(receipt_path)
    core.require(receipt.get("schema") == 2 and receipt.get("planSha256") == plan_hash
                 and receipt.get("planId") == plan["planId"] and receipt.get("rid") == plan["rid"]
                 and receipt.get("version") == plan["version"] and receipt.get("completed") is True,
                 "다른 계획/미완료 게시 결과 거부")
    core.require(re.fullmatch(r"10\.[0-9]+\.[0-9]+", receipt.get("sdkVersion", "")) is not None,
                 ".NET 10 SDK 게시 기록 필요")
    expected = receipt["files"]
    core.require(expected and all(name.startswith(("ui/", "worker/")) for name in expected),
                 "게시 UI/Worker 밖의 파일 거부")
    actual = inventory(results)
    actual.pop("publish-receipt.json", None)
    core.require(actual == expected, "게시 영수증/실제 출력 전체 해시 불일치")
    windows = plan["rid"] == "win-x64"
    suffix = ".exe" if windows else ""
    template = bundle / "apphost" / ("apphost" + suffix)
    for prefix, stem, relative in [("ui", "Zipelo" if windows else "Zipelo.Ubuntu", "runtime"),
                                    ("worker", "Zipelo.Worker", "../runtime")]:
        directory = results / prefix
        verify_apphost(directory / (stem + suffix), template, stem, relative, windows)
        for assembly in [stem + ".dll", "Zipelo.Core.dll", "Zipelo.Runtime.dll"]:
            data = (directory / assembly).read_bytes()
            core.require(b"ZipeloPublishPlan" in data and plan_hash.encode() in data,
                         "계획 식별자가 없는 낡은 제품 어셈블리 거부")
    return receipt


def finalize(args):
    bundle = args.bundle.resolve(strict=True)
    results = args.results.resolve(strict=True)
    core.require(bundle.is_dir() and results.is_dir(), "준비 및 게시 결과 폴더 필요")
    output = new_output(args.output, [ROOT, bundle, results])
    plan = load_plan(bundle, args.plan_sha256)
    receipt = verify_result(results, bundle, plan, args.plan_sha256)
    windows = plan["rid"] == "win-x64"
    with tempfile.TemporaryDirectory(dir=output.parent, prefix="zipelo-final-") as temporary:
        payload = Path(temporary) / "payload"
        shutil.copytree(results / "ui", payload)
        core.require(not (payload / "worker").exists() and not (payload / "runtime").exists(), "게시 중첩 거부")
        shutil.copytree(results / "worker", payload / "worker")
        shutil.copytree(bundle / "runtime", payload / "runtime")
        if not windows:
            # Windows 교차 게시 파일은 새 후보에서 공식 Linux apphost의 실행 모드로 배치함.
            mode = plan["apphostMode"]
            core.require(mode == 0o755, "공식 Linux apphost 실행 모드 불일치")
            for path in [payload / "Zipelo.Ubuntu", payload / "worker/Zipelo.Worker"]:
                path.chmod(mode)
        core.verify_app(payload, "Zipelo" if windows else "Zipelo.Ubuntu", windows, windows)
        core.verify_app(payload / "worker", "Zipelo.Worker", windows, False)
        for name in ["LICENSE", "THIRD_PARTY_NOTICES.txt"]:
            shutil.copyfile(bundle / "source" / name, payload / name)
        check_files(bundle, plan["files"])
        verify_result(results, bundle, plan, args.plan_sha256)
        expected_payload = {name.removeprefix("ui/") if name.startswith("ui/") else name: entry
                            for name, entry in receipt["files"].items()}
        expected_payload.update({name: entry for name, entry in plan["files"].items()
                                 if name.startswith("runtime/")})
        for name in ["LICENSE", "THIRD_PARTY_NOTICES.txt"]:
            expected_payload[name] = plan["files"]["source/" + name]
        check_files(payload, expected_payload, exact=True)
        candidate = {"schema": 2, "planSha256": args.plan_sha256, "planId": plan["planId"],
                     "rid": plan["rid"], "version": plan["version"], "inputs": plan["inputs"],
                     "publishReceiptSha256": core.digest(results / "publish-receipt.json"),
                     "sdkVersion": receipt["sdkVersion"], "files": inventory(payload),
                     "releaseReady": False, "actualExecutionValidated": False}
        (payload / "shared-runtime-candidate.json").write_text(json.dumps(candidate, indent=4) + "\n")
        move_new(payload, output, [ROOT, bundle, results])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--validation", action="store_true", required=True)
    modes = parser.add_subparsers(dest="mode", required=True)
    prepare_parser = modes.add_parser("prepare")
    prepare_parser.add_argument("--rid", choices=["win-x64", "linux-x64"], required=True)
    prepare_parser.add_argument("--archives", type=Path, required=True)
    prepare_parser.add_argument("--notices", type=Path, required=True)
    prepare_parser.add_argument("--output", type=Path, required=True)
    prepare_parser.add_argument("--extraction-validation", action="store_true")
    final_parser = modes.add_parser("finalize")
    final_parser.add_argument("--bundle", type=Path, required=True)
    final_parser.add_argument("--results", type=Path, required=True)
    final_parser.add_argument("--plan-sha256", required=True)
    final_parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        (prepare if args.mode == "prepare" else finalize)(args)
    except (ValueError, OSError, KeyError, core.tarfile.TarError, core.zipfile.BadZipFile) as error:
        parser.exit(1, "분리 조립 실패: " + str(error) + "\n")


if __name__ == "__main__":
    main()

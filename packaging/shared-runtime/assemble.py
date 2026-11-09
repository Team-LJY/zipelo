"""로컬 공식 아카이브만 사용하는 공유 런타임 검증 후보 조립기임."""

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import stat
import struct
import subprocess
import tarfile
import tempfile
import zipfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
VERSION = "10.0.14"


def require(ok, message):
    if not ok:
        raise ValueError(message)


def digest(path, algorithm="sha256"):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, algorithm).hexdigest()


def safe_parts(name):
    require("\\" not in name and ":" not in name, "아카이브 경로가 잘못됨")
    path = PurePosixPath(name)
    require(not path.is_absolute() and all(p not in {"", ".", ".."} for p in path.parts),
            "아카이브 상대 경로가 잘못됨")
    return path.parts


def extract(archive, destination, allowed):
    seen = set()

    def put(name, stream, mode):
        parts = safe_parts(name)
        normalized = "/".join(parts)
        require(normalized.casefold() not in seen, "아카이브 이름 중복")
        seen.add(normalized.casefold())
        require(allowed(normalized), "허용하지 않은 공식 아카이브 구성: " + normalized)
        target = destination.joinpath(*parts)
        target.parent.mkdir(parents=True, exist_ok=True)
        if target.exists():
            with tempfile.TemporaryFile() as buffer:
                shutil.copyfileobj(stream, buffer)
                buffer.seek(0)
                incoming = hashlib.file_digest(buffer, "sha256").hexdigest()
            require(digest(target) == incoming, "아카이브 간 파일 충돌")
        else:
            with target.open("xb") as output:
                shutil.copyfileobj(stream, output)
            target.chmod(0o755 if mode & 0o111 else 0o644)

    if archive.name.endswith(".zip"):
        with zipfile.ZipFile(archive) as bundle:
            for member in bundle.infolist():
                safe_parts(member.filename)
                mode = member.external_attr >> 16
                require(not stat.S_ISLNK(mode), "심볼릭 링크 아카이브 거부")
                if member.is_dir():
                    continue
                require(stat.S_IFMT(mode) in {0, stat.S_IFREG}, "특수 파일 거부")
                with bundle.open(member) as stream:
                    put(member.filename, stream, mode)
    else:
        with tarfile.open(archive, "r:gz") as bundle:
            for member in bundle:
                safe_parts(member.name)
                if member.isdir():
                    continue
                require(member.isfile(), "링크 및 특수 파일 아카이브 거부")
                with bundle.extractfile(member) as stream:
                    put(member.name, stream, member.mode)


def require_x64(path, windows):
    with path.open("rb") as stream:
        header = stream.read(64)
        if windows:
            require(header[:2] == b"MZ", "PE 실행파일 필요")
            stream.seek(struct.unpack_from("<I", header, 60)[0])
            require(stream.read(6) == b"PE\0\0\x64\x86", "Windows x64 실행파일 필요")
        else:
            require(header[:6] == b"\x7fELF\x02\x01" and header[18:20] == b"\x3e\0",
                    "Linux x64 ELF 필요")


def verify_app(directory, stem, windows, desktop):
    suffix = ".exe" if windows else ""
    for name in [stem + suffix, stem + ".dll", stem + ".deps.json",
                 stem + ".runtimeconfig.json", "Zipelo.Core.dll", "Zipelo.Runtime.dll"]:
        require((directory / name).is_file(), "게시 필수 파일 누락: " + name)
    require_x64(directory / (stem + suffix), windows)
    if not windows:
        require(os.access(directory / stem, os.X_OK), "Linux apphost 실행 권한 누락")
    options = json.loads((directory / (stem + ".runtimeconfig.json")).read_text())["runtimeOptions"]
    frameworks = options.get("frameworks", [options["framework"]] if "framework" in options else [])
    expected = {"Microsoft.NETCore.App"}
    if desktop:
        expected.add("Microsoft.WindowsDesktop.App")
    require(not options.get("includedFrameworks") and options.get("rollForward") == "Disable",
            "정확한 버전의 framework-dependent 구성이 필요함")
    require({item["name"] for item in frameworks} == expected
            and all(item["version"] == VERSION for item in frameworks), "런타임 버전/프레임워크 불일치")
    require(not (directory / "System.Private.CoreLib.dll").exists(), "앱별 런타임 중복 거부")
    deps = json.loads((directory / (stem + ".deps.json")).read_text())
    rid = "win-x64" if windows else "linux-x64"
    require(deps["runtimeTarget"]["name"].endswith("/" + rid), "게시 RID 불일치")


def publish(dotnet, project, output, apphost, rid, relative, ui, extraction):
    args = [str(dotnet), "publish", str(project), "--no-restore", "-c", "Release",
            "-r", rid, "--self-contained", "false", "-o", str(output),
            "-p:ZipeloSharedRuntimeValidation=true", "-p:ZipeloSharedRuntimeAssembler=true",
            "-p:UseAppHost=true", "-p:PublishSelfContained=false", "-p:PublishSingleFile=false",
            "-p:PublishTrimmed=false", "-p:PublishAot=false", "-p:RuntimeFrameworkVersion=" + VERSION,
            "-p:RollForward=Disable", "-p:TargetLatestRuntimePatch=false",
            "-p:AppHostDotNetSearch=AppRelative", "-p:AppHostRelativeDotNet=" + relative,
            "-p:AppHostSourcePath=" + str(apphost), "-p:EnableAppHostPackDownload=false",
            "-p:EnableTargetingPackDownload=false", "-p:ZipeloCreationValidation=false",
            "-p:ZipeloExtractionValidation=" + str(extraction).lower()]
    if ui:
        args.append("-p:PublishProfile=SharedRuntimeValidation")
    subprocess.run(args, cwd=ROOT, check=True)


def validate_paths(output, archives, dotnet, root=ROOT):
    require(dotnet.is_absolute() and dotnet.is_file() and not dotnet.is_symlink(),
            "검증된 SDK dotnet 호스트의 절대 경로 필요")
    require(output.name not in {"", ".", ".."} and not os.path.lexists(output),
            "기존 출력 또는 잘못된 출력 이름 거부")
    # 기존 부모를 엄격히 해석한 뒤 새 마지막 이름만 결합함.
    normalized_output = output.parent.resolve(strict=True) / output.name
    normalized_archives = archives.resolve(strict=True)
    normalized_dotnet = dotnet.resolve(strict=True)
    normalized_root = root.resolve(strict=True)
    require(normalized_output.parent.is_dir() and normalized_archives.is_dir()
            and normalized_root.is_dir(), "기존 소스/입력/출력 부모 디렉터리 필요")
    require(not os.path.lexists(normalized_output), "기존 출력은 변경하지 않음")
    for protected in [normalized_root, normalized_archives, normalized_dotnet.parent]:
        require(not normalized_output.is_relative_to(protected)
                and not protected.is_relative_to(normalized_output),
                "소스 또는 입력과 출력의 중첩 거부")
    for path in [root, output, archives, dotnet, normalized_root, normalized_output,
                 normalized_archives, normalized_dotnet]:
        require(not any(character in str(path) for character in ";,%$\n\r"),
                "MSBuild 속성 해석 문자가 포함된 경로 거부")
    return normalized_output, normalized_archives, normalized_dotnet


def assemble(args):
    require(args.validation, "--validation으로 개발 검증을 명시해야 함")
    args.output, args.archives, args.dotnet = validate_paths(args.output, args.archives, args.dotnet)
    config = json.loads((HERE / "inputs.json").read_text())
    require(config["version"] == VERSION, "입력 manifest 버전 불일치")
    inputs = config["platforms"][args.rid]
    for item in inputs:
        archive = args.archives / item["file"]
        require(archive.is_file() and not archive.is_symlink(), "공식 로컬 아카이브 누락: " + item["file"])
        require(digest(archive, "sha512") == item["sha512"], "공식 아카이브 SHA-512 불일치")
    windows = args.rid == "win-x64"
    suffix = ".exe" if windows else ""
    with tempfile.TemporaryDirectory(prefix="zipelo-shared-runtime-", dir=args.output.parent) as temporary:
        staging = Path(temporary)
        payload = staging / "payload"
        payload.mkdir()
        runtime = payload / "runtime"
        notices = {"LICENSE.txt", "ThirdPartyNotices.txt", "THIRD-PARTY-NOTICES.txt"}
        roots = ["host/fxr/" + VERSION + "/", "shared/Microsoft.NETCore.App/" + VERSION + "/"]
        if windows:
            roots.append("shared/Microsoft.WindowsDesktop.App/" + VERSION + "/")
        allowed_runtime = lambda name: name in notices | {"dotnet" + suffix} or any(name.startswith(p) for p in roots)
        for item in inputs[:-1]:
            extract(args.archives / item["file"], runtime, allowed_runtime)
        pack = staging / "apphost-pack"
        host_prefix = "packs/Microsoft.NETCore.App.Host." + args.rid + "/" + VERSION + "/"
        extract(args.archives / inputs[-1]["file"], pack,
                lambda name: name in notices or name.startswith(host_prefix))
        apphost = pack / host_prefix / "runtimes" / args.rid / "native" / ("apphost" + suffix)
        require(apphost.is_file(), "공식 apphost pack 배치 불일치")
        require_x64(apphost, windows)
        core = runtime / roots[1]
        fxr = runtime / roots[0] / ("hostfxr.dll" if windows else "libhostfxr.so")
        require_x64(fxr, windows)
        for name in ["System.Private.CoreLib.dll", "Microsoft.NETCore.App.deps.json",
                     "Microsoft.NETCore.App.runtimeconfig.json", "hostpolicy.dll" if windows else "libhostpolicy.so"]:
            require((core / name).is_file(), "런타임 필수 파일 누락: " + name)
        require(any((runtime / name).is_file() for name in notices if "LICENSE" in name), "런타임 라이선스 누락")
        require(any((runtime / name).is_file() for name in notices if name != "LICENSE.txt"), "런타임 고지 누락")
        if windows:
            desktop = runtime / roots[2]
            for name in ["PresentationFramework.dll", "Microsoft.WindowsDesktop.App.deps.json",
                         "Microsoft.WindowsDesktop.App.runtimeconfig.json"]:
                require((desktop / name).is_file(), "Desktop 런타임 필수 파일 누락: " + name)
        app = "Zipelo.Desktop" if windows else "Zipelo.Ubuntu"
        ui_stage = staging / "ui"
        worker_stage = staging / "worker"
        publish(args.dotnet, ROOT / "src" / app / (app + ".csproj"), ui_stage, apphost,
                args.rid, "runtime", True, args.extraction_validation)
        publish(args.dotnet, ROOT / "src/Zipelo.Worker/Zipelo.Worker.csproj", worker_stage, apphost,
                args.rid, "../runtime", False, False)
        verify_app(ui_stage, "Zipelo" if windows else app, windows, windows)
        verify_app(worker_stage, "Zipelo.Worker", windows, False)
        require(not (ui_stage / "worker").exists() and not (ui_stage / "runtime").exists(), "예전 작업자/런타임 출력 거부")
        shutil.copytree(ui_stage, payload, dirs_exist_ok=True)
        shutil.copytree(worker_stage, payload / "worker")
        for name in ["LICENSE", "THIRD_PARTY_NOTICES.txt"]:
            source = ROOT / name
            require(source.is_file() and not source.is_symlink(), "제품 라이선스/의존성 고지를 검토해 준비해야 함: " + name)
            shutil.copyfile(source, payload / name)
        files = {}
        for path in sorted(payload.rglob("*")):
            require(not path.is_symlink(), "게시 심볼릭 링크 거부")
            if path.is_file():
                require(path.stat().st_nlink == 1, "게시 하드 링크 거부")
                files[path.relative_to(payload).as_posix()] = {"sha256": digest(path), "bytes": path.stat().st_size}
        receipt = {"schema": 1, "rid": args.rid, "runtimeVersion": VERSION,
                   "releaseReady": False, "actualExecutionValidated": False,
                   "extractionValidation": args.extraction_validation,
                   "inputs": inputs, "apphostSourceSha256": digest(apphost), "files": files}
        (payload / "shared-runtime-candidate.json").write_text(json.dumps(receipt, indent=4) + "\n")
        # 기존 출력과 병합하지 않고 새 폴더를 예약한 뒤 완성된 후보만 이동함.
        checked_output, _, _ = validate_paths(args.output, args.archives, args.dotnet)
        require(checked_output == args.output, "검증 후 출력 부모 경로가 변경됨")
        args.output.mkdir()
        try:
            for path in payload.iterdir():
                shutil.move(str(path), args.output / path.name)
        except BaseException:
            shutil.rmtree(args.output)
            raise


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--validation", action="store_true")
    parser.add_argument("--extraction-validation", action="store_true")
    parser.add_argument("--rid", choices=["win-x64", "linux-x64"], required=True)
    parser.add_argument("--archives", type=Path, required=True)
    parser.add_argument("--dotnet", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        assemble(args)
    except (ValueError, OSError, subprocess.CalledProcessError, tarfile.TarError, zipfile.BadZipFile) as error:
        parser.exit(1, "공유 런타임 후보 준비 실패: " + str(error) + "\n")


if __name__ == "__main__":
    main()

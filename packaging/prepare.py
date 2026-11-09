"""검증된 게시 산출물만 설치 초안 입력으로 준비함. 설치 도구는 실행하지 않음."""

import argparse
import hashlib
import json
import re
import shutil
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parent
SYSTEMS = {
    "win-x64": {"windows-11-x64"},
    "linux-x64": {"ubuntu-24.04-x64", "ubuntu-26.04-x64"},
}
CHECKS = {"ui_launch", "worker_protocol", "new_zip_publication", "cancel_cleanup",
          "original_preserved", "license_review", "dependency_review"}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def safe_name(name):
    require(isinstance(name, str) and re.fullmatch(r"[A-Za-z0-9_.\-/]+", name),
            "산출물 경로는 안전한 ASCII 상대 경로여야 함")
    parts = PurePosixPath(name).parts
    require(not name.startswith("/") and all(part not in {".", ".."} for part in parts)
            and str(PurePosixPath(name)) == name, "상대 경로 정책 위반")
    return name


def inventory(payload):
    require(payload.is_dir() and not payload.is_symlink(), "게시 폴더가 필요함")
    files = {}
    for path in sorted(payload.rglob("*")):
        require(not path.is_symlink(), "게시 폴더의 심볼릭 링크는 허용하지 않음")
        if path.is_dir():
            continue
        require(path.is_file() and path.stat().st_nlink == 1, "일반 단일 링크 파일만 허용함")
        name = safe_name(path.relative_to(payload).as_posix())
        files[name] = digest(path)
    return files


def validate(payload, support, verification, platform):
    require(support.get("schema") == 1 and verification.get("schema") == 1, "manifest schema는 1이어야 함")
    require(support.get("releaseReady") is False, "현재 패키지는 개발 검증 초안 전용임")
    version = support.get("version", "")
    require(re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version), "version은 숫자 3부분이어야 함")
    require(verification.get("platform") == platform, "검증 플랫폼이 다름")
    require(verification.get("validationPassed") is True, "실제 실행 검증 기록이 없음")
    require(verification.get("selfContained") is True, "독립 실행 게시 검증이 필요함")
    require(verification.get("creationValidationEnabled") is False, "일반 배포물의 생성 검증 플래그는 꺼야 함")
    require(SYSTEMS[platform] <= set(verification.get("validatedSystems", [])), "OS별 검증 기록이 부족함")
    require(all(verification.get("checks", {}).get(check) is True for check in CHECKS),
            "필수 UI/작업자/게시/취소/원본/라이선스/의존성 검증이 부족함")
    files = inventory(payload)
    require(not any(name.lower() in {"uninstall.exe", ".zipelo-install.ini"} or name.lower().startswith("packaging-meta/") for name in files),
            "설치 관리 파일과 게시 산출물의 경로 충돌")
    require(files and files == verification.get("files"), "검증한 전체 파일 목록 또는 SHA-256이 다름")
    require(len({name.casefold() for name in files}) == len(files), "설치 파일 이름의 대소문자 충돌")
    app = "Zipelo" if platform == "win-x64" else "Zipelo.Ubuntu"
    exe = ".exe" if platform == "win-x64" else ""
    native = ["coreclr.dll", "clrjit.dll", "hostfxr.dll", "hostpolicy.dll"] if exe else [
        "libcoreclr.so", "libclrjit.so", "libhostfxr.so", "libhostpolicy.so"]
    for prefix, stem in [("", app), ("worker/", "Zipelo.Worker")]:
        required = [stem + exe, stem + ".dll", stem + ".runtimeconfig.json", stem + ".deps.json",
                    "Zipelo.Core.dll", "Zipelo.Runtime.dll", "System.Private.CoreLib.dll"] + native
        require(all(prefix + name in files for name in required), "앱/작업자/독립 런타임 필수 파일 누락")
        options = json.loads((payload / (prefix + stem + ".runtimeconfig.json")).read_text())["runtimeOptions"]
        frameworks = options.get("includedFrameworks", [])
        require(frameworks and not options.get("framework") and not options.get("frameworks")
                and all(item.get("version", "").startswith("10.") for item in frameworks),
                "외부 런타임이 필요하거나 .NET 10 독립 실행 구성이 아님")
    require(all(name in files and (payload / name).stat().st_size > 0
                for name in ["LICENSE", "THIRD_PARTY_NOTICES.txt"]), "라이선스와 배포 고지 누락")
    evidence = verification.get("evidence", [])
    require(evidence and all(name in files and name.startswith("validation/")
                            and (payload / name).stat().st_size > 0 for name in evidence), "검증 근거 파일 누락")
    formats = support.get("formats")
    require(isinstance(formats, list) and len(formats) <= 1, "지원 형식 manifest가 잘못됨")
    # 현재 계약은 ZIP만 허용함. JAR과 앱 컨테이너는 확장할 수 없는 제외 대상임.
    for item in formats:
        require(item.get("extension") == ".zip" and item.get("mime") == "application/zip"
                and item.get("validated") is True and item.get("evidence") in evidence,
                "검증되지 않은 형식 또는 제외 형식 등록 요청")
        require(verification.get("fileOpenArgumentsValidated") is True, "실제 시작 인수 파일 열기 검증이 없음")
        require(verification.get("fileOpenArgumentsContract") == "single-local-zip-path-v1",
                "검증한 시작 인수 계약이 패키지 진입 명령과 다름")
    if platform == "linux-x64":
        dependencies = verification.get("dependencies", [])
        atom = r"[a-z0-9][a-z0-9+.-]*(?: \(>= [0-9A-Za-z.+:~\-]+\))?"
        require(dependencies and all(re.fullmatch(atom + r"(?: \| " + atom + ")*", item)
                                     for item in dependencies), "각 LTS에서 검증한 Debian 의존성 목록이 필요함")
    return files


def windows(output, support, files):
    template = (ROOT / "windows/installer.nsi.in").read_text()
    install, remove, rollback, variables = [], [], [], []
    directories = set()
    for index, name in enumerate(sorted(files)):
        path = name.replace("/", "\\")
        parent = str(PurePosixPath(name).parent).replace("/", "\\")
        suffix = "" if parent == "." else "\\" + parent
        flag = f"PayloadOwned{index}"
        variables.append("Var " + flag)
        install.extend(['    ClearErrors', '    SetOutPath "$INSTDIR' + suffix + '"',
                        '    IfErrors install_failed',
                        '    IfFileExists "$INSTDIR\\' + path + '" install_failed',
                        '    StrCpy $' + flag + ' 1', '    ClearErrors',
                        '    File "payload\\' + path + '"', '    IfErrors install_failed'])
        rollback.append('        !insertmacro RemoveOwnedFile "$INSTDIR\\' + path + '" ' + flag)
        remove.append('    Delete "$INSTDIR\\' + path + '"')
        directories.update(str(path).replace("/", "\\") for path in PurePosixPath(name).parents if str(path) != ".")
    create_directories = []
    ordered_directories = sorted(directories, key=lambda value: (value.count("\\"), value))
    for index, directory in enumerate(ordered_directories):
        flag = f"DirectoryOwned{index}"
        variables.append("Var " + flag)
        create_directories.extend([
            """    System::Call 'kernel32::CreateDirectoryW(w "$INSTDIR\\""" + directory + """", p 0) i.r0'""",
            '    IntCmp $0 0 install_failed',
            '    StrCpy $' + flag + ' 1',
        ])
    install = create_directories + install
    for index in reversed(range(len(ordered_directories))):
        directory = ordered_directories[index]
        flag = f"DirectoryOwned{index}"
        remove.append('    RMDir "$INSTDIR\\' + directory + '"')
        rollback.extend([
            '        ' + "$" + '{If} $' + flag + ' == 1',
            '            ClearErrors', '            RMDir "$INSTDIR\\' + directory + '"',
            '            IfErrors 0 +2', '                StrCpy $CleanupFailed 1',
            '        ' + "$" + '{EndIf}',
        ])

    uninstall_key = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Zipelo-Draft"
    writes = [
        (uninstall_key, "DisplayName", '"Zipelo 개발 검증 초안"', "UninstallOwned"),
        (uninstall_key, "DisplayVersion", '"' + support["version"] + '"', "UninstallOwned"),
        (uninstall_key, "UninstallString", r"""'$\"$INSTDIR\Uninstall.exe$\"'""", "UninstallOwned"),
    ]
    owned_roots = [(uninstall_key, "UninstallOwned")]
    shared, register = [], []
    if support["formats"]:
        progid, capabilities = r"Software\Classes\ZipeloDraft.zip", r"Software\ZipeloDraft"
        owned_roots.extend([(progid, "ProgIdOwned"), (capabilities, "CapabilitiesOwned")])
        register.extend(['    !insertmacro ReserveKey "' + progid + '" ProgIdOwned',
                         '    !insertmacro ReserveKey "' + capabilities + '" CapabilitiesOwned'])
        writes.extend([
            (progid, "", '"Zipelo ZIP"', "ProgIdOwned"),
            (progid + r"\shell\open\command", "", r"""'$\"$INSTDIR\Zipelo.exe$\" $\"%1$\"'""", "ProgIdOwned"),
            (capabilities + r"\Capabilities", "ApplicationName", '"Zipelo Development Draft"', "CapabilitiesOwned"),
            (capabilities + r"\Capabilities\FileAssociations", ".zip", '"ZipeloDraft.zip"', "CapabilitiesOwned"),
        ])
        shared = [
            (r"Software\Classes\.zip\OpenWithProgids", "ZipeloDraft.zip", '""', "OpenWithOwned"),
            (r"Software\RegisteredApplications", "Zipelo Development Draft", r'"Software\ZipeloDraft\Capabilities"', "RegisteredAppOwned"),
        ]

    def checked_write(key, value, data):
        return ['    ClearErrors', f'    WriteRegStr HKCU "{key}" "{value}" {data}', '    IfErrors install_failed']

    uninstall_register = []
    for key, value, data, flag in writes:
        (uninstall_register if flag == "UninstallOwned" else register).extend(checked_write(key, value, data))
    for key, value, data, flag in shared:
        register.extend([f'    !insertmacro RequireAbsentValue "{key}" "{value}"',
                         '    StrCpy $' + flag + ' 1'] + checked_write(key, value, data))

    logic = lambda name: "$" + "{" + name + "}"
    cleanup, unregister = [], []
    for key, value, _, flag in reversed(shared + writes):
        unregister.append(f'    DeleteRegValue HKCU "{key}" "{value}"')
        cleanup.extend([
            '    ' + logic("If") + ' $' + flag + ' == 1',
            '        ClearErrors',
            f'        DeleteRegValue HKCU "{key}" "{value}"',
            '        ClearErrors',
            f"""        System::Call 'advapi32::RegOpenKeyExW(p 0x80000001, w "{key}", i 0, i 0x20119, *p.r1) i.r0'""",
            '        ' + logic("If") + ' $0 == 0',
            f"""            System::Call 'advapi32::RegQueryValueExW(p r1, w "{value}", p 0, p 0, p 0, p 0) i.r2'""",
            """            System::Call 'advapi32::RegCloseKey(p r1) i.r0'""",
            '            ' + logic("If") + ' $2 != 2',
            '            ' + logic("OrIf") + ' $0 != 0',
            '                StrCpy $CleanupFailed 1',
            '            ' + logic("EndIf"),
            '        ' + logic("ElseIf") + ' $0 != 2',
            '            StrCpy $CleanupFailed 1',
            '        ' + logic("EndIf"),
            '    ' + logic("EndIf"),
        ])
    for root, flag in reversed(owned_roots):
        keys = {root}
        for key, _, _, write_flag in writes:
            if write_flag == flag:
                while key != root:
                    keys.add(key)
                    key = key.rsplit("\\", 1)[0]
        for key in sorted(keys, key=lambda value: (value.count("\\"), value), reverse=True):
            unregister.append(f'    DeleteRegKey /ifempty HKCU "{key}"')
            cleanup.extend(['    ' + logic("If") + ' $' + flag + ' == 1',
                            '        ClearErrors', f'        DeleteRegKey /ifempty HKCU "{key}"',
                            '        IfErrors 0 +2', '            StrCpy $CleanupFailed 1',
                            '    ' + logic("EndIf")])
    values = {
        "VERSION": support["version"], "VARS": "\n".join(variables),
        "FILES": "\n".join(install), "DELETE": "\n".join(remove),
        "ROLLBACK_FILES": "\n".join(rollback), "ROLLBACK_REGISTER": "\n".join(cleanup),
        "UNINSTALL_REGISTER": "\n".join(uninstall_register),
        "REGISTER": "\n".join(register), "UNREGISTER": "\n".join(unregister),
    }
    for key, value in values.items():
        template = template.replace("@" + key + "@", value)
    require(not re.search(r"@[A-Z_]+@", template), "NSIS 템플릿 토큰이 남아 있음")
    (output / "installer.nsi").write_text(template, encoding="utf-8-sig")


def ubuntu(output, support, verification, maintainer):
    require(re.fullmatch(r"[A-Za-z][A-Za-z0-9 .-]+ <[A-Za-z0-9_.+\-]+@[A-Za-z0-9.\-]+>", maintainer),
            "실제 승인된 Maintainer 이름과 이메일이 필요함")
    stage = output / "deb-root"
    stage.mkdir()
    (stage / "usr/lib").mkdir(parents=True)
    shutil.move(str(output / "payload"), str(stage / "usr/lib/zipelo"))
    for source, target in [("zipelo", "usr/bin/zipelo"), ("zipelo.svg", "usr/share/icons/hicolor/scalable/apps/zipelo.svg")]:
        target = stage / target
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / "ubuntu" / source, target)
    desktop = (ROOT / "ubuntu/zipelo.desktop").read_text()
    if support["formats"]:
        desktop = desktop.replace("Exec=/usr/bin/zipelo", "Exec=/usr/bin/zipelo %f")
        desktop += "MimeType=application/zip;\n"
    target = stage / "usr/share/applications/zipelo-draft.desktop"
    target.parent.mkdir(parents=True)
    target.write_text(desktop)
    control = (ROOT / "ubuntu/control.in").read_text().replace("@VERSION@", support["version"] + "~draft1")
    dependencies = sorted(set(verification["dependencies"]) | {"shared-mime-info", "desktop-file-utils"})
    control = control.replace("@MAINTAINER@", maintainer).replace("@DEPENDS@", ", ".join(dependencies))
    (stage / "DEBIAN").mkdir()
    (stage / "DEBIAN/control").write_text(control)
    # 다음 명령은 승인된 준비 실행에서 패키지 내부 모드만 설정함. 설치하거나 소유자를 변경하지 않음.
    for path in stage.rglob("*"):
        path.chmod(0o755 if path.is_dir() else 0o644)
    for name in ["usr/bin/zipelo", "usr/lib/zipelo/Zipelo.Ubuntu", "usr/lib/zipelo/worker/Zipelo.Worker"]:
        (stage / name).chmod(0o755)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", required=True, choices=SYSTEMS)
    parser.add_argument("--payload", required=True, type=Path)
    parser.add_argument("--support", required=True, type=Path)
    parser.add_argument("--verification", required=True, type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--maintainer", default="")
    parser.add_argument("--check-only", action="store_true")
    args = parser.parse_args()
    try:
        support = json.loads(args.support.read_text())
        verification = json.loads(args.verification.read_text())
        files = validate(args.payload, support, verification, args.platform)
        if args.check_only:
            print("입력 검증 통과. 설치 패키지 실행 검증 결과가 아님.")
            return
        require(args.output is not None and not args.output.exists(), "새 출력 폴더가 필요함")
        args.output.mkdir(parents=True)
        shutil.copytree(args.payload, args.output / "payload")
        require(inventory(args.output / "payload") == files, "복사 중 게시 산출물이 바뀜")
        metadata = args.output / "payload/packaging-meta"
        require(not metadata.exists(), "예약 메타데이터 경로 충돌")
        metadata.mkdir()
        (metadata / "support.json").write_text(json.dumps(support, indent=4) + "\n")
        (metadata / "verification.json").write_text(json.dumps(verification, indent=4) + "\n")
        if args.platform == "win-x64":
            windows(args.output, support, inventory(args.output / "payload"))
        else:
            ubuntu(args.output, support, verification, args.maintainer)
        print("설치 초안 입력 준비 완료. 컴파일/설치/서명은 실행하지 않음.")
    except (ValueError, OSError, KeyError, TypeError) as error:
        parser.exit(2, f"패키지 준비 중단: {error}\n")


if __name__ == "__main__":
    main()

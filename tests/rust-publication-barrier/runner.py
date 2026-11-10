import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import sys
import uuid

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parent
PROJECT = ROOT.parent.parent
BASE = ROOT.parent / "rust-zip-worker-probe"
WORKER_COMMIT = "9626abbf22f3a4c66a996e64601694ed5fe98763"
WORKER_HASH = "62e157e304b01cac7c99a8eb60a147066877cf141b0314db5cf1a76aab55a5f7"
INPUT_HASH = "a6e206b9f9fefd406d765562664ce023aa30dcacf66e942e264e276d95600b51"
MODES = ("control", "cancel-new", "cancel-existing")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    with Path(path).open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def no_links(path):
    path = Path(path)
    require(path.is_absolute() and ".." not in path.parts, "정규 절대 경로가 필요함")
    for part in (path, *path.parents):
        require(not part.is_symlink(), "링크 경로 금지")
    return path


def read_json(path, limit=1024 * 1024):
    path = no_links(path)
    require(path.is_file() and path.stat().st_size <= limit, "JSON 크기 또는 종류 오류")
    return json.loads(path.read_text(encoding="utf-8"))


def write_json(path, value):
    with Path(path).open("x", encoding="utf-8") as output:
        json.dump(value, output, ensure_ascii=False, indent=4)
        output.write("\n")


def checked(record):
    path = no_links(record["path"])
    require(path.is_file() and re.fullmatch(r"[0-9a-f]{64}", record["sha256"]), "실행 파일 명세 오류")
    require(digest(path) == record["sha256"], "파일 해시 불일치: " + str(path))
    return path


def check_manifest(root, expected):
    require(digest(root / "SHA256SUMS") == expected, "소스 manifest 불일치")
    seen = set()
    for line in (root / "SHA256SUMS").read_text(encoding="ascii").splitlines():
        sha, name = line.split("  ", 1)
        require(name not in seen and not Path(name).is_absolute() and ".." not in Path(name).parts,
                "manifest 경로 오류")
        seen.add(name)
        checked({"path": str(root / name), "sha256": sha})


def check_sources(settings):
    check_manifest(ROOT, settings["sourceManifestSha256"])
    provenance = read_json(ROOT / "provenance.json")
    require(provenance["baseCommit"] == "dde29069f89016111c3a023a89530045190dcb13", "기준 commit 오류")
    check_manifest(BASE, provenance["frozenManifestSha256"])
    for item in [*provenance["linkedFiles"], provenance["supervisor"]]:
        checked({"path": str(PROJECT / item["path"]), "sha256": item["sha256"]})
    derived = provenance["derived"]
    original = checked({"path": str(PROJECT / derived["originalPath"]), "sha256": derived["originalSha256"]})
    text = original.read_bytes()
    injection = b"        Zipelo.PublicationBarrierTests.Barrier.Reach(TemporaryName, leaf, token);\n"
    marker = "        token.ThrowIfCancellationRequested();\n        // RENAME_NOREPLACE".encode()
    require(text.count(marker) == 1 and (ROOT / derived["path"]).read_bytes() ==
            text.replace(marker, injection + marker), "단일 주입 차이 불일치")


def config(path):
    value = read_json(path)
    require(sys.platform == "linux" and value["schemaVersion"] == 1, "Linux 시험 설정 필요")
    check_sources(value)
    require(re.fullmatch(r"[0-9a-f]{40}", value["host"]["sourceCommit"]), "파생 host commit 필요")
    require(value["worker"]["sourceCommit"] == WORKER_COMMIT and
            value["worker"]["sha256"] == WORKER_HASH, "검증한 Rust 바이너리 고정 필요")
    require(Path(value["worker"]["path"]).suffix.lower() != ".dll", "native Rust worker 필요")
    require(Path(value["dotnet"]["path"]).name == "dotnet" and
            Path(value["host"]["path"]).name == "Zipelo.PublicationBarrierProbe.dll", "호출 방식 불일치")
    artifacts = value["artifacts"]
    require(0 < len(artifacts) <= 256, "전체 실행 산출물 해시 목록 필요")
    by_path = {}
    for entry in artifacts:
        path = checked(entry)
        require(str(path) not in by_path, "산출물 중복")
        by_path[str(path)] = entry["sha256"]
    for name in ("dotnet", "host", "worker"):
        entry = value[name]
        path = checked(entry)
        require(by_path.get(str(path)) == entry["sha256"], "호출 산출물 해시 누락")
    host = Path(value["host"]["path"])
    for name in ("Zipelo.PublicationBarrierProbe.deps.json", "Zipelo.PublicationBarrierProbe.runtimeconfig.json"):
        require(str(host.parent / name) in by_path, "managed 의존 명세 누락")
    for path in host.parent.iterdir():
        require(not path.is_symlink(), "빌드 산출물 링크 금지")
        if path.is_file():
            require(str(path) in by_path, "빌드 디렉터리 파일 해시 누락: " + path.name)
    for other in ("dotnet", "host"):
        require(not os.path.samefile(value[other]["path"], value["worker"]["path"]) and
                value[other]["sha256"] != WORKER_HASH, "실제 후보 구분 실패")
    for name in ("LD_PRELOAD", "LD_AUDIT", "DOTNET_STARTUP_HOOKS", "DOTNET_ADDITIONAL_DEPS", "DOTNET_SHARED_STORE"):
        require(not os.environ.get(name), "외부 주입 환경을 허용하지 않음: " + name)
    return value


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", required=True)
    parser.add_argument("--work-parent", required=True)
    args = parser.parse_args()
    settings = config(Path(args.config))
    parent = no_links(args.work_parent)
    require(parent.is_dir() and parent.is_relative_to(ROOT), "새 시험 폴더 아래 기존 소유 부모 필요")
    require(parent.stat().st_uid == os.geteuid() and not parent.stat().st_mode & 0o022,
            "시험 부모 소유권 또는 쓰기 권한 오류")
    area = parent / ("run-" + uuid.uuid4().hex)
    area.mkdir(mode=0o700)
    spec = importlib.util.spec_from_file_location("publication_process_scope", BASE / "process_scope.py")
    scope = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(scope)
    report = {
        "scope": "고정 Rust worker + 파생 시험 host",
        "hostSourceCommit": settings["host"]["sourceCommit"],
        "sourceManifestSha256": settings["sourceManifestSha256"],
        "worker": settings["worker"],
        "inputSha256": INPUT_HASH,
        "cases": [],
        "stopRequired": True,
        "frozenUnverifiedCount": 7,
        "performanceGate": "not_evaluated",
        "fullAcceptance": False,
        "windowsExecution": "unverified",
    }
    try:
        for mode in MODES:
            for entry in settings["artifacts"]:
                checked(entry)
            child = area / mode
            child.mkdir(mode=0o700)
            worker = settings["worker"]
            invocation = {
                "kind": "native", "executable": worker["path"], "executableSha256": WORKER_HASH,
                "worker": worker["path"], "workerSha256": WORKER_HASH, "arguments": ["--extract-stream-v2"],
            }
            write_json(child / "plan.json", {
                "mode": mode, "runDirectory": str(child), "invocation": invocation,
            })
            supervised = scope.bounded([
                settings["dotnet"]["path"], settings["host"]["path"], "--gate", str(child / "gate"),
                "--plan", str(child / "plan.json"),
            ], child, child / "gate", 75)
            record = {"mode": mode, "supervisor": supervised}
            report["cases"].append(record)
            observed = read_json(child / "observation.json")
            record["observation"] = observed
            safe = (supervised["exitCode"] == 0 and supervised["ownedProcessesExited"] and
                    not supervised["externalTimeout"] and not supervised["forcedTermination"] and
                    observed["verdict"] == "pass" and observed["stopRequired"] is False and
                    observed["mode"] == mode and observed["workerSha256"] == WORKER_HASH and
                    observed["inputSha256"] == INPUT_HASH and all(observed[key] is True for key in
                    ("preservationConfirmed", "cleanupConfirmed", "workerExitConfirmed",
                     "diagnosticsComplete", "barrierConfirmed")))
            for entry in settings["artifacts"]:
                checked(entry)
            if not safe:
                break
        report["stopRequired"] = len(report["cases"]) != len(MODES) or not safe
    except Exception as error:
        report["failure"] = type(error).__name__ + ": " + str(error)
        report["stopRequired"] = True
    write_json(area / "summary.json", report)
    print(str(area / "summary.json"))
    return 124 if report["stopRequired"] else 0


if __name__ == "__main__":
    raise SystemExit(main())

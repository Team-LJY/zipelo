import argparse
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import statistics
import sys
import time
import uuid
import zlib

from process_scope import bounded


ROOT = Path(__file__).resolve().parent
HEX256 = re.compile(r"[0-9a-f]{64}")
HEX160 = re.compile(r"[0-9a-f]{40}")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def no_links(path):
    path = Path(path).absolute()
    for item in (path, *path.parents):
        require(not item.is_symlink(), "링크 경로를 허용하지 않음")
        if os.name == "nt" and item.exists():
            require(not item.lstat().st_file_attributes & 0x400, "reparse 경로를 허용하지 않음")
    return path


def read_json(path, limit=16 * 1024 * 1024):
    path = no_links(path)
    require(path.is_file() and path.stat().st_size <= limit, "JSON 크기 또는 종류 오류")
    return json.loads(path.read_text(encoding="utf-8"))


def write_json(path, value):
    with Path(path).open("x", encoding="utf-8") as output:
        json.dump(value, output, ensure_ascii=False, indent=4)
        output.write("\n")


def checked_file(record):
    path = no_links(record["path"])
    require(path.is_absolute() and path.is_file(), "기존 일반 파일의 절대 경로가 필요함")
    require(HEX256.fullmatch(record["sha256"]) is not None, "SHA-256 형식 오류")
    require(digest(path.read_bytes()) == record["sha256"], "실행 입력 해시 불일치")
    return path


def check_sources():
    lines = (ROOT / "SHA256SUMS").read_text(encoding="ascii").splitlines()
    for line in lines:
        expected, relative = line.split("  ", 1)
        path = no_links(ROOT / relative)
        require(path.is_relative_to(ROOT) and path.is_file(), "소스 경로 오류")
        require(digest(path.read_bytes()) == expected, "고정 소스 변경: " + relative)


def validate_candidates(value):
    candidates = {item["name"]: item for item in value["implementations"]}
    csharp, rust = candidates["csharp"], candidates["rust"]
    require(csharp["invocationKind"] == "dotnet-managed" and
            Path(csharp["path"]).suffix.lower() == ".dll", "C# managed DLL 호출 필요")
    require(rust["invocationKind"] == "native" and
            Path(rust["path"]).suffix.lower() != ".dll", "Rust native 실행 필요")
    require(Path(value["dotnet"]["path"]).stem.lower() == "dotnet", "고정 dotnet 실행파일 필요")
    pairs = [(csharp, rust), (rust, value["dotnet"]), (rust, value["host"]), (csharp, value["host"])]
    for left, right in pairs:
        require(not os.path.samefile(left["path"], right["path"]), "서로 다른 실제 후보 파일 필요")
        require(left["sha256"] != right["sha256"], "동일 바이트 후보를 별개 구현으로 비교할 수 없음")


def invocation(implementation, settings, operation):
    managed = implementation["invocationKind"] == "dotnet-managed"
    arguments = [implementation["path"]] if managed else []
    if operation != "list":
        arguments.append("--extract-stream-v2")
    executable = settings["dotnet"] if managed else implementation
    return {
        "kind": implementation["invocationKind"], "executable": executable["path"],
        "executableSha256": executable["sha256"], "worker": implementation["path"],
        "workerSha256": implementation["sha256"], "arguments": arguments,
    }


def run_schedule(jobs, execute):
    for job in jobs:
        record = execute(job)
        yield record
        if record["stopRequired"]:
            return


def config(path):
    value = read_json(path)
    require(value["schemaVersion"] == 1, "설정 버전 오류")
    for key in ("dotnet", "host", "corpus"):
        checked_file(value[key])
    require({x["name"] for x in value["implementations"]} == {"csharp", "rust"} and
            len(value["implementations"]) == 2, "두 후보가 필요함")
    for item in value["implementations"]:
        checked_file(item)
        require(HEX160.fullmatch(item["sourceCommit"]) is not None, "후보 commit 오류")
    validate_candidates(value)
    require(HEX256.fullmatch(value["sourceManifestSha256"]) is not None, "소스 manifest 해시 필요")
    require(value["sourceManifestSha256"] == digest((ROOT / "SHA256SUMS").read_bytes()), "소스 manifest 불일치")
    require(HEX160.fullmatch(value["host"]["sourceCommit"]) is not None, "host commit 오류")
    artifacts = value["artifacts"]
    require(len(artifacts) > 0, "빌드 산출물 전체 목록 필요")
    for artifact in artifacts:
        checked_file(artifact)
    # 동일 폴더의 로드 가능한 산출물은 빠짐없이 고정함.
    by_path = {str(no_links(x["path"])): x["sha256"] for x in artifacts}
    for key in ("host", "corpus"):
        for item in Path(value[key]["path"]).parent.iterdir():
            if item.is_file() and (item.suffix in (".dll", ".so", ".dylib", ".exe", ".json") or item.suffix == ""):
                require(str(no_links(item)) in by_path, "산출물 명세 누락: " + item.name)
    csharp = next(x for x in value["implementations"] if x["name"] == "csharp")
    for item in Path(csharp["path"]).parent.iterdir():
        if item.is_file() and (item.suffix in (".dll", ".so", ".dylib", ".exe", ".json") or item.suffix == ""):
            require(str(no_links(item)) in by_path, "C# 산출물 명세 누락: " + item.name)
    helper = value.get("nativeNoExit")
    if helper is not None:
        require(sys.platform == "linux" and helper["invocationKind"] == "native" and
                Path(helper["path"]).suffix.lower() != ".dll", "Linux native no-exit helper만 허용함")
        checked_file(helper)
        require(helper["sourceCommit"] == value["host"]["sourceCommit"], "helper와 host source commit 불일치")
        require(by_path.get(str(no_links(helper["path"]))) == helper["sha256"], "helper 산출물 해시 누락")
        for other in [value["dotnet"], value["host"], value["corpus"], *value["implementations"]]:
            require(not os.path.samefile(helper["path"], other["path"]) and
                    helper["sha256"] != other["sha256"], "별도 no-exit helper 실행파일 필요")
    build = value["rustBuild"]
    require(build["toolchain"] and build["target"] and isinstance(build["features"], list), "Rust 구성 필요")
    for key in ("manifestSha256", "lockSha256"):
        require(HEX256.fullmatch(build[key]) is not None, "실제 Cargo 구성 해시 필요")
    return value


def run_root(parent):
    parent = no_links(parent)
    require(parent.is_dir() and parent.is_relative_to(ROOT), "시험 경로 안의 기존 작업 부모가 필요함")
    path = parent / ("run-" + uuid.uuid4().hex)
    path.mkdir(mode=0o700)
    return path


def expected(name, data, kind="File"):
    return {"name": name, "kind": kind, "length": len(data), "sha256": digest(data),
            "crc32": zlib.crc32(data)}


def factory_child(gate, output):
    deadline = time.monotonic() + 10
    while not Path(gate).exists() and time.monotonic() < deadline:
        time.sleep(0.01)
    require(Path(gate).read_text(encoding="ascii") == "1", "외부 감독 장벽 필요")
    path = ROOT / "frozen/tests/extraction/fixture_factory.py"
    spec = importlib.util.spec_from_file_location("frozen_zipelo_fixture_factory", path)
    factory = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(factory)
    samples = factory.build_samples()
    manifest = read_json(ROOT / "frozen/tests/extraction/fixtures.json")
    require(factory.build_manifest(samples) == manifest, "원본 명세와 다른 생성 환경임")
    normal = [
        expected("hello.txt", factory.PAYLOAD), expected("nested/", b"", "Directory"),
        expected("nested/data.bin", bytes(range(32))), expected("nested/empty.txt", b""),
    ]
    entries = {
        "normal-stored": normal, "normal-deflated": normal,
        "duplicate": [expected("same.txt", b"one"), expected("same.txt", b"two")],
        "case-distinct": [expected("Readme.txt", b"first"), expected("README.TXT", b"second")],
        "linux-names": [expected("CON", b"one"), expected("name:stream", b"two")],
        "absolute-drive": [expected("C:/escape.txt", factory.PAYLOAD)],
        "backslash-traversal": [expected("..\\escape.txt", factory.PAYLOAD)],
        "bad-payload-crc": [expected("payload.txt", factory.PAYLOAD)],
        "cumulative": [expected("first.bin", bytes(range(8))), expected("second.bin", bytes(range(8, 16)))],
    }
    bundle = []
    for filename, data in samples.items():
        name = filename[:-4]
        bundle.append({"id": name, "base64": base64.b64encode(data).decode("ascii"),
                       "sha256": digest(data), "entries": entries.get(name, [])})
    write_json(output, bundle)


def prepare(args, settings):
    root = run_root(args.work_parent)
    python_output = root / "python-corpus.json"
    python_gate = root / "python-gate"
    python_observed = bounded(
        [sys.executable, str(ROOT / "runner.py"), "--factory-child", str(python_gate), str(python_output)],
        root, python_gate
    )
    require(python_observed["exitCode"] == 0 and not python_observed["forcedTermination"] and
            python_observed["ownedProcessesExited"], "Python fixture 생성 또는 종료 미확인")
    corpus = root / "dotnet-corpus.json"
    gate = root / "gate"
    command = [settings["dotnet"]["path"], settings["corpus"]["path"],
               "--gate", str(gate), "--output", str(corpus)]
    observed = bounded(command, root, gate)
    require(observed["exitCode"] == 0 and not observed["forcedTermination"] and
            observed["ownedProcessesExited"], "corpus 생성 또는 종료 미확인")
    bundle = read_json(python_output)
    bundle.extend(read_json(corpus))
    validate_bundle(bundle)
    target = root / "bundle.json"
    write_json(target, bundle)
    write_json(root / "preparation.json", {
        "sourceManifestSha256": settings["sourceManifestSha256"], "bundleSha256": digest(target.read_bytes()),
        "python": sys.version, "zlib": zlib.ZLIB_RUNTIME_VERSION, "config": settings,
        "generatedSamples": len(bundle), "process": observed, "pythonProcess": python_observed,
    })
    print(json.dumps({"bundle": str(target), "sha256": digest(target.read_bytes()), "samples": len(bundle)}))


def validate_bundle(bundle):
    require(isinstance(bundle, list) and len(bundle) == 37, "고정 37개 입력이 필요함")
    require(len({item["id"] for item in bundle}) == len(bundle), "표본 ID 중복")
    for item in bundle:
        require(len(item["base64"]) <= 2 * 1024 * 1024, "base64 상한 초과")
        data = base64.b64decode(item["base64"], validate=True)
        require(len(data) <= 1024 * 1024 and digest(data) == item["sha256"], "표본 해시 불일치")


def child(case, implementation, bundle, settings, root, ordinal):
    area = root / f"case-{ordinal:04d}"
    area.mkdir(mode=0o700)
    plan = {
        "case": case, "sample": bundle[case["fixture"]], "implementation": implementation["name"],
        "worker": implementation["path"], "workerSha256": implementation["sha256"],
        "dotnet": settings["dotnet"]["path"], "dotnetSha256": settings["dotnet"]["sha256"],
        "sourceCommit": implementation["sourceCommit"],
        "invocation": invocation(implementation, settings, case["operation"]),
        "runDirectory": str(area),
    }
    write_json(area / "plan.json", plan)
    gate = area / "gate"
    command = [settings["dotnet"]["path"], settings["host"]["path"],
               "--gate", str(gate), "--plan", str(area / "plan.json")]
    supervised = bounded(command, area, gate)
    result_path = area / "observation.json"
    result = read_json(result_path, 1024 * 1024) if result_path.exists() else {
        "caseId": case["id"], "implementation": implementation["name"],
        "verdict": "fail", "detail": "missing_observation", "stopRequired": True,
    }
    if result_path.exists():
        schema = read_json(ROOT / "result.schema.json")
        require(set(result) == set(schema["required"]), "결과 필드 누락 또는 추가")
        require(result["verdict"] in ("pass", "fail", "blocked"), "결과 판정 오류")
        for key, rule in schema["properties"].items():
            if rule.get("type") == "boolean":
                require(type(result[key]) is bool, "결과 bool 형식 오류")
    require(result["caseId"] == case["id"] and result["implementation"] == implementation["name"],
            "결과 identity 불일치")
    if result_path.exists():
        require(result["invocation"] == plan["invocation"], "실행 결과의 호출 계획 불일치")
        require(result["binarySha256"] == implementation["sha256"], "결과 후보 해시 불일치")
        if case["operation"] == "list" or result.get("workerStarted") is True:
            require(result["identityConfirmed"], "실제 실행 identity 미확인")
    result["supervisor"] = supervised
    result["runDirectory"] = str(area)
    if supervised["forcedTermination"] or not supervised["ownedProcessesExited"] or supervised["exitCode"] == 124:
        result.update(verdict="fail", stopRequired=True, detail="external_timeout_or_cleanup_unconfirmed")
    elif result["verdict"] == "pass" and supervised["exitCode"] != 0:
        result.update(verdict="fail", stopRequired=True, detail="exit_result_mismatch")
    if result.get("workerExitConfirmed") is None and supervised["ownedProcessesExited"]:
        result["externalWorkerExitConfirmed"] = True
    return result


def comparison(records):
    paired = {}
    for record in records:
        paired.setdefault(record["caseId"], {})[record["implementation"]] = record
    differences = []
    keys = ("state", "error", "published", "inputUnchanged", "existingTargetUnchanged",
            "cleanupConfirmed", "outputBytes", "entries")
    for case_id, pair in paired.items():
        if {"csharp", "rust"} <= pair.keys():
            changed = [key for key in keys if pair["csharp"].get(key) != pair["rust"].get(key)]
            if changed:
                differences.append({"caseId": case_id, "fields": changed})
    return differences


def run(args, settings):
    bundle_path = no_links(args.bundle)
    require(bundle_path.is_file() and bundle_path.stat().st_size <= 16 * 1024 * 1024, "bundle 크기/종류 오류")
    require(digest(bundle_path.read_bytes()) == args.bundle_sha256, "승인된 공통 bundle 해시 불일치")
    require(bundle_path.stat().st_size <= 16 * 1024 * 1024, "bundle 크기 초과")
    items = read_json(bundle_path)
    validate_bundle(items)
    bundle = {item["id"]: item for item in items}
    cases = read_json(ROOT / "cases.json")
    require(all(case["fixture"] in bundle for case in cases), "표본 연결 누락")
    root = run_root(args.work_parent)
    records = []
    implementations = {item["name"]: item for item in settings["implementations"]}
    jobs = []
    for case in cases:
        targets = ["csharp", "rust"] if case["operation"] not in ("peer", "blocked") else ["csharp"]
        for name in targets:
            item = dict(implementations[name])
            if case["operation"] == "peer":
                item = {**item, "name": "host-peer", "path": settings["host"]["path"],
                        "sha256": settings["host"]["sha256"], "sourceCommit": settings["host"]["sourceCommit"]}
                if case["id"] == "peer.deadline-no-exit" and sys.platform == "linux" and settings.get("nativeNoExit"):
                    item = {**settings["nativeNoExit"], "name": "host-peer-native"}
            if case["operation"] == "blocked":
                item["name"] = "contract"
            jobs.append((case, item))
    def execute(job):
        case, item = job
        record = child(case, item, bundle, settings, root, len(records))
        print(case["id"] + " " + item["name"] + " " + record["verdict"], flush=True)
        return record
    for record in run_schedule(jobs, execute):
        records.append(record)
    check_sources()
    config(args.config)
    summary = {
        "schemaVersion": 1, "mode": "functional", "sourceManifestSha256": settings["sourceManifestSha256"],
        "bundleSha256": args.bundle_sha256, "config": settings,
        "registeredConditions": len(cases), "plannedObservations": 137,
        "observed": len(records), "remaining": 137 - len(records), "records": records,
        "differences": comparison(records),
        "fullAcceptance": len(records) == 137 and all(r["verdict"] == "pass" for r in records),
        "windowsCleanupSequence24": "unverified",
    }
    write_json(root / "summary.json", summary)
    if args.performance:
        require(summary["fullAcceptance"], "필수 기능 수용 완료 전 성능 모드를 허용하지 않음")
        performance(settings, bundle, root, records)
    return 0 if summary["fullAcceptance"] else 1


def performance(settings, bundle, root, functional):
    # 전체 필수 수용 뒤 같은 호스트와 보호 경로로만 측정함.
    timings = []
    cases = read_json(ROOT / "cases.json")
    selected = [c for c in cases if c["id"] in ("list.empty", "extract.deflate-chunk")]
    ordinal = len(functional)
    for case in selected:
        for turn in range(6):
            order = settings["implementations"] if turn % 2 == 0 else list(reversed(settings["implementations"]))
            for implementation in order:
                result = child(case, implementation, bundle, settings, root, ordinal)
                ordinal += 1
                require(result["verdict"] == "pass" and not result["stopRequired"], "성능 중 기능 실패")
                if turn > 0:
                    timings.append({
                        "caseId": case["id"], "implementation": implementation["name"],
                        "seconds": result["supervisor"]["elapsedSeconds"],
                        "sampledPeakRssBytes": result["supervisor"]["sampledPeakRssBytes"],
                        "outputBytes": result["outputBytes"],
                    })
    groups = []
    for case in selected:
        for implementation in settings["implementations"]:
            observations = [r for r in timings if r["caseId"] == case["id"] and
                            r["implementation"] == implementation["name"]]
            seconds = [r["seconds"] for r in observations]
            groups.append({
                "caseId": case["id"], "implementation": implementation["name"], "runs": observations,
                "medianSeconds": statistics.median(seconds), "minSeconds": min(seconds), "maxSeconds": max(seconds),
            })
    write_json(root / "performance.json", {
        "groups": groups, "idleRam": None, "idleReason": "one_shot_worker_has_no_stable_idle_phase",
        "installedBytes": None, "installedReason": "requires_complete_distribution_inventory",
        "measurement": "process_launch_to_host_validation_publication_cleanup_and_exit",
        "cache": "uncontrolled_no_cold_cache_claim", "rss": "sampled_sum_shared_pages_may_be_counted_twice",
    })


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("prepare", "run"))
    parser.add_argument("--config", required=True)
    parser.add_argument("--work-parent", required=True)
    parser.add_argument("--bundle")
    parser.add_argument("--bundle-sha256")
    parser.add_argument("--performance", action="store_true")
    args = parser.parse_args()
    require(args.mode != "run" or (args.bundle and HEX256.fullmatch(args.bundle_sha256 or "")), "bundle 해시 필요")
    check_sources()
    settings = config(args.config)
    if args.mode == "prepare":
        prepare(args, settings)
        return 0
    return run(args, settings)


if __name__ == "__main__":
    try:
        if len(sys.argv) == 4 and sys.argv[1] == "--factory-child":
            factory_child(sys.argv[2], sys.argv[3])
            sys.exit(0)
        sys.exit(main())
    except Exception as error:
        print("SETUP_OR_SUPERVISOR_FAILURE:" + type(error).__name__ + ":" + str(error), file=sys.stderr)
        sys.exit(2)

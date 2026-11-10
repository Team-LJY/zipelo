import argparse
import hashlib
import json
from pathlib import Path
import re
import stat
import tomllib

ROOT = Path(__file__).resolve().parents[2]
BASE = Path(__file__).resolve().parent

def require(value, message):
    if not value:
        raise ValueError(message)

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def verify():
    pins = json.loads((BASE / "source-pins.json").read_text())
    require(pins["head"] == "65f59f7f119605ab1643b23c73c3e9ef8be43926", "고정 소스 변경")
    require(len(pins["files"]) == 14, "입력 수 변경")
    for row in pins["files"]:
        path = ROOT / row["path"]
        require(path.resolve() == path and stat.S_ISREG(path.lstat().st_mode), "입력 경로 변경")
        raw = path.read_bytes()
        blob = hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest()
        mode = "100755" if path.stat().st_mode & 0o111 else "100644"
        require((blob, mode, digest(path)) == (row["sha"], row["mode"], row["sha256"]), row["path"])
    contract = json.loads((BASE / "contract.json").read_text())
    listed = []
    for line in (BASE / "SHA256SUMS").read_text().splitlines():
        expected, relative = line.split("  ", 1)
        path = ROOT / relative
        require(path.resolve() == path and path.parent == BASE, "manifest 경로 이탈")
        require(digest(path) == expected, relative)
        listed.append(path.name)
    require(sorted(listed) == sorted(set(contract["deliverables"]) - {"SHA256SUMS"}), "manifest 집합 변경")
    names = []
    for module in ("wire", "limits", "lifecycle"):
        names.extend(module + "::" + n for n in re.findall(
            r"#\[test\]\s*fn\s+(\w+)\s*\(", (BASE / (module + ".rs")).read_text()))
    require(len(names) == len(set(names)) == 48, "추가 선언 수 변경")
    require(sorted(names) == contract["additional_names"], "추가 시험 이름 변경")
    original = re.findall(r"#\[test\]\s*fn\s+(\w+)\s*\(",
                          (ROOT / "tests/rust-worker-session-model/main.rs").read_text())
    require(len(original) == len(set(original)) == 30, "원래 선언 수 변경")
    require(sorted(original) == contract["baseline_names"], "원래 시험 이름 변경")
    package = tomllib.loads((BASE / "Cargo.toml").read_text())
    require(package["package"]["autotests"] is False and package["package"]["publish"] is False, "자동 target 금지")
    require(package["dependencies"] == {
        "zipelo-rust-worker-session-model": {"path": "../../experiments/rust-worker-session-model"}
    }, "의존성 변경")
    require(package["test"] == [
        {"name": "worker_session_baseline", "path": "../rust-worker-session-model/main.rs"},
        {"name": "worker_session_regressions", "path": "main.rs"},
    ], "target 경계 변경")
    require(not package.get("build-dependencies") and not (BASE / "build.rs").exists(), "build script 금지")
    lock = tomllib.loads((BASE / "Cargo.lock").read_text())
    require(len(lock["package"]) == 2 and not any("source" in p for p in lock["package"]), "외부 의존성 금지")
    source = "\n".join((BASE / n).read_text() for n in contract["deliverables"] if n.endswith(".rs"))
    require(not re.search(r"#\[ignore|should_panic|std::(?:fs|process|net|os)|Command::|extern\s|unsafe\s*\{|include!|include_bytes!|thread::|sleep\(", source), "금지된 시험 경계")
    model = (ROOT / "experiments/rust-worker-session-model/src/model.rs").read_text()
    for method in ("live_verified", "publication_allowed"):
        require(re.search(r"pub fn " + method + r"\(&self\) -> bool\s*\{\s*false\s*\}", model), "권한 불변식 변경")
    plan = json.loads((BASE / "run-plan.json").read_text())
    require(plan["groups"][0]["exact_names"] == contract["baseline_names"], "baseline 계획 변경")
    require(plan["groups"][1]["exact_names"] == contract["additional_names"], "추가 계획 변경")
    return {"source_files": 14, "baseline_declared": 30, "additional_declared": 48,
            "external_dependencies": 0, "compiled": False, "discovered": False, "executed": False}

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest-sha256", required=True)
    parser.add_argument("--metadata", type=Path)
    args = parser.parse_args()
    require(digest(BASE / "SHA256SUMS") == args.manifest_sha256, "manifest 고정 hash 불일치")
    before = verify()
    if args.metadata:
        require(args.metadata.resolve().is_relative_to(BASE), "metadata 경로 이탈")
        metadata = json.loads(args.metadata.read_text())
        require(len(metadata["packages"]) == 2, "metadata package 수")
        require(all(p["source"] is None for p in metadata["packages"]), "metadata registry 의존성")
        for package in metadata["packages"]:
            require(not any("custom-build" in t["kind"] for t in package["targets"]), "metadata build script")
        own = next(p for p in metadata["packages"] if p["name"] == "zipelo-worker-session-regressions")
        require({t["name"] for t in own["targets"]} == {"worker_session_baseline", "worker_session_regressions"}, "metadata target")
    require(verify() == before, "검사 중 입력 변경")
    print(json.dumps({"state": "passed", **before}))

if __name__ == "__main__":
    main()

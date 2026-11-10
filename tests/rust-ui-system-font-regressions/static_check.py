import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import stat
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent


def require(value, message):
    if not value:
        raise ValueError(message)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def checked_path(name, base):
    path = PurePosixPath(name)
    require(not path.is_absolute() and str(path) == name, "상대 경로가 아님")
    require(all(part not in ("", ".", "..") for part in name.split("/")), "경로 요소 오류")
    current = base
    for part in path.parts:
        current = current / part
        require(not current.is_symlink(), "symlink 입력 거부")
    require(stat.S_ISREG(current.lstat().st_mode), "일반 파일이 아님")
    return current


def main():
    require(not sys.flags.optimize, "일반 Python 모드가 필요함")
    require(len(sys.argv) == 2 and re.fullmatch(r"[0-9a-f]{64}", sys.argv[1]), "manifest hash 필요")
    require(digest(HERE / "SHA256SUMS") == sys.argv[1], "manifest hash 불일치")
    contract = json.loads((HERE / "contract.json").read_text())
    seen = set()
    data = (HERE / "SHA256SUMS").read_text()
    require(data.endswith("\n"), "manifest 끝 줄바꿈 누락")
    for line in data.splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_./-]+)", line)
        require(match is not None, "manifest 행 오류")
        expected, name = match.groups()
        require(name not in seen, "manifest 중복")
        seen.add(name)
        require(digest(checked_path(name, HERE)) == expected, "시험 입력 hash 불일치: " + name)
    require(seen == set(contract["deliverables"]) - {"SHA256SUMS"}, "manifest 파일 집합 불일치")
    pins = json.loads((HERE / "source-pins.json").read_text())
    paths = set()
    for entry in pins["files"]:
        require(entry["path"] not in paths, "source 중복")
        paths.add(entry["path"])
        path = checked_path(entry["path"], ROOT)
        raw = path.read_bytes()
        blob = hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest()
        require(blob == entry["git_blob"] and digest(path) == entry["sha256"], "source pin 불일치: " + entry["path"])
        mode = "100755" if path.stat().st_mode & 0o111 else "100644"
        require(mode == entry["mode"], "source mode 불일치")
    test_source = (HERE / "linux_loader.rs").read_text()
    names = re.findall(r"#\[test\]\s*fn (\w+)", test_source)
    require(names == contract["new_tests"], "신규 선언 이름 불일치")
    require('#[path = "../../experiments/rust-ui-memory-app/src/system_font.rs"]' in test_source, "원본 모듈 재사용 누락")
    require('#![cfg(target_os = "linux")]' in test_source, "Linux 시험 경계 누락")
    original = json.loads((ROOT / "tests/rust-ui-memory-app/contract.json").read_text())
    old_source = (ROOT / "tests/rust-ui-memory-app/regressions.rs").read_text()
    old_names = re.findall(r"#\[test\]\s*fn (\w+)", old_source)
    require(old_names == original["validation"]["names"], "기존 UI 선언 불일치")
    require(sorted(old_names) == contract["reused_ui_tests"] and len(old_names) == 31, "재사용 UI 이름 불일치")
    app = (ROOT / "experiments/rust-ui-memory-app/src/app.rs").read_text()
    mapping = "system_font::load().map_err(|e| eframe::Error::AppCreation(Box::new(e)))?"
    require(mapping in app and app.index(mapping) < app.index("eframe::run_native("), "AppCreation 정적 경계 변경")
    require("FontData::from_owned(font.bytes)" in app and "data.index = font.face_index;" in app, "폰트 값 연결 변경")
    historical = app.replace('#[path = "system_font.rs"]\nmod system_font;\n\n', "", 1)
    historical = historical.replace(
        "let font = " + mapping + ";",
        'let font = std::fs::read("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc")\n'
        "        .map_err(|e| eframe::Error::AppCreation(Box::new(e)))?;", 1)
    historical = historical.replace("FontData::from_owned(font.bytes)", "FontData::from_owned(font)", 1)
    historical = historical.replace("data.index = font.face_index;", "data.index = 1;", 1)
    require(hashlib.sha256(historical.encode()).hexdigest() == contract["base_app_sha256"], "폰트 외 UI 변경 발견")
    manifest = tomllib.loads((HERE / "Cargo.toml").read_text())
    require(not manifest.get("dependencies") and not manifest.get("dev-dependencies"), "신규 의존성 발견")
    lock = tomllib.loads((HERE / "Cargo.lock").read_text())
    require([p["name"] for p in lock["package"]] == ["zipelo-ui-system-font-regressions"], "loader lock graph 변경")
    print(json.dumps({"source_pins": len(paths), "new_tests_declared": len(names),
                      "reused_ui_tests_declared": len(old_names), "rust_compiled": False,
                      "rust_executed": False, "app_creation_evidence": "static-only"}))


if __name__ == "__main__":
    main()

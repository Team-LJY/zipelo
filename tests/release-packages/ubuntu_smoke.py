"""현재 사용자 설정을 보존하고 실제 패키지 설치와 창 실행을 검증함."""

import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import select
import subprocess
import time
import zipfile

ROOT = Path(__file__).resolve().parents[2]
MIMES = ("application/zip", "application/x-7z-compressed", "application/x-compressed-tar")
CONFIGS = (Path.home() / ".config", Path.home() / ".local/share/applications")


def configs():
    return {p: p.read_bytes() for directory in CONFIGS for p in directory.glob("*mimeapps.list")}


def restore_configs(before, after):
    current = configs()
    failures = []
    for path in set(before) | set(after):
        try:
            if current.get(path) != after.get(path):
                raise RuntimeError(f"다른 MIME 설정 변경이 있어 복원을 보류함: {path}")
            if path in before:
                path.write_bytes(before[path])
            elif path.exists():
                path.unlink()
        except Exception as error:
            failures.append(error)
    if failures:
        raise ExceptionGroup("MIME 설정 복원 실패", failures)
    if configs() != before:
        raise RuntimeError("기존 MIME 설정 복원 실패")


@contextmanager
def installed_package(package):
    before = configs()
    after = None
    failures = []
    try:
        try:
            subprocess.run(["sudo", "-n", "dpkg", "-i", str(package)], check=True)
        finally:
            after = configs()
        yield
    except BaseException as error:
        failures.append(error)
    finally:
        try:
            subprocess.run(["sudo", "-n", "dpkg", "-P", "zipelo"], check=True)
        except BaseException as error:
            failures.append(error)
        try:
            if after is not None:
                restore_configs(before, after)
        except BaseException as error:
            failures.append(error)
    if failures:
        raise BaseExceptionGroup("패키지 시험 또는 정리 실패", failures)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path)
    args = parser.parse_args()
    package = args.package.resolve(strict=True)
    runs = ROOT / "tests/release-packages/runs/smoke"
    runs.mkdir(parents=True, exist_ok=False)
    assert subprocess.run(["dpkg-query", "-W", "-f=${db:Status-Status}", "zipelo"],
                          capture_output=True).stdout != b"installed", "기존 설치를 보존해야 함"
    for name in ("/usr/lib/zipelo", "/usr/bin/zipelo", "/usr/share/applications/zipelo.desktop"):
        assert not Path(name).exists(), "기존 파일이 있어 설치를 중단함"
    app = None
    display = None
    with installed_package(package):
        try:
            for mime in MIMES:
                result = subprocess.check_output(["xdg-mime", "query", "default", mime], text=True).strip()
                assert result == "zipelo.desktop", f"연결 실패: {mime}"
            info = json.loads(Path("/usr/lib/zipelo/BUILD-INFO.json").read_text())
            assert info["profile"] == "release"
            archive = runs / "sample archive.zip"
            with zipfile.ZipFile(archive, "w") as output:
                output.writestr("example.txt", "Zipelo package verification")
            display = subprocess.Popen(
                ["Xvfb", "-displayfd", "1", "-screen", "0", "1280x900x24", "-nolisten", "tcp"],
                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
            assert select.select([display.stdout], [], [], 15)[0], "Xvfb 준비 실패"
            number = display.stdout.readline().decode().strip()
            assert number.isdecimal()
            env = {**os.environ, "DISPLAY": ":" + number, "WINIT_UNIX_BACKEND": "x11", "XDG_SESSION_TYPE": "x11"}
            env.pop("WAYLAND_DISPLAY", None)
            with (runs / "app.stdout").open("wb") as stdout, (runs / "app.stderr").open("wb") as stderr:
                app = subprocess.Popen(["/usr/bin/zipelo", str(archive)], env=env, stdout=stdout, stderr=stderr)
                deadline = time.monotonic() + 30
                while True:
                    assert app.poll() is None, "설치된 앱이 조기에 종료됨"
                    window = subprocess.run(["xdotool", "search", "--all", "--pid", str(app.pid), "--name", "Zipelo"],
                                            env=env, capture_output=True, text=True)
                    if window.returncode == 0:
                        break
                    assert time.monotonic() < deadline, "앱 창 표시 시간 초과"
                    time.sleep(0.2)
                time.sleep(3)
                assert app.poll() is None
                subprocess.run(["import", "-window", window.stdout.splitlines()[0],
                                str(runs / "installed-window.png")], env=env, check=True)
            print("PASS installed package, ZIP/7z/TAR.GZ defaults and native X11 window")
        finally:
            for process in (app, display):
                if process is not None and process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
    assert not Path("/usr/bin/zipelo").exists()
    assert not Path("/usr/lib/zipelo").exists()
    print("PASS package removed and previous MIME settings restored")


if __name__ == "__main__":
    main()

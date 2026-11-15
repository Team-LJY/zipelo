"""현재 사용자 설정을 보존하고 실제 패키지 설치와 창 실행을 검증함."""

import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import select
import subprocess
import tempfile
import time
import zipfile

ROOT = Path(__file__).resolve().parents[2]
MIMES = ("application/zip", "application/x-7z-compressed", "application/x-compressed-tar")
CONFIGS = (Path.home() / ".config", Path.home() / ".local/share/applications")


def configs():
    return {p: p.read_bytes() for directory in CONFIGS for p in directory.glob("*mimeapps.list")}


def work_root():
    common = subprocess.check_output(
        ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"], cwd=ROOT, text=True
    ).strip()
    directory = Path(common).resolve()
    if directory.name != ".git":
        raise RuntimeError("프로젝트 Git 작업 폴더를 찾을 수 없음")
    return directory.parent / "tests/work"


def selected_work_directory(requested, allowed_work_root):
    runs = (requested or allowed_work_root / "ubuntu-package-smoke").resolve()
    if runs.parent != allowed_work_root:
        raise ValueError("작업 폴더는 프로젝트 tests/work 바로 아래에 있어야 함")
    return runs


def ensure_no_existing_installation():
    status = subprocess.run(
        ["dpkg-query", "-W", "-f=${db:Status-Status}", "zipelo"],
        capture_output=True,
    )
    if status.stdout == b"installed":
        raise RuntimeError("기존 설치를 보존해야 함")
    for name in ("/usr/lib/zipelo", "/usr/bin/zipelo", "/usr/share/applications/zipelo.desktop"):
        if Path(name).exists():
            raise RuntimeError("기존 파일이 있어 설치를 중단함")


def isolated_environment(home):
    home = home.resolve()
    for directory in (home / ".config", home / ".local/share", home / ".local/state", home / ".cache"):
        directory.mkdir(parents=True, exist_ok=True)
    return {
        **os.environ,
        "HOME": str(home),
        "XDG_CONFIG_HOME": str(home / ".config"),
        "XDG_DATA_HOME": str(home / ".local/share"),
        "XDG_STATE_HOME": str(home / ".local/state"),
        "XDG_CACHE_HOME": str(home / ".cache"),
        "XDG_DATA_DIRS": "/usr/local/share:/usr/share",
    }


def ensure_host_configs_unchanged(before):
    if configs() != before:
        raise RuntimeError("실제 사용자 MIME 설정이 시험 중 변경됨")


@contextmanager
def installed_package(package, home):
    before = configs()
    isolated = isolated_environment(home)
    package_environment = [
        "sudo", "-n", "env", "SUDO_USER=root",
        f"HOME={isolated['HOME']}",
        f"XDG_CONFIG_HOME={isolated['XDG_CONFIG_HOME']}",
        f"XDG_DATA_HOME={isolated['XDG_DATA_HOME']}",
        f"XDG_STATE_HOME={isolated['XDG_STATE_HOME']}",
        f"XDG_CACHE_HOME={isolated['XDG_CACHE_HOME']}",
    ]
    defaults_started = False
    failures = []
    try:
        subprocess.run([*package_environment, "dpkg", "-i", str(package)], check=True)
        ensure_host_configs_unchanged(before)
        defaults_started = True
        subprocess.run(["/usr/lib/zipelo/default-apps", "install"], env=isolated, check=True)
        yield isolated
    except BaseException as error:
        failures.append(error)
    finally:
        if defaults_started:
            try:
                subprocess.run(["/usr/lib/zipelo/default-apps", "remove"], env=isolated, check=True)
            except BaseException as error:
                failures.append(error)
        try:
            subprocess.run([*package_environment, "dpkg", "-P", "zipelo"], check=True)
        except BaseException as error:
            failures.append(error)
        try:
            ensure_host_configs_unchanged(before)
        except BaseException as error:
            failures.append(error)
    if failures:
        raise BaseExceptionGroup("패키지 시험 또는 정리 실패", failures)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path)
    parser.add_argument("--work-dir", type=Path)
    args = parser.parse_args()
    package = args.package.resolve(strict=True)
    allowed_work_root = work_root().resolve()
    try:
        runs = selected_work_directory(args.work_dir, allowed_work_root)
    except ValueError as error:
        parser.error(str(error))
    runs.mkdir(parents=True, exist_ok=False)
    ensure_no_existing_installation()
    app = None
    display = None
    with tempfile.TemporaryDirectory(dir=runs) as temporary_home:
        with installed_package(package, Path(temporary_home)) as isolated:
            try:
                for mime in MIMES:
                    result = subprocess.check_output(
                        ["xdg-mime", "query", "default", mime], env=isolated, text=True
                    ).strip()
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
                env = {**isolated, "DISPLAY": ":" + number, "WINIT_UNIX_BACKEND": "x11", "XDG_SESSION_TYPE": "x11"}
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
    print("PASS package removed and host MIME settings unchanged")


if __name__ == "__main__":
    main()

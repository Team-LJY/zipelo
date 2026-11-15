import os
import re
from pathlib import Path
import select
import signal
import subprocess
import time


ROOT = Path(__file__).resolve().parents[2]


def work_directory():
    common = Path(subprocess.check_output(
        ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
        cwd=ROOT, text=True,
    ).strip()).resolve()
    if common.name != ".git":
        raise RuntimeError("프로젝트 Git 작업 폴더를 찾을 수 없음")
    job = os.environ.get("ZIPELO_CI_JOB", "rust-local")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]{0,63}", job):
        raise ValueError("안전한 작업명이 필요함")
    work = common.parent / "tests/work" / job
    if work.is_symlink() or not work.resolve().is_relative_to(common.parent / "tests/work"):
        raise ValueError("작업 폴더 밖의 경로는 허용하지 않음")
    return work


def binary_path(work):
    target = Path(os.environ.get("CARGO_TARGET_DIR", work / "target"))
    return target / "debug/zipelo-ui-memory-app"


def main():
    work = work_directory()
    output = work / "screens" / str(time.time_ns())
    output.mkdir(mode=0o700, parents=True)
    display = subprocess.Popen(
        ["Xvfb", "-displayfd", "1", "-screen", "0", "1280x900x24", "-nolisten", "tcp"],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True,
    )
    app = None
    try:
        if not select.select([display.stdout], [], [], 15)[0]:
            raise RuntimeError("Xvfb 준비 시간 초과")
        number = display.stdout.readline().decode().strip()
        if not number.isdecimal():
            raise RuntimeError("Xvfb display 응답 오류")
        env = {**os.environ, "DISPLAY": ":" + number, "WINIT_UNIX_BACKEND": "x11"}
        env.pop("WAYLAND_DISPLAY", None)
        with (output / "app.stdout").open("wb") as stdout, (output / "app.stderr").open("wb") as stderr:
            app = subprocess.Popen(
                [str(binary_path(work)), "--ui-fixture", "/synthetic/example.zip"],
                env=env, stdout=stdout, stderr=stderr, start_new_session=True,
            )
            deadline = time.monotonic() + 30
            while True:
                if app.poll() is not None:
                    raise RuntimeError(f"화면 조기 종료: {app.returncode}")
                window = subprocess.run(["xdotool", "search", "--all", "--pid", str(app.pid), "--name", "Zipelo"], env=env, timeout=5, capture_output=True)
                if window.returncode == 0:
                    break
                if time.monotonic() >= deadline:
                    raise RuntimeError("소유 창 표시 시간 초과")
                time.sleep(0.1)
            time.sleep(1)
            subprocess.run(["import", "-window", "root", str(output / "list.png")], env=env, timeout=10, check=True)
            subprocess.run(["xdotool", "search", "--pid", str(app.pid), "--name", "Zipelo"], env=env, timeout=10, check=True)
            print(str(output / "list.png"))
    finally:
        for process in [app, display]:
            if process is not None:
                if process.poll() is None:
                    os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=5)
        if display.stdout:
            display.stdout.close()
        if display.stderr:
            display.stderr.close()


if __name__ == "__main__":
    main()

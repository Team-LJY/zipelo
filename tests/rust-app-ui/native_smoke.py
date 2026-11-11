import os
from pathlib import Path
import select
import signal
import subprocess
import time


ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "tests/rust-app-ui" / ("screens-" + str(time.time_ns()))


def main():
    OUTPUT.mkdir(exist_ok=True)
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
        with (OUTPUT / "app.stdout").open("wb") as stdout, (OUTPUT / "app.stderr").open("wb") as stderr:
            app = subprocess.Popen(
                [str(ROOT / "tests/ci/target/debug/zipelo-ui-memory-app"), "--ui-fixture", "/synthetic/example.zip"],
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
            subprocess.run(["import", "-window", "root", str(OUTPUT / "list.png")], env=env, timeout=10, check=True)
            subprocess.run(["xdotool", "search", "--pid", str(app.pid), "--name", "Zipelo"], env=env, timeout=10, check=True)
            print(str(OUTPUT / "list.png"))
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

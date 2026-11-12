"""격리한 Xvfb에서 상태별 실제 창을 촬영함."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]


def command(args, environment, **kwargs):
    return subprocess.run(args, env=environment, check=True, **kwargs)


def stop(process):
    process.terminate()
    try:
        process.wait(timeout=3)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()


def capture(binary, variant, state, output, environment, width, height):
    automatic = ROOT / "tests/ui-redesign/runs/fixtures/여행"
    try:
        automatic.rmdir()
    except OSError:
        pass
    mode = "opened" if state in {"picker", "selected-empty"} else state
    log = ROOT / "tests/ui-redesign/runs" / f"{variant}-{state}.log"
    with log.open("w") as stream:
        app = subprocess.Popen([str(binary), mode], cwd=ROOT, env=environment, stdout=stream, stderr=stream)
        try:
            deadline = time.monotonic() + 15
            window = None
            while time.monotonic() < deadline and app.poll() is None:
                found = subprocess.run(
                    ["xdotool", "search", "--onlyvisible", "--name", "^Zipelo UI capture"],
                    env=environment, capture_output=True, text=True,
                )
                if found.returncode == 0:
                    window = found.stdout.splitlines()[0]
                    break
                time.sleep(0.1)
            if window is None:
                raise RuntimeError(f"창 열기 실패: {log}")
            command(["xdotool", "windowsize", window, str(width), str(height)], environment)
            command(["xdotool", "windowfocus", "--sync", window], environment)
            time.sleep(0.7)

            def click(x, y):
                command(["xdotool", "mousemove", "--window", window, str(x), str(y), "click", "1"], environment)
                time.sleep(0.4)

            if state == "create" or (state == "create-filled" and variant == "before"):
                click(180 if variant == "before" else width - 90, 18 if variant == "before" else 40)
            if state == "create-filled" and variant == "before":
                base = ROOT / "tests/ui-redesign/runs/fixtures"
                click(100, 105)
                command(["xdotool", "type", "--clearmodifiers", "--delay", "0",
                         str(ROOT / "tests/ui-redesign/README.md")], environment)
                command(["xdotool", "key", "Return"], environment)
                command(["xdotool", "type", "--clearmodifiers", "--delay", "0", str(base)], environment)
                click(100, 200)
                command(["xdotool", "type", "--clearmodifiers", "--delay", "0", str(base / "archive.zip")], environment)
            elif state == "picker":
                click(97 if variant == "before" else width - 90, 18 if variant == "before" else height - 80)
            elif state == "selected-empty":
                click(16 if variant == "before" else 24, 126 if variant == "before" else height - 36)
            elif state == "cancelling":
                click(260 if variant == "before" else width - 60, 18 if variant == "before" else height - 36)
            if state == "password-create" and variant == "after":
                click(width // 2 - 56, height // 2 - 12)
                command(["xdotool", "type", "--clearmodifiers", "--delay", "0", "fixture-password"], environment)
                command(["xdotool", "key", "Tab", "Tab"], environment)
                command(["xdotool", "type", "--clearmodifiers", "--delay", "0", "different"], environment)
            command(["xdotool", "mousemove", "--window", window, "2", "2"], environment)
            time.sleep(0.3)
            destination = output / f"{state}.png"
            command(["import", "-window", window, str(destination)], environment)
            return {"state": state, "path": str(destination.relative_to(ROOT)), "width": width, "height": height}
        finally:
            stop(app)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--variant", choices=["before", "after"], required=True)
    parser.add_argument("--states", nargs="+", default=["empty", "hover", "opened", "selected-empty", "picker", "create", "create-filled", "password", "password-retry", "password-create", "progress", "progress-unknown", "cancelling", "success", "error"])
    parser.add_argument("--width", type=int, default=1060)
    parser.add_argument("--height", type=int, default=720)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    output = (args.output or ROOT / "tests/ui-redesign/screenshots" / args.variant).resolve()
    if not output.is_relative_to(ROOT / "tests"):
        raise ValueError("촬영 결과는 tests/ 안에만 저장함")
    output.mkdir(parents=True, exist_ok=True)
    read_fd, write_fd = os.pipe()
    server = subprocess.Popen(
        ["Xvfb", "-displayfd", str(write_fd), "-screen", "0", "1280x900x24", "-nolisten", "tcp"],
        pass_fds=(write_fd,), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    os.close(write_fd)
    try:
        with os.fdopen(read_fd) as stream:
            display = stream.readline().strip()
        if not display:
            raise RuntimeError("Xvfb를 시작하지 못함")
        environment = dict(os.environ, DISPLAY=f":{display}", LIBGL_ALWAYS_SOFTWARE="1", WINIT_UNIX_BACKEND="x11", XDG_SESSION_TYPE="x11")
        environment.pop("WAYLAND_DISPLAY", None)
        results = [
            capture(args.binary.resolve(), args.variant, state, output, environment, args.width, args.height)
            for state in args.states
        ]
        metadata = output / "capture.json"
        existing = json.loads(metadata.read_text()) if metadata.exists() else []
        merged = {row["state"]: row for row in existing}
        merged.update({row["state"]: row for row in results})
        metadata.write_text(json.dumps(list(merged.values()), ensure_ascii=False, indent=4) + "\n")
        print(json.dumps(results, ensure_ascii=False))
    finally:
        stop(server)


if __name__ == "__main__":
    main()

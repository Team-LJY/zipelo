"""실제 X11 창에서 파일 행을 눌렀을 때 선택 배경이 바뀌는지 확인함."""

import argparse
from pathlib import Path
import subprocess
import sys
import time

import capture


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()
    original = capture.command
    clicked = False

    def command(arguments, environment, **kwargs):
        nonlocal clicked
        if arguments[:3] == ["xdotool", "mousemove", "--window"] and arguments[-2:] == ["2", "2"]:
            original(
                ["xdotool", "mousemove", "--window", arguments[3], "120", "271"],
                environment,
            )
            time.sleep(0.2)
            original(["xdotool", "click", "1"], environment)
            clicked = True
            time.sleep(0.2)
        return original(arguments, environment, **kwargs)

    capture.command = command
    output = capture.ROOT / "tests/ui-redesign/runs/row-selection"
    sys.argv = [
        sys.argv[0], "--binary", str(args.binary), "--variant", "after",
        "--states", "opened", "--output", str(output),
    ]
    capture.main()
    if not clicked:
        raise AssertionError("행 클릭을 실행하지 못함")
    pixel = subprocess.check_output(
        ["convert", str(output / "opened.png"), "-format", "%[pixel:p{500,271}]", "info:"],
        text=True,
    ).strip()
    if pixel != "srgb(232,240,254)":
        raise AssertionError(f"행 선택 배경 불일치: {pixel}")
    print("실제 파일 이름 클릭 후 선택 표시 확인")


if __name__ == "__main__":
    main()

"""실제 Ubuntu 패키지와 격리된 사용자 설정으로 제거 후 ZIP 열기를 검증함."""

import argparse
import os
from pathlib import Path
import subprocess
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[2]


def run(*args, **kwargs):
    return subprocess.run(args, check=True, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path)
    args = parser.parse_args()
    package = args.package.resolve(strict=True)
    status = subprocess.run(["dpkg-query", "-W", "-f=${db:Status-Status}", "zipelo"],
                            capture_output=True, text=True)
    if status.returncode == 0 and status.stdout not in ("not-installed", "config-files"):
        raise RuntimeError("기존 Zipelo 패키지가 있어 검증을 중단함")
    for name in ("/usr/lib/zipelo", "/usr/bin/zipelo", "/usr/share/applications/zipelo.desktop"):
        if Path(name).exists() or Path(name).is_symlink():
            raise RuntimeError("기존 설치 파일이 있어 검증을 중단함")
    work = ROOT / "tests/work"
    work.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(dir=work) as temporary:
        root = Path(temporary)
        config = root / "config"
        data = root / "data"
        config.mkdir()
        apps = data / "applications"
        apps.mkdir(parents=True)
        output = root / "opened.txt"
        opener = root / "open-zip"
        opener.write_text("#!/bin/sh\n/usr/bin/unzip -p \"$1\" proof.txt > '" + str(output) + "'\n")
        opener.chmod(0o755)
        (apps / "previous.desktop").write_text(
            "[Desktop Entry]\nType=Application\nName=Previous archive app\n"
            f"Exec={opener} %f\nMimeType=application/zip;\nTerminal=false\n")
        archive = root / "proof.zip"
        with zipfile.ZipFile(archive, "w") as zipped:
            zipped.writestr("proof.txt", "opened by another app")
        env = dict(os.environ, HOME=str(root), XDG_CONFIG_HOME=str(config),
                   XDG_DATA_HOME=str(data), XDG_STATE_HOME=str(root / "state"),
                   XDG_DATA_DIRS="/usr/local/share:/usr/share",
                   XDG_CONFIG_DIRS=str(root / "system-config"), XDG_CURRENT_DESKTOP="X-Generic",
                   DE="generic", DISPLAY=":987", BROWSER="")
        env.pop("WAYLAND_DISPLAY", None)
        env.pop("DBUS_SESSION_BUS_ADDRESS", None)
        helper = "/usr/lib/zipelo/default-apps"
        attempted = False
        try:
            # 호스트 기본 앱은 건드리지 않고 등록 후보만 실제 설치함.
            attempted = True
            run("sudo", "-n", "env", "-u", "SUDO_USER", "dpkg", "-i", str(package))
            run("xdg-mime", "default", "previous.desktop", "application/zip", env=env)
            run("sh", helper, "install", env=env)
            selected = subprocess.check_output(
                ["xdg-mime", "query", "default", "application/zip"], env=env, text=True).strip()
            assert selected == "zipelo.desktop", selected
            run("sh", helper, "remove", env=env)
            selected = subprocess.check_output(
                ["xdg-mime", "query", "default", "application/zip"], env=env, text=True).strip()
            assert selected == "previous.desktop", selected
        finally:
            if attempted:
                run("sudo", "-n", "env", "-u", "SUDO_USER", "dpkg", "-P", "zipelo")
        assert not Path(helper).exists()
        assert not Path("/usr/share/applications/zipelo.desktop").exists()
        run("xdg-open", str(archive), env=env)
        assert output.read_text() == "opened by another app"
        print("PASS actual package install/remove, isolated default restore and ZIP open via unzip")
        print("LIMIT no native desktop UI; SUDO_USER dispatch is covered separately")


if __name__ == "__main__":
    main()

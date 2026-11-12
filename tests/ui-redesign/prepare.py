"""현재 화면과 로컬 Git 기준 화면의 촬영 실행 파일을 준비함."""

import argparse
import os
from pathlib import Path
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = Path("experiments/rust-ui-memory-app/Cargo.toml")
HELPER = Path("tests/rust-app-ui/capture.rs")
BIN = '\n[[bin]]\nname = "zipelo-ui-capture"\npath = "../../tests/rust-app-ui/capture.rs"\nrequired-features = ["ui-fixture"]\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before-ref", default="b10c7eedddd202f83766aa8f001142cdfdb4097c")
    parser.add_argument("--output", type=Path, default=ROOT / "tests/ui-redesign/runs")
    parser.add_argument("--target-dir", type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    if not output.is_relative_to(ROOT / "tests"):
        raise ValueError("기준 소스와 실행 파일은 tests/ 안에만 저장함")
    before = output / "before-src"
    if before.exists():
        raise ValueError("기존 기준 소스는 보존함. 새 --output 폴더를 지정하세요")
    before.mkdir(parents=True)
    revision = subprocess.check_output(
        ["git", "rev-parse", "--verify", "--end-of-options", args.before_ref + "^{commit}"],
        cwd=ROOT, text=True,
    ).strip()
    with subprocess.Popen(["git", "archive", revision], cwd=ROOT, stdout=subprocess.PIPE) as archive:
        with tarfile.open(fileobj=archive.stdout, mode="r|") as source:
            source.extractall(before, filter="data")
        if archive.wait() != 0:
            raise RuntimeError("로컬 기준 소스를 읽지 못함")
    helper = (ROOT / HELPER).read_text()
    helper = helper.replace(
        "self.app.begin_extract();",
        "self.app.model.begin(start, &mut self.factory);",
    )
    helper = helper.replace(
        'app.add_sources(vec![base.join("읽어주세요.txt"), base.join("사진")]);',
        "let _ = &mut app;",
    )
    helper = helper.replace('self.app.browse_folder("Install");', "")
    helper = helper.replace('self.app.show_create(false);', "")
    (before / HELPER).write_text(helper)
    original = before / MANIFEST
    original.write_text(original.read_text() + BIN)
    environment = dict(os.environ, CARGO_TARGET_DIR=str(args.target_dir.resolve()))
    for variant, root in [("before", before), ("after", ROOT)]:
        with (output / f"{variant}-build.log").open("w") as log:
            subprocess.run(
                ["cargo", "+1.99.1", "build", "--locked", "--offline", "--manifest-path",
                 str(root / MANIFEST), "--features", "ui-fixture", "--bin", "zipelo-ui-capture"],
                cwd=ROOT, env=environment, stdout=log, stderr=log, check=True,
            )
        executable = args.target_dir.resolve() / "debug/zipelo-ui-capture"
        destination = output / f"{variant}-capture"
        destination.write_bytes(executable.read_bytes())
        destination.chmod(executable.stat().st_mode)
    print(f"기준 커밋 {revision}, 촬영 실행 파일 {output}")


if __name__ == "__main__":
    main()

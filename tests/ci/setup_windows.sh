#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
common_dir="$(git -C "$repo_root" rev-parse --path-format=absolute --git-common-dir)"
project_root="$(dirname "$common_dir")"
job="${ZIPELO_CI_JOB:-rust-local}"
if [[ ! "$job" =~ ^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$ ]]; then
    echo "유효하지 않은 작업명: $job" >&2
    exit 2
fi
ci_tools="${ZIPELO_CI_TOOLS:-$project_root/tests/work/$job/tools}"
ci_tools="$(python3 - "$project_root" "$job" "$ci_tools" <<'PYTHON'
import os
from pathlib import Path
import sys

project = Path(sys.argv[1]).resolve()
allowed = project / "tests/work" / sys.argv[2]
tools = Path(os.path.abspath(sys.argv[3]))
for path in (project / "tests", project / "tests/work", allowed, tools,
             tools / "downloads", tools / "mingw"):
    if path.resolve() != path:
        raise SystemExit(f"작업 폴더 밖의 경로는 허용하지 않음: {path}")
if not tools.is_relative_to(allowed):
    raise SystemExit(f"작업 폴더 밖의 경로는 허용하지 않음: {tools}")
print(tools)
PYTHON
)"
mkdir -p "$ci_tools/downloads" "$ci_tools/mingw"

# 도구를 프로젝트 안에 풀며 시스템 패키지는 설치하지 않음.
(
    cd "$ci_tools/downloads"
    apt-get download \
        gcc-mingw-w64-x86-64-posix \
        g++-mingw-w64-x86-64-posix \
        gcc-mingw-w64-x86-64-posix-runtime \
        gcc-mingw-w64-base \
        binutils-mingw-w64-x86-64 \
        binutils-mingw-w64-base \
        mingw-w64-x86-64-dev \
        mingw-w64-common
    for package in ./*.deb; do
        dpkg-deb -x "$package" "$ci_tools/mingw"
    done
)
RUSTUP_HOME="$ci_tools/rustup" rustup toolchain install 1.99.1 \
    --profile minimal --no-self-update --target x86_64-pc-windows-gnu

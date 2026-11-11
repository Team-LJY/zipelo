# 로컬 Rust 검증

PR을 병합하기 전에 linux-desktop-2에서 아래 명령을 실행하고 검증한 커밋, 요약과 실패 또는 미실행 항목을 PR 본문에 남깁니다. 실패를 성공으로 처리하지 않습니다.

```sh
# 최초 1회, Ubuntu의 apt 저장소에서 프로젝트 내부로 교차 도구를 준비함.
bash tests/ci/setup_windows.sh
python3 tests/ci/run_rust.py local
```

`local`은 실행기 자체 검사 후 교차 도구를 찾습니다. 도구가 없으면 `setup_windows.sh`를 실행해 프로젝트 안에 최초 준비하므로 네트워크와 아래 준비 도구가 필요합니다. 준비 실패는 결과 JSON에 `tool-setup` 실패로 남깁니다. 준비된 도구가 있으면 다시 다운로드하지 않습니다. 이후 Linux 전체 빌드, Linux Rust 시험, Windows 교차 빌드를 순서대로 수행합니다. 단계별 통과/실패/비해당 개수를 출력하고 `tests/ci/local-results.json`에 검증한 HEAD와 결과를 기록합니다. 실패가 있으면 종료 코드는 1입니다.

- Rust 1.99.1, 기존 Cargo.lock, `--locked --all-targets --all-features`를 사용합니다. `experiments/*/Cargo.toml`과 `tests/*/Cargo.toml`을 자동 발견합니다.
- Windows는 `x86_64-pc-windows-gnu` 대상의 링크까지 확인합니다. 시험 바이너리도 컴파일하지만 실행하지 않습니다. Windows MSVC 빌드 및 실제 실행 검증을 대신하지 않습니다.
- 빌드 기본 병렬 수는 2이며 `CARGO_BUILD_JOBS`로 조절합니다. 시험 스레드는 1개입니다. `CARGO_TARGET_DIR` 기본값은 `tests/ci/target`이므로 후속 실행에서 빌드 캐시를 재사용합니다.
- `setup_windows.sh`는 관리자 권한 없이 도구를 `tests/ci/tools`에 풉니다. apt-get, dpkg-deb, rustup 및 GCC 호스트 실행 라이브러리가 필요합니다. 공용 도구 경로를 재사용할 때는 프로젝트 안의 소유 경로를 `ZIPELO_CI_TOOLS`로 지정합니다. 연결된 Git worktree에서는 현재 checkout의 도구를 먼저 찾고 같은 저장소 기본 worktree의 `tests/ci/tools`도 찾습니다. `ZIPELO_CI_TOOLS`를 지정하면 그 경로만 사용하며 별도 PATH 내보내기는 필요하지 않습니다. 기존 Rust target과 MinGW가 있으면 준비 스크립트 없이 PATH와 관련 Cargo/CC/CXX 환경 설정을 사용할 수 있습니다.
- Linux GUI 빌드에는 pkg-config, libxkbcommon-dev, libwayland-dev, libgl1-mesa-dev가 필요하며 글꼴 시험에는 fonts-noto-cjk가 필요합니다.
- 추적된 fixture를 Git blob과 대조합니다. Windows checkout 시 core.autocrlf=false를 사용해야 하며 기대값의 바이트를 바꾸지 않습니다.
- Unix 소켓 시험을 위해 짧은 checkout 경로를 사용합니다. 예: 프로젝트 아래 `tests/ci-k`. 기존 `runs` 디렉터리는 소유권, 0700, 빈 상태를 확인하고 새 디렉터리만 생성합니다. 실패한 fixture는 삭제하거나 권한을 바꾸지 않습니다.
- 파일 게시 계층은 ext4/XFS만 지원합니다. tmpfs 등 다른 파일시스템에서는 `rust-file-publication`과 `rust-gzip-file-job`의 Rust 실행 시험만 `not-applicable`로 기록하고 파일시스템 식별값과 사유를 출력합니다. 해당 crate의 Linux 및 Windows 빌드와 snapshot 등 나머지 시험은 계속합니다. 전체 시험 통과가 필요한 검증은 짧은 ext4/XFS checkout에서 수행합니다. 파일시스템 탐색 오류나 기존 시험 디렉터리의 소유권 문제는 실패로 남깁니다.
- 개별 단계는 `build`, `test`, `cross-build`로 실행할 수 있습니다. 한 crate가 실패해도 다른 crate 결과를 수집합니다.
- GUI 앱과 GUI race harness는 컴파일하며 실제 화면 시나리오는 실행하지 않습니다. C# 시험과 역사적 hash 검사는 포함하지 않습니다.

## 출시 전 Windows 실기 확인

`.github/workflows/rust.yml`에는 `workflow_dispatch`만 있습니다. push/PR 자동 실행과 Linux GitHub job은 없습니다. 수동 Windows 실행은 출시 전에 별도로 배정하며, 이 작업에서는 다시 실행하지 않습니다. workflow가 기본 브랜치에 반영된 후 Actions에서 해당 브랜치를 골라 실행할 수 있습니다.

수동 실행은 Windows에서 전체 빌드와 Rust 시험을 수행하고 Linux 전용 publication, snapshot, file-job, system-font 시험은 `not-applicable`로 표시합니다. 기존 GUI 화면 시나리오는 별도 확인해야 합니다.

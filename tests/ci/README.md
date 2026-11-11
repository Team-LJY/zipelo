# Rust CI

`.github/workflows/rust.yml`은 PR, main push와 수동 실행에서 Ubuntu와 Windows를 각각 검사합니다. Rust 1.99.1과 기존 Cargo.lock을 사용하며 제품 소스와 의존성 버전을 바꾸지 않습니다.

```sh
python -m unittest discover -s tests/ci -p 'test_*.py' -v
python tests/ci/run_rust.py build
python tests/ci/run_rust.py test
```

- `experiments/*/Cargo.toml`과 `tests/*/Cargo.toml`을 자동 발견합니다. 새 crate도 같은 구조와 Cargo.lock을 사용하면 포함됩니다.
- checkout 단계의 Git 프로세스에만 core.autocrlf=false를 적용해 fixture 원본 바이트를 보존합니다. 빌드 전에 추적된 fixture 33개를 Git blob과 대조하며, OS에 따라 기대값을 바꾸지 않습니다.
- 모든 target과 feature를 빌드합니다. 시험은 Cargo에 등록된 루트 tests/의 Rust 회귀와 crate의 단위시험을 실행합니다. 시험 스레드는 1개입니다.
- Linux 전용 publication, snapshot, file-job, system-font 시험은 Windows에서 `not-applicable`로 표시합니다. 해당 crate의 Windows 빌드는 확인합니다.
- Linux 파일 시험에는 새 소유 `runs` 디렉터리를 0700으로 생성합니다. 기존 디렉터리의 권한은 변경하지 않으며 비어 있지 않으면 중단합니다. 글꼴 시험에는 `fonts-noto-cjk`가 필요합니다.
- Unix 소켓 시험은 최종 fixture 경로가 108바이트 미만이어야 합니다. 로컬 검증은 짧은 checkout 경로를 사용하세요. 실패한 fixture와 로그는 원인 확인 전까지 보존합니다.
- GUI 앱과 GUI race harness는 컴파일하며 실제 GUI 시나리오는 실행하지 않습니다. C# 시험과 역사적 manifest/hash 검사는 이 Rust CI 범위에 포함하지 않습니다.
- Cargo 실패를 숨기지 않고 나머지 crate 결과도 수집한 뒤 실패로 종료합니다. OS 간에는 fail-fast를 끕니다. 빌드와 시험 단계는 각각 20분, 전체 job은 45분 제한입니다.
- 결과는 Actions 로그와 job summary 및 로컬 `tests/ci/*-results.json`에 남습니다. 공유 target은 `tests/ci/target`이며 `CARGO_TARGET_DIR`로 바꿀 수 있습니다.

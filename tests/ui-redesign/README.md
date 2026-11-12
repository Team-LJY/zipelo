# Zipelo 화면 회귀 확인

현재 화면은 툴바, 현재 폴더 경로와 검색, 폴더 탐색, 상태 줄과 ZIP 만들기 모달을 사용함. 파일과 폴더 선택 및 저장은 rfd의 운영체제 기본 창을 사용함. 설계 문서는 docs/ui-design-v0.1.md이며 실행 근거는 validation.json에 기록함.

## 기능 검증

프로젝트 루트에서 실행함.

```sh
python3 tests/ci/run_rust.py local
```

tests/rust-app-ui/toolbar.rs는 가상 폴더, 현재 폴더 검색, 대화창 취소, 파일 경로 검증, 키보드 활성화와 폴더 선택 확장을 확인함. 폴더 전체 선택 뒤 하위 파일을 해제한 경우도 최종 요청 ID에서 제외되는지 검증함. 대화창의 반환값은 tests/rust-app-ui/dialogs.rs로 주입하며 실제 운영체제 창 실행과 구분함.

tests/rust-app-real-worker/selection_flows.rs는 전체, 파일, 폴더, 혼합, 빈 폴더와 중첩 폴더 선택을 실제 작업자로 검증함. 선택한 폴더의 하위 항목을 요청 ID에 포함하고, 동일한 범위로 완료 판정과 실제 파일 내용을 확인함. 암호 ZIP 생성과 추출, 취소 후 파일 보존은 flows.rs와 password_flows.rs에서 확인함.

Enter는 초점이 있는 버튼만 활성화하며 암호 입력란 제출만 예외임. 완료 + Unconfirmed 안내는 자세히 보기에, Failed 내구성 경고와 실패 및 부분 추출의 Unconfirmed 안내는 본문에 표시함. 자동 풀 위치의 Unix 0700 생성, 렌더러 폴백과 계약 revision 2도 기존 회귀 시험을 유지함.

## 화면 재현

Xvfb, xdotool, ImageMagick import와 시스템 CJK 글꼴이 필요함. 촬영 실행 파일은 실제 eframe 컴포넌트에 합성 목록을 전달하며 압축파일을 읽거나 결과물을 저장하지 않음.

```sh
cargo +1.99.1 build --locked --offline --manifest-path experiments/rust-ui-memory-app/Cargo.toml --features ui-fixture --bin zipelo-ui-capture
python3 tests/ui-redesign/capture.py --binary tests/ci/target/debug/zipelo-ui-capture --variant after --states toolbar-root toolbar-folder toolbar-create --output tests/ui-redesign/screenshots/toolbar
```

CARGO_TARGET_DIR을 지정한 경우 --binary 경로도 해당 경로에 맞춤. 촬영기는 전용 Xvfb에서 자신이 만든 앱과 서버만 종료함. 현재 세 화면은 screenshots/toolbar에 있으며 모두 1060x720임. 기존 before, after와 minimum 디렉터리는 이전 UI의 기록이므로 현재 화면 검증으로 사용하지 않음. 기존 prepare.py는 과거 기준 화면과의 비교용임.

최소 창 크기는 760x540이며 ZIP 만들기 본문만 스크롤하고 제목과 하단 버튼은 유지함. 현재 화면의 Windows 실제 실행, Linux portal 기본 창, 운영체제 탐색기 실행과 전체 보조 기술 감사는 별도 확인이 필요함.

## 의존성 고지

```sh
python3 packaging/licenses.py --manifest experiments/rust-ui-memory-app/Cargo.toml --manifest experiments/rust-archive-worker/Cargo.toml --output tests/ui-redesign/runs/THIRD-PARTY-LICENSES.txt --audit tests/ui-redesign/runs/toolbar-license-audit.json
```

rfd 0.16.0과 ashpd 0.11.1은 MIT이며 최종 고지는 패키지 빌드 시 잠금 파일에서 다시 생성함. 생성 결과와 해시는 validation.json에 기록함. 대용량 작업자 시험에는 임시 파일과 빌드 캐시를 위한 여유 공간이 필요함.

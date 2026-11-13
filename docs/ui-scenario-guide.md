# UI 상황 디버깅 가이드

실제 압축파일이나 작업자 프로세스 없이 원하는 화면을 바로 연다. 현재 제품의 위젯과 배치를 그대로 사용하며 진행률은 고정된다. 화면을 검토하기 위한 미리보기이므로 버튼, 암호 입력, 파일 끌어 놓기 등 조작은 받지 않는다. 창을 닫고 다른 상황 이름으로 다시 실행한다. 실제 조작 시험에는 기존 --ui-fixture 실행을 사용한다.

이 기능은 debug_assertions가 켜지고 ui-fixture feature를 활성화한 개발 빌드에서만 제공한다. 일반 빌드와 --release 빌드에는 상황 목록과 구성 코드가 들어가지 않는다. 릴리스 설치 파일은 ui feature만 사용한다. 화면에 표시되는 /Samples와 C:\Samples 경로는 가상 경로이며 파일이나 폴더를 만들 필요가 없다.

## Ubuntu

저장소 최상위에서 실행한다. Rust 1.99.1과 기존 프로젝트의 Linux GUI 빌드 도구가 필요하다. 빌드 결과와 임시 파일은 tests/work 안에 둔다.

~~~bash
mkdir -p tests/work/tmp
export TMPDIR="$PWD/tests/work/tmp"
export CARGO_TARGET_DIR="$PWD/tests/work/ui-scenario-target"
cargo +1.99.1 build --locked --manifest-path experiments/rust-ui-memory-app/Cargo.toml --features ui-fixture --bin zipelo-ui-memory-app

"$CARGO_TARGET_DIR/debug/zipelo-ui-memory-app" --list-ui-scenarios
"$CARGO_TARGET_DIR/debug/zipelo-ui-memory-app" --ui-scenario password
"$CARGO_TARGET_DIR/debug/zipelo-ui-memory-app" --ui-scenario create-encrypted
~~~

현재 데스크톱 세션에서 실행한다. 목록과 이름 오류 확인은 DISPLAY나 Wayland 연결 없이도 된다.

## Windows

Rust 1.99.1, Visual Studio C++ 빌드 도구와 Windows SDK가 설치된 환경의 PowerShell에서 저장소 최상위로 이동해 실행한다. 프로젝트 자체의 외부 crate는 추가하지 않는다.

~~~powershell
New-Item -ItemType Directory -Force tests/work/tmp | Out-Null
$env:TMP = Join-Path $PWD "tests/work/tmp"
$env:TEMP = $env:TMP
$env:CARGO_TARGET_DIR = Join-Path $PWD "tests/work/ui-scenario-target"
cargo +1.99.1 build --locked --manifest-path experiments/rust-ui-memory-app/Cargo.toml --features ui-fixture --bin zipelo-ui-memory-app

& "$env:CARGO_TARGET_DIR/debug/zipelo-ui-memory-app.exe" --list-ui-scenarios
& "$env:CARGO_TARGET_DIR/debug/zipelo-ui-memory-app.exe" --ui-scenario password
& "$env:CARGO_TARGET_DIR/debug/zipelo-ui-memory-app.exe" --ui-scenario create-encrypted
~~~

개발 빌드는 목록과 오류를 콘솔에 출력한다. MSVC 아이콘 컴파일러가 없는 환경에서는 기존 정책대로 경고 후 아이콘 없이 빌드할 수 있다.

## 상황과 확인할 점

- empty: 빈 첫 화면. 파일 열기 안내와 툴바 배치를 확인한다.
- tar-bz2: TAR.BZ2 목록. 제목표시줄의 파일 이름과 목록을 확인한다.
- tar-xz: TAR.XZ 목록. 제목표시줄의 대소문자가 섞인 복합 확장자와 목록을 확인한다.
- archive: 파일과 폴더가 있는 압축파일. 이름, 크기, 잠금 아이콘과 상태 줄을 확인한다.
- folder: 사진 폴더 내부. 상위 폴더 이동과 경로 줄을 확인한다.
- many-items: 기본 5개 외에 500개 파일을 추가한 목록. 스크롤바와 행 배치를 확인한다.
- selection: 파일과 폴더 두 항목 선택. 파란 선택 배경과 선택 수를 확인한다.
- marquee: 끌어서 선택 중인 고정 화면. 반투명 파란 면, 테두리와 아래 행의 가독성을 확인한다.
- search-empty: 검색 결과 없음. 검색 입력과 빈 결과 안내를 확인한다.
- password: 암호 입력 창. 입력칸과 보기, 취소, 확인 버튼 배치를 확인한다.
- password-retry: 두 번째 암호 입력 요청. 틀린 암호 안내와 입력칸 배치를 확인한다.
- extract-progress: 압축 해제 중. 62% 진행 막대와 취소 버튼을 확인한다.
- extract-unknown: 전체 크기를 모르는 압축 해제. 진행 중 안내와 처리량을 확인한다.
- extract-complete: 압축 해제 완료. 완료 문구와 결과 행동을 확인한다.
- extract-cancelled: 취소 완료. 취소 안내와 다시 시도 배치를 확인한다.
- cancelling: 취소 요청 후 대기. 취소 중 표시와 비활성 취소 버튼을 확인한다.
- conflict: 같은 이름 파일이 있는 오류. 오류 설명과 복구 행동을 확인한다.
- unsupported: 지원하지 않는 형식. 파일 열기 실패 안내를 확인한다.
- damaged: 손상된 파일. 무결성 오류 안내를 확인한다.
- create: 암호를 사용하지 않는 ZIP 만들기 창. 원본 목록과 압축 설정을 확인한다.
- create-encrypted: 암호 설정이 켜진 ZIP 만들기 창. 두 암호 입력칸과 실행 버튼 상태를 확인한다.
- create-progress: ZIP 만들기 중. 진행 문구와 취소 버튼을 확인한다.
- create-complete: ZIP 만들기 완료. 압축 해제 완료와 다른 문구를 확인한다.
- folder-empty: 빈 하위 폴더. 목록의 빈 폴더 안내를 확인한다.
- gzip-conflict: 단일 GZ 이름 충돌. 다른 이름으로 저장 버튼과 안내를 확인한다.
- extract-partial: 일부 추출 후 실패. 이미 풀린 파일이 남는다는 경고를 확인한다.
- extract-uncertain: 저장 여부 불확실. 결과 확인을 권하는 경고를 확인한다.
- extract-unsynced: 완료 뒤 저장 확인 미완료. 완료 안내를 유지하고 자세히 보기에 경고가 있는지 확인한다.
- source-changed: 원본 변경. 다시 시도 대신 압축파일 다시 열기 버튼을 확인한다.
- dialog-error: 운영체제 파일 선택 창을 열지 못함. 파일 끌어 놓기 대안 안내를 확인한다.
- create-error: ZIP 만들기 실패. 디스크 여유 공간 안내를 확인한다.
- password-create: 암호 ZIP 생성 중 작업자 암호 요청. 암호와 암호 확인 입력 배치를 확인한다.
- close-confirm: 진행 중 창 닫기 확인. 작업 취소와 유지 선택지를 확인한다.

--ui-scenario에는 이름 하나만 지정한다. 없는 이름, 누락된 이름, 다른 옵션이나 경로와의 혼합은 사용 가능한 목록을 출력하고 실패로 끝난다. --list-ui-scenarios도 단독으로 사용한다. 기존 파일 연결 실행과 --ui-fixture의 인자 해석은 유지한다.

## 검증

~~~bash
cargo +1.99.1 test --locked --manifest-path experiments/rust-ui-memory-app/Cargo.toml --features ui-fixture --test ui_scenarios
python3 tests/ci/run_rust.py local
~~~

첫 명령은 이름 해석, 잘못된 인자, 화면 연결 없이 목록 출력, 전체 상황의 실제 egui 그리기를 확인한다. 모든 상황을 1060x720과 최소 크기 760x540에서 그린다. 별도 운영체제 창을 만들 수 없는 시험에서는 기존 내장 창 대체 배치를 사용한다. 운영체제별 창 테두리와 글꼴, 자식 창 배치는 실제 실행으로 추가 확인한다. 자동 그리기 통과만으로 Windows의 실제 화면 확인을 대신하지 않는다.

## 새 상황 추가

1. tests/rust-app-ui/scenarios.rs의 SCENARIOS를 만드는 catalog! 목록에 이름과 한국어 설명을 추가한다. 이름은 영문 소문자와 하이픈을 사용한다.
2. Scenario::build에 기존 ArchiveApp 상태를 구성하는 분기를 추가한다. 작업 상태는 FakeFactory::manual과 기존 Event로 구성하며 작업자 계약을 바꾸지 않는다. 실제 파일이나 프로세스를 사용하지 않는다.
3. tests/rust-app-ui/scenario_tests.rs에 새 상황의 화면 문구나 상태 검증을 추가한다. 전체 상황 그리기 시험은 목록에 추가된 상황도 자동으로 검사한다.
4. 이 문서에 확인할 점을 적고 대표 화면을 직접 살핀다. 새 위젯을 별도의 가짜 화면으로 다시 만들지 않는다.

화면에 새 창, 버튼, 메시지, 상태 또는 오류를 추가하거나 바꾸면 같은 PR에서 상황, 시험과 이 가이드를 갱신한다. 화면 변화가 없으면 PR 본문에 "UI 변경 없음"이라고 적는다. 사용자가 직접 올린 UI 변경에서 빠진 상황은 병합 뒤 프론트엔드 담당이 보완한다.


TAR.BZ2와 TAR.XZ의 자동 해제 폴더 이름은 미리보기에 표시되지 않는다. tests/rust-app-ui/presentation.rs의 이름 시험과 tests/rust-app-real-worker/compressed_tar_flows.rs의 실제 작업자 시험에서 복합 확장자 제거와 해제 결과를 확인한다.

## 큰 작업의 승인과 재시도

- resource-approval: 목록에서 큰 작업임을 확인한 뒤 처리 전 승인을 묻는 화면
- resource-ratio: 압축률 경고와 미확인 예상값을 표시하는 화면
- resource-retry: 출력 없이 한도에 걸린 뒤 "한도를 늘려 다시 시도"를 고르는 화면
- resource-partial: 일부 파일을 저장한 뒤 한도에 걸려 저장 위치를 안내하는 화면

일반 작업은 16 GiB와 10,000개 한도를 유지하며 압축률을 감시합니다.
목록이나 선택한 일반 파일의 크기로 한도 초과를 알면 처리 전에 작업자가 확대 승인을 묻습니다.
미리 알 수 없는 폴더 내부와 압축파일은 기본 한도로 시작합니다.
출력이 없고 임시 파일 정리와 작업자 종료가 확인된 한도 오류에서만 확대 재시도를 제공합니다.
확대 재시도를 선택해도 작업자의 승인 질문에서 "이 작업 허용"을 눌러야 진행합니다.
이미 저장되거나 일부 남은 파일, 저장 상태나 정리가 불확실한 작업은 확대 재시도를 제공하지 않습니다.

승인은 해당 작업과 질문 범위에만 적용합니다. 예상값, 실제값과 저장 볼륨의 여유 공간은 구분하며
확인하지 못한 값은 "미확인"으로 표시합니다. 여유 공간은 예약이나 모든 볼륨의 합계가 아닙니다.
승인 후에도 상한과 공간 검사를 유지합니다. 취소 버튼과 Escape는 승인 대기 중 작업을 취소합니다.
Enter만으로 허용하지 않습니다. 기존 작업자로 자동 재시도하지 않습니다.
미리보기는 입력과 파일 쓰기를 막으며 실제 64 GiB 작업을 수행하지 않습니다.

승인된 작업의 목록 한도는 200,000개입니다. 출력 기록에는 압축파일에 따로 적혀 있지 않은
상위 폴더도 포함되므로 작업자 메타데이터 정책과 같은 262,144개 노드까지 보관합니다.
기본 작업의 출력 기록 한도는 100,000개를 유지합니다. 이 기록 정책 수정은 화면 요소를 바꾸지 않습니다.

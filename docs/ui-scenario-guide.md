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
- archive: 파일과 폴더가 있는 압축파일. 이름, 크기, 잠금 아이콘과 상태 줄을 확인한다. 폴더 크기는 0 B가 아니라 그 안의 모든 파일 크기의 합(사진 폴더 2.5 MB)으로 표시한다.
- folder: 사진 폴더 내부. 상위 폴더 이동과 경로 줄을 확인한다.
- many-items: 기본 5개 외에 500개 파일을 추가한 목록. 스크롤바와 행 배치를 확인한다.
- selection: 파일과 폴더 두 항목 선택. 파란 선택 배경과 선택 수를 확인한다.
- marquee: 끌어서 선택 중인 고정 화면. 반투명 파란 면, 테두리와 아래 행의 가독성을 확인한다.
- search-empty: 검색 결과 없음. 검색 입력과 빈 결과 안내를 확인한다.
- find-folder: Ctrl+F 찾기 창. 연 폴더 아래 모든 하위 폴더에서 이름에 "jpg"가 들어간 파일을 찾은 결과입니다. 위쪽 검색어 입력란과 그 아래 세로로 긴 결과 목록, 각 결과의 이름과 상위 경로를 확인합니다. 결과를 두 번 누르거나 Enter를 누르면 찾기 창이 닫히고 그 위치로 이동하며, 파일이면 들어 있는 폴더를 열고 그 파일을 선택합니다.
- find-all: Ctrl+Shift+F 찾기 창. 압축파일 전체에서 전체 경로에 "사진"이 들어간 폴더와 파일을 찾은 결과입니다. 범위 안내 문구가 "압축파일 전체 경로에서 찾습니다"인지 확인합니다. 찾기 창에서 Ctrl+F와 Ctrl+Shift+F로 범위를 바꿀 수 있습니다.
- find-empty: 찾기 결과가 없을 때 "찾은 항목이 없습니다" 안내를 확인합니다. Esc를 누르면 찾기 창만 닫힙니다.
- open-blocked: 압축파일 안 실행 파일을 두 번 눌렀을 때 바로 열지 않는다는 안내를 확인합니다. 풀기 작업은 시작하지 않습니다.
- open-failed: 파일을 임시 폴더에 푼 뒤 기본 프로그램 실행에 실패한 안내를 확인합니다.
- open-missing: 열기용으로 푼 결과에서 고른 파일을 찾지 못한 안내를 확인합니다.
- password: 암호 입력 창. 입력칸과 보기, 취소, 확인 버튼 배치를 확인한다.
- password-retry: 두 번째 암호 입력 요청. 틀린 암호 안내와 입력칸 배치를 확인한다.

헤더까지 암호화한 7z(`-mhe=on`, 분할 포함)는 목록을 읽는 작업에서 같은 암호 창을 엽니다. 화면은 password, password-retry와 같으며 취소하면 목록 없이 끝납니다. 해제는 새 작업자이므로 암호를 다시 묻습니다. 실제 창 흐름은 tests/rust-app-real-worker/password_flows.rs에서 확인합니다.
- archive-loading: 플랫폼과 관계없이 별도 작업 창으로 재현합니다. 최초 생성에서 포커스 정보가 아직 없을 때만 창을 올리며, 다른 앱에 포커스가 있는 상태에서 다시 표시할 때는 올리지 않습니다. 명령줄이나 더블클릭으로 압축파일을 연 직후의 목록 로딩입니다. 목록을 읽는 동안에는 "이 폴더는 비어 있습니다"를 표시하지 않아야 합니다. X11에서는 처음부터 작업 창이 부모 위에 보이고 부모를 클릭해도 유지되는지 확인합니다. 다른 앱으로 전환하면 작업 창이 그 앱 뒤로 내려가야 합니다.
- archive-loading-in-parent: 플랫폼과 관계없이 부모 내부 창으로 재현합니다. 실제 앱은 Wayland에서 이 방식을 사용합니다. Wayland의 목록 로딩입니다. 네이티브 창 올리기를 지원하지 않는 환경에서는 같은 진행 화면을 부모 내부의 이동 가능한 창으로 표시합니다. 부모를 클릭해도 작업 화면이 가려지지 않고, 다른 앱이나 기본 파일 선택 창 위에 남지 않아야 합니다. 암호 입력 등 기존 모달이 열리면 작업 화면은 숨깁니다. 애니메이션은 사용하지 않습니다.
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
- extract-disk-full: 압축 해제의 원본 사본 또는 단계 임시 파일을 쓰는 도중 디스크 공간이나 할당량이 부족한 상황입니다. 공간 확보 후 재시도 안내와 "아무 파일도 풀지 않았습니다"를 확인합니다. ZIP 만들기 화면이 아닌 압축 해제 실패 화면이어야 합니다.
- extract-complete-cleanup-failed: 압축 해제와 파일 저장은 완료됐지만 출력 폴더의 임시 파일 정리에 실패한 상황입니다. "압축을 풀었습니다"와 저장 위치를 유지하면서 임시 파일이 남을 수 있다는 경고가 함께 표시되는지 확인합니다. 해제 실패나 출력 없음으로 안내하지 않아야 합니다.
- extract-cleanup-failed: 압축 해제 실패 뒤 출력 폴더의 임시 파일 이름을 지우지 못한 상황입니다. 파일 접근 오류와 함께 풀 위치 폴더에 임시 파일이 남을 수 있다는 경고를 확인합니다. 게시된 파일이 없어도 정리 실패를 표시하며, "아무 파일도 풀지 않았습니다"로 잔여 파일이 없다고 단정하지 않아야 합니다.
- extract-partial: 일부 추출 후 실패. 이미 풀린 파일이 남는다는 경고를 확인한다.
- extract-uncertain: 저장 여부 불확실. 결과 확인을 권하는 경고를 확인한다.
- extract-unsynced: 완료 뒤 저장 확인 미완료. 완료 안내를 유지하고 자세히 보기에 경고가 있는지 확인한다.
- source-changed: 원본 변경. 다시 시도 대신 압축파일 다시 열기 버튼을 확인한다.
- dialog-error: 운영체제 파일 선택 창을 열지 못함. 파일 끌어 놓기 대안 안내를 확인한다.
- create-error: ZIP 만들기 실패. 디스크 여유 공간 안내를 확인한다.
- password-create: 암호 ZIP 생성 중 작업자 암호 요청. 암호와 암호 확인 입력 배치를 확인한다.
- close-confirm: 진행 중 창 닫기 확인. 작업 취소와 유지 선택지를 확인한다.
- uninstall-windows: Windows 제거 전 다른 기본 앱 선택 안내와 설정 열기 버튼 문구를 확인한다.
- uninstall-settings-error: Windows 설정 실행 실패 시 수동 설정 경로를 확인한다.
- uninstall-ubuntu-missing: 이전 앱이 없어 복원하지 못한 경우의 터미널 안내 문구를 확인한다.
- uninstall-ubuntu-unknown-user: 제거 사용자를 확인하지 못해 설정을 유지하는 안내 문구를 확인한다.

## 창 밖으로 끌어서 풀기

Wayland에서 목록의 행을 누른 채 끌면 운영체제 끌어 놓기를 시작합니다. 선택하지 않은 행을 끌면 그 행만 선택합니다. 새 창, 버튼이나 문구는 없으며 풀기 진행 중에는 기존 작업 창이 보이고, 끝나면 결과 창 없이 받는 프로그램이 파일을 가져갑니다. 놓지 않고 취소하면 풀던 작업을 취소하며 취소 결과 창도 띄우지 않습니다. 화면 변화가 없으므로 상황은 추가하지 않았고, 흐름은 tests/rust-app-ui/drag_out_flow.rs의 가짜 끌기 구현으로 확인합니다. 실제 파일 관리자와 브라우저에 놓기는 Ubuntu Wayland 실기로 확인합니다. X11과 Windows에서는 행을 끌어도 아무 동작을 하지 않습니다.

## Esc 키

자식 창(찾기 창, 작업 결과 창, 압축 파일 만들기 창, 미리보기, 암호 입력, 큰 작업 승인, 파일 충돌 선택, 종료 확인 등)이 떠 있으면 Esc는 그 창을 닫거나 취소합니다. 진행 중인 작업 창은 Esc로 닫지 않으며 [취소] 버튼을 사용합니다. 목록에 선택한 항목이 있으면 Esc는 먼저 선택을 해제하고, 입력란에 초점이 있으면 초점만 해제합니다. 아무 자식 창도 없으면 Esc로 프로그램을 종료하며, 작업 중이면 기존 종료 확인 창을 거칩니다. 미리보기 상황은 입력을 받지 않으므로 Esc 동작은 tests/rust-app-ui/explorer_selection.rs와 실제 실행으로 확인합니다.

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


## 파일 충돌 선택과 결과

- gzip-conflict-choice: 단일 GZ 파일의 충돌 선택 창. 저장 창에서 고른 이름과 세 선택을 확인한다.
- conflict-choice: 파일 경로와 기존 크기, 새 크기 및 덮어쓰기, 건너뛰기, 이름 바꿔 저장을 확인합니다. Tab으로 이동하고 Space로 선택할 수 있으며 Escape 또는 작업 취소는 전체 작업을 취소합니다.
- conflict-apply: "나머지 파일에도 같게 적용"이 켜진 상태입니다. 현재 작업의 남은 일반 파일 충돌에만 적용하며 이름 바꿔 저장은 같은 폴더에 번호를 붙입니다.
- extract-skipped: 모든 파일을 건너뛴 정상 완료입니다. 생성된 파일이 없으며 저장 실패 경고가 없어야 합니다.
- extract-renamed: 실제 저장된 새 이름을 표시합니다. 이름 변경 결과만으로 저장 성공을 판정하지 않고 별도의 생성 보고와 완료 집계가 일치해야 합니다.
- extract-rename-failed: 새 이름을 정한 뒤 실제 생성 보고 없이 I/O 오류가 난 결과입니다. 실패 원인과 "아무 파일도 풀지 않았습니다"를 표시하고 "이름 바꿔 저장" 완료 안내는 표시하지 않아야 합니다. 일부 파일이 저장된 뒤 실패한 경우에도 생성 보고가 있는 이름만 저장 완료로 표시합니다.

실제 추출은 revision 3의 StartGuarded에 interactive=true를 보내며 ZIP, 7z, TAR 계열과 단일 GZ의 파일 충돌에 같은 선택 창을 사용합니다. 목록 읽기와 ZIP 만들기도 StartGuarded로 자원을 감시하며 충돌 질문은 추출에만 사용합니다. 위 명령은 실제 파일을 쓰지 않는 개발용 화면 미리보기입니다. 파일 기반 버튼 연동은 tests/rust-app-real-worker/conflict_flows.rs에서 확인하며, 운영체제별 실제 창 확인은 별도입니다.

TAR.BZ2와 TAR.XZ의 자동 해제 폴더 이름은 미리보기에 표시되지 않는다. tests/rust-app-ui/presentation.rs의 이름 시험과 tests/rust-app-real-worker/compressed_tar_flows.rs의 실제 작업자 시험에서 복합 확장자 제거와 해제 결과를 확인한다.

## 큰 작업의 승인과 재시도

- resource-approval: 목록에서 큰 작업임을 확인한 뒤 처리 전 승인을 묻는 화면
- resource-ratio: 압축률 경고와 미확인 예상값을 표시하는 화면
- resource-retry: 출력 없이 한도에 걸린 뒤 주 창 안의 모달에서 "한도를 늘려 다시 시도"를 고르는 화면
- resource-partial: 일부 파일을 저장한 뒤 한도에 걸려 주 창 안의 모달에서 저장 위치를 안내하는 화면

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

한도 초과 결과는 별도 운영체제 창 대신 주 창 안의 모달로 표시합니다. Wayland에서도 파일 경로로 실행한 직후 Alt+Tab 없이 결과와 재시도 버튼이 보여야 합니다. "확인"으로 닫으며, 일부 출력이 남은 경우 확대 재시도는 제공하지 않습니다.

큰 작업을 허용한 뒤 파일 충돌이 발견되면 승인 창을 닫고 충돌 선택 창을 표시합니다. 승인과 충돌 질문은 동시에 표시하지 않으며, 같은 작업의 질문 번호를 이어서 검증합니다.
## 설치 프로그램의 제거 안내

Windows NSIS 제거 프로그램은 앱과 별도의 실행 파일입니다. 이번 제거 안내는 Uninstall.exe 실행 후 확인 화면 앞에 표시되며 [기본 앱 설정 열기]로 Windows 설정을 엽니다. --ui-scenario uninstall-windows와 uninstall-settings-error로 안내 문구를 확인합니다. Ubuntu 터미널 안내는 uninstall-ubuntu-missing과 uninstall-ubuntu-unknown-user로 확인합니다. 이 네 상황은 외부 설치 프로그램과 터미널의 문구를 egui에 재현한 읽기 전용 미리보기이며, NSIS의 실제 배치나 운영체제 창 모양을 재현하지 않습니다. 버튼은 설정을 열거나 제거를 실행하지 않습니다. 다른 운영체제에서도 같은 안내를 확인할 수 있습니다.

실제 검증은 전용 Windows VM에서 설치와 기본 앱 지정 후 Uninstall.exe를 실행합니다. 설정에서 ZIP의 다른 기본 앱을 선택하고 제거한 뒤 ZIP을 열어야 합니다. 설정을 열지 않고 제거한 경우의 [연결 프로그램] 경로, 취소 시 앱 유지, /S 무인 제거의 창 생략, 사용자가 바꾼 등록 값 보존도 확인합니다. NSIS 컴파일 시험은 이 운영체제 화면 검증을 대신하지 않습니다.

Ubuntu 제거 안내는 패키지 관리자 터미널 출력입니다. 이전 기본 앱이 있는 경우, 삭제된 경우, 기록이 없는 경우, 설치 후 다른 앱으로 바꾼 경우와 다른 사용자의 제거를 구분해 확인합니다. 사용자 홈 전체나 MIME 설정 파일 전체를 초기화하지 않습니다.

### 파일 미리보기

- preview-catalog-bytes-approval: 전체 목록 크기가 16 GiB를 넘는 압축파일에서 7바이트 텍스트를 미리볼 때 새 승인을 요청. 미리보기 고유 상한은 유지.
- preview-catalog-approval: 10,001개 목록에서 파일 하나를 미리볼 때 목록 전체 처리 예산의 새 승인을 요청. 텍스트 1 MiB와 이미지 8 MiB 고유 상한은 유지.
- preview-loading: 미리보기 로딩 중 [취소] 또는 Esc로 닫기. 닫힌 결과는 늦게 도착해도 표시하지 않음.
- preview-text: UTF-8 텍스트의 읽기 전용 표시와 스크롤.
- preview-image: 디코딩된 정적 이미지의 화면 맞춤 표시.
- preview-empty: 빈 텍스트 파일 안내.
- preview-limit: 크기 또는 처리 한도 초과 안내. 미리보기 상한을 늘리는 버튼은 제공하지 않음.
- preview-damaged: 손상 또는 잘못된 UTF-8 안내.
- preview-unsupported: 지원하지 않는 형식 안내.
- preview-implicit-folder: 디렉터리 항목 없이 파일 하나만 포함한 가상 폴더 선택 시 [미리보기] 비활성화. 폴더 안에서 파일을 선택하면 활성화.
- preview-selection: 폴더 또는 여러 항목 선택 시 [미리보기] 비활성화.

`--ui-scenario` 화면은 기존 규칙대로 입력을 받지 않는 디자인 점검용이다.
키보드 상호작용은 `--ui-fixture /absolute/example.zip` 또는 실제 압축파일로 확인한다.
파일을 고르고 Tab으로 [미리보기]에 이동해 Enter로 열며, 닫기 버튼 또는 Esc로 닫는다.
실제 압축파일에서는 파일 하나를 선택하고 [미리보기]를 직접 눌렀을 때만 읽기를 시작한다.
닫은 뒤 선택, 폴더 위치와 스크롤을 유지하고 실행 전 키보드 포커스로 돌아간다.
시나리오의 텍스트와 이미지는 화면 점검용이며 디코더 검증은 실제 압축파일로 별도 수행한다.

### 분할 7z

조각 오류와 제외 형식 안내는 주 창 안의 작업 결과 모달로 표시한다. 별도 결과 창 전환 없이 파일명과 재시도 방법을 확인할 수 있다.

- split-archive: .7z.001 목록과 미리보기 비활성화. 분할 입력은 목록 보기와 풀기만 지원.
- split-first-volume: .7z.002 등 선택 시 필요한 .001의 정확한 이름 안내. 자동으로 다른 파일을 열지 않음.
- split-missing-volume: 확정된 중간 누락의 파일명과 같은 폴더에 모든 조각을 모은 뒤 .001을 다시 여는 방법 안내.
- split-incomplete: 불완전하거나 손상된 묶음 안내. 마지막 누락 파일명을 추측하지 않음.
- split-unsupported: 실제 분할 ZIP의 UnsupportedFeature 오류로 분할 ZIP과 RAR 미지원 안내를 표시한다. UnsupportedFormat도 같은 안내를 사용한다.
- split-source-changed: 목록 이후 조각 변경 시 다시 열기 안내.

운영체제 기본 열기 창에서 같은 폴더의 첫 조각 `.7z.001`을 선택한다. 분할 묶음은 최대 999개이며 목록 열람 성공은 전체 데이터 무결성 검증 완료를 뜻하지 않는다. 실제 분할 파일 처리는 작업자 구현 후 독립 도구 표본으로 확인한다. Windows와 Wayland에서 첫 조각 선택, 누락 이름, .001 재시도 안내와 닫기를 확인하며, 위 디자인 상황은 실제 파일 검증을 대신하지 않는다.

## 7z 만들기 상황

- create-sevenzip: 기존 만들기 창의 ZIP/7z 선택, LZMA2 고정과 암호 미지원 안내.
- create-sevenzip-progress: 7z 생성 진행과 취소 버튼.
- create-sevenzip-complete: 7z 파일의 저장 완료와 실제 저장 경로.
- create-sevenzip-error: 7z 저장 실패와 재시도.
- create-sevenzip-cancelled: 게시 전 취소 완료와 출력 없음.
- create-sevenzip-resource: 7z 생성의 큰 작업 허용 질문.
- create-sevenzip-split: 7z를 고르면 암호 안내 아래에 보이는 "쪼갤 용량 설정" 체크와 볼륨 크기 칸(정수)과 MB/GB 선택, `이름.7z.001, 이름.7z.002 …` 안내와 최대 999개 안내.
- create-sevenzip-split-invalid: 0처럼 1 MB보다 작거나 정수가 아닌 볼륨 크기의 빨간 안내와 비활성 압축하기 버튼.
- create-sevenzip-split-complete: 볼륨 세 개로 나눈 7z의 완료, 첫 볼륨(`.7z.001`) 경로와 볼륨 수 안내.

7z를 선택하면 입력한 ZIP 암호와 실행 취소 기록을 지우며, ZIP으로 돌아와도 암호를 복원하지 않는다. 기본 저장 창은 .7z를 제안하며 암호가 필요하면 ZIP을 선택한다. 이 상황들은 가짜 작업자로 화면을 구성하므로 실제 7z 생성과 독립 도구 호환 시험을 대신하지 않는다.

쪼갤 용량은 7z에서만 보이며 1 MB = 1,048,576바이트로 계산한다. 저장 창에서 `이름.7z`를 고르면 같은 폴더에 `이름.7z.001`부터 차례로 만들고 `이름.7z` 자체는 만들지 않는다. 같은 폴더에 `이름.7z.<숫자>`가 하나라도 있으면 분할 읽기가 한 묶음으로 보므로 시작하지 않는다. 작업자는 모든 볼륨을 임시 파일로 쓴 뒤 한 번에 게시하며, 중간에 실패하거나 취소하면 이미 게시한 볼륨까지 지운다. 볼륨이 999개를 넘으면 한도 오류로 끝나고 쪼갤 용량을 늘리라고 안내한다. 실제 볼륨 크기와 분할 읽기 왕복은 tests/rust-archive-worker/seven_zip_split_create.rs에서 확인한다.

### ZIP 복구

손상 오류에서 "복구본 저장 위치 선택"을 누르면 운영체제 기본 저장 창이 열립니다.
원본을 수정하지 않고 새 ZIP을 만들며, 기존 파일은 덮어쓰지 않습니다.
복구본 저장 완료와 일부 복구를 구분하고, 일부 복구본을 열어도 유실 경고를 유지합니다.
검증된 항목도 손상 전 원본과 동일하다는 보증은 아닙니다.

- recovery-progress: 항목 분석 중에는 전체 개수나 완료율을 추정하지 않습니다.
- recovery-complete: 검증과 저장, 작업자 종료를 확인한 복구본입니다.
- recovery-partial: 유실 항목을 구분하고 저장된 부분 복구본을 열 수 있습니다.
- recovery-unknown: 전체 목록을 알 수 없어 추가 유실 여부를 확인할 수 없습니다.
- recovery-impossible: 복구 가능한 항목과 생성된 복구본이 없습니다.
- recovery-cancelled: 취소 종료와 출력 없음 상태입니다.
- recovery-retained: 저장 실패 뒤 남은 임시 파일의 실제 경로와 정리 경고를 표시합니다. 완성된 복구본으로 열지 않습니다.
- recovery-save-error: 분석이 끝났어도 저장 실패를 완료로 표시하지 않습니다.
- recovery-resource: 원본 제한은 유지하면서 압축률 예산 승인을 확인합니다.

예: `Zipelo --ui-scenario recovery-partial`. 상황 명령은 가짜 작업자만 사용합니다.

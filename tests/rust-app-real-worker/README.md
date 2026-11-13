# 실제 작업자와 화면 연결 시험

기본 앱은 실행 파일과 같은 폴더의 zipelo-archive-worker(Windows는 .exe)를 실행함.
PATH나 현재 작업 폴더에서 임의 작업자를 검색하지 않음. ui-fixture와 --ui-fixture가
모두 지정된 경우에만 기존 가짜 작업자를 사용함.

클라이언트는 작업자 crate의 protocol과 wire codec을 재사용하며 압축 처리와 파일 접근은
별도 작업자 프로세스에서 수행함. UI에서 파이프 쓰기를 기다리지 않도록 8프레임 요청 큐와
64이벤트 응답 큐를 사용함. 파이프 입출력과 child wait는 백그라운드에서 처리함.
정상 종료에는 Terminal, 양쪽 EOF와 child wait를 모두 확인함. 통신 오류는 성공으로
표시하지 않으며, 오류 경로에서는 child 회수와 모든 파이프 스레드/핸들 정리가 끝났다는
명시적 완료 신호까지 확인함. 연결이 예외적으로 폐기돼도 소유한 자식을 종료하고 회수함.
확대 승인 전에는 기존 310초 제한과 강제 종료 및 회수를 유지함. 유효한 자원 질문과 일치하는 허용 응답이 요청 큐에 들어간 뒤에는 앱의 전체 시간 제한을 해제함. 승인 뒤 실제 진행 없는 60초와 질문별 응답 대기 300초는 작업자의 Watchdog에서 판단하며, 앱은 응답 프레임을 실제 진행으로 추정하지 않음. 암호/충돌/자원 질문에 답하는 시간은 정체 시간에 포함하지 않음. 완료 여부가 불명확하면 닫기를 허용하지 않음.

## 실행

저장소 루트에서 로컬 실행기를 사용하면 실제 작업자와 probe 빌드가 화면 시험보다 먼저 수행됨.

```sh
python3 tests/ci/run_rust.py local
```

단독 시험에서는 같은 target 디렉터리에 먼저 두 실행 파일을 빌드할 것.

```sh
export CARGO_TARGET_DIR="$PWD/tests/ci/target"
cargo +1.99.1 build --locked --manifest-path experiments/rust-archive-worker/Cargo.toml
cargo +1.99.1 build --locked --manifest-path tests/rust-app-real-worker/Cargo.toml
cargo +1.99.1 test --locked --all-features --manifest-path experiments/rust-ui-memory-app/Cargo.toml --test real_worker
```

기본 앱의 직접 실행도 같은 target 디렉터리를 사용할 것.

```sh
cargo +1.99.1 run --locked --manifest-path experiments/rust-ui-memory-app/Cargo.toml --features ui -- /absolute/sample.zip
```

## 확인 범위

- 합성 ZIP과 TAR.GZ를 실제 파일 열기 위젯으로 열고 목록을 확인한 뒤 풀기 버튼으로 해제함.
- ZIP 만들기 위젯으로 합성 파일을 압축하고, 결과를 다시 열고 풀어 바이트가 같은지 확인함.
- 위 세 흐름은 실제 작업자 EXE와 OS 파이프를 사용하며 Terminal, 양쪽 EOF, child wait와 연결 정리 순서를 관측함.
- 누락된 작업자, 잘린 프레임, 잘못된 action 승인, Terminal 없는 조기 종료를 오류로 처리함.
- 첫 Start 직렬화 실패도 실제 작업자 회수까지 대기함. Linux에서는 실행 중인 probe 연결을 폐기한 후 /proc에서 소유 자식이 사라진 것도 확인함.
- flows.rs의 창은 egui Context에서 실제 위젯에 포인터와 키보드 이벤트를 전달함. 네이티브 OS 창/입력 자동화나 Windows 실행 검증은 아님.

fixtures/는 generate.py의 Python 표준 라이브러리로 생성한 작은 합성 입력임.
정상 흐름은 제품 작업자를 사용하고, probe.rs는 통신 실패와 생명주기 시험 전용임.
사용자 자료를 읽거나 출력하지 않으며 모든 시험 출력은 이 폴더의 runs/ 안에만 생성함.
암호 ZIP, 대용량 성능, OS sandbox, 악성 파일 전체 검증과 Windows 실기는 이 시험의 범위 밖임.

취소와 종료 경합 회귀 시험은 실제 작업자에 List를 보낸 뒤 화면 poll 전에 취소하는 흐름을 100회 확인함. 취소 파이프 쓰기가 실패해도 검증된 Terminal과 성공 종료가 있으면 원래 결과를 보존하며, Terminal이 없는 조기 종료는 계속 실패로 처리함. 취소 쓰기 오류 판단을 미루어도 EOF와 child wait 전에는 닫기나 작업 교체를 허용하지 않음.

암호 화면 흐름

password_flows.rs는 공개 시험 암호 fixture-password를 사용하는 기존 합성 ZIP 6개로
ZipCrypto와 AES의 Store/Deflate 목록 및 해제를 실제 화면 위젯과 작업자로 확인함.
목록은 암호 없이 표시하며 해제 시 작업자의 challenge_id와 attempt로 암호를 요청함.
틀린 암호 뒤 재입력, 3회 실패, 빈 입력과 UTF-8 바이트 한도 초과, 대기 중 취소,
첫 실행의 AES ZIP 생성 및 재열기/해제를 검증함. 실패와 취소 중 출력이 없고 기존 파일과
원본이 보존되며 Terminal, 양쪽 EOF 및 child wait 후 연결이 정리되는지 확인함.
암호 표시를 가리고 재요청, 취소 및 창 해제 때 앱이 소유한 입력 버퍼를 지움.
Windows 실행이나 네이티브 OS 창 입력 자동화 검증은 포함하지 않음.

7z 앱 연동은 revision 2의 기존 Format 값 0~3을 유지하고 SevenZip 값 4를 추가함.
formats.rs는 전체 ListEnd 프레임을 고정 바이트와 비교하고 Unix/Windows 식별자가
목록 응답과 해제 요청 사이에서 보존되는지 확인함. 화면 흐름 시험은 PR69의 LZMA와
LZMA2 표본을 실제 작업자로 열고, 받은 ListId로 풀어 원본 바이트를 확인함.
구버전 앱은 새 형식 값을 해석할 수 없으므로 앱과 작업자를 함께 갱신해야 함.

가상 시계 회귀 approved_password_wait_is_not_killed_by_app_deadline은 정상 시계 대조군, 승인 뒤 관리 스레드의 311초 경과와 암호 대기, 승인하지 않은 작업의 기존 강제 종료를 실제 작업자 파이프로 검증함. 시계는 별도 시험 프로세스의 관리 스레드에만 적용하며 실제 310초를 기다리지 않음.

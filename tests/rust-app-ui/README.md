# 앱 화면 흐름 시험

가짜 작업자는 계약 revision 2의 의미를 따르는 프론트엔드 어댑터임. 바이너리 codec, 실제 프로세스 실행, 파일 읽기와 쓰기, 암호 해독은 수행하지 않음. 실제 codec은 backend 소유이며 이 PR에서 중복 구현하지 않음.

`python3 tests/ci/run_rust.py local`은 기존 시험과 새 `archive_ui` 시험을 all-features로 실행함. Windows 결과는 교차 빌드이며 Windows 실제 실행은 별도 확인해야 함.

화면 직접 확인:

```sh
cargo +1.99.1 run --locked --manifest-path experiments/rust-ui-memory-app/Cargo.toml --features ui-fixture -- --ui-fixture /absolute/example.zip
```

`locked.zip` 이름은 가짜 암호 요청을 만듦. 아무 비어 있지 않은 합성 암호를 받으며 암호를 기록하거나 저장하지 않음. 실제 암호나 사용자 자료를 입력할 필요가 없음. 시험 모드는 화면에 명시되고 실제 파일을 만들지 않음. `ui-fixture` 없이 실행하면 앱과 같은 폴더의 실제 zipelo-archive-worker를 실행함. Windows에서는 .exe 확장자를 사용함. 작업자가 누락되면 오류를 표시하며 가짜 성공으로 대체하지 않음. 실제 작업자 연결 시험은 ../rust-app-real-worker/README.md를 참고할 것.

범위는 목록 확인, 선택 해제, GZ 출력명, ZIP 생성, 암호 요청, 진행률, 취소와 닫기 철회, Terminal/EOF/child wait 분리, 오래된 ID/sequence 거부, SourceChanged 재열람, 부분 출력과 불확실 게시 보존임. 구성한 이벤트 시험은 실제 OS 종료나 파일 보안 검증의 증거가 아님.

디자인은 흰 바탕 #FFFFFF, 글자 #182739, 선택 #D3E6F6, 강조 #174C7C를 사용함. 기존 한글 시스템 폰트와 본문/버튼 18px, 제목 26px를 유지함. 상단 작업 버튼, 가운데 파일 목록, 하단 진행/결과의 왼쪽 정렬 구조이며 작은 부가 설명, 장식 카드와 새 아이콘을 추가하지 않음.

풀기 버튼은 raw GZ에만 출력명을 보내며 ZIP/TAR/TAR.GZ에는 None을 보냄. 모델과 가짜 작업자도 이 계약을 검증함. Start 전송 실패 시에는 연결을 보존하고 추가 요청을 막으며, 양쪽 EOF와 child wait가 모두 올 때까지 교체와 닫기를 허용하지 않음. 이때 Terminal이 없어도 종료 후 오류 확인 화면으로 이동하며 성공으로 표시하지 않음.

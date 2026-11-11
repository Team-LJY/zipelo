# Rust archive worker

별도 프로세스의 stdin/stdout으로 revision 2 요청을 처리합니다. 제품 앱 연결과 OS sandbox를 제공하는 단계는 아닙니다. 암호 기능은 capability=false이며 암호 ZIP 해제 및 AES 생성 요청은 UnsupportedFeature로 종료합니다.

- ZIP32 Store/Deflate 목록, 해제, 생성, 기본 TAR/USTAR, GZ, TAR.GZ 해제를 지원합니다. GZ 연결 멤버는 하나의 논리 스트림으로 합칩니다. ZIP64, 다중 볼륨, 비 UTF-8 ZIP 이름, PAX/GNU 확장과 특수 항목 해제는 미지원입니다.
- 입력은 최대 64 MiB, 해독/생성 출력은 최대 128 MiB, 항목은 최대 10,000개입니다. 요청 limits는 이 값을 낮출 수 있습니다. 입력과 검증 완료 payload를 메모리에 보관해 모든 선택 출력의 CRC, 크기와 경로를 쓰기 전에 검사합니다. checkpoint마다 취소와 300초 한도를 확인합니다.
- 경로 깊이는 64개 구성 요소, 이름/출력 계획 메타데이터 예산은 8 MiB, 출력 계획 노드는 32,768개로 제한합니다. 이름 복제와 인덱스 비용을 보수적으로 계산해 할당 전에 Limit으로 거부합니다.
- GZip 내부 형식은 완전한 USTAR magic이 있을 때만 TAR로 판별합니다. magic이 없는 영값 데이터는 길이와 무관하게 raw GZip으로 유지하며 명시 출력명이 필요합니다.
- ListId는 열린 파일 객체, 전체 입력 SHA-256과 catalog version에 연결됩니다. 입력을 다시 읽어 검증한 전용 메모리 스냅샷만 해독합니다. 불일치는 출력 전 SourceChanged이며 목록을 다시 열어야 합니다. 원본의 사후 변경 감시나 파일 잠금 보장은 아닙니다.
- 출력 폴더는 미리 존재해야 합니다. Linux는 소유 UID가 같고 group/other 쓰기가 없는 폴더만 허용합니다. openat/O_NOFOLLOW로 디렉터리를 고정하고 새 임시 파일을 배타적으로 만든 뒤 hard link로 게시합니다. 기존 파일과 링크를 덮어쓰지 않습니다. Windows는 reparse point를 거부하고 부모 디렉터리 핸들에서 delete sharing을 제한합니다.
- 파일 sync/close는 게시 전에 확인합니다. 게시 후 부모 sync와 임시 파일 정리 실패는 게시 사실과 따로 보고합니다. Windows의 디렉터리 내구성은 Unconfirmed입니다. hard link의 결과가 불명확하면 CommitUncertain/Uncertain이며 자동 재시도나 rollback은 없습니다.
- 생성한 디렉터리와 게시한 파일은 취소 시 유지하고 Output으로 보고합니다. 실패한 임시 파일 정리는 실제 임시 이름과 PartialRetained로 보고합니다. 실파일 close/sync/link 장애 주입과 Windows 실기 검증은 아직 수행하지 않았습니다.

## 공유 codec

`protocol.rs`는 요청과 이벤트 타입, `wire.rs`는 의존성 없는 LE codec입니다. 앱은 이 라이브러리의 타입과 Reader/Writer를 사용할 수 있습니다. `contract.json`은 합의한 revision 2 원문입니다.

헤더는 ZAW1, version u16=1, kind u16, body length u32, reserved u32=0, job ID u64, sequence u64의 32바이트입니다. 양방향 sequence는 각각 0부터 시작합니다. body는 최대 1 MiB이며 한 방향당 최대 1,000,000프레임입니다. 잘린 프레임, 알 수 없는 tag, 잔여 payload, 잘못된 ID/sequence를 거부합니다.

- kind: Start=1, Password=2, Cancel=3, Accepted=100, Entry=101, ListEnd=102, PasswordRequired=103, Progress=104, Output=105, Terminal=106, CancelAck=107입니다.
- body 필드는 `protocol.rs` 선언 순서입니다. enum은 선언 순서의 u8 tag이며 NativePath/SourceObject만 Unix=1, Windows=2입니다. bool/Option은 0 또는 1입니다. String/Vec는 u32 길이 뒤 원소를 저장하며 String은 UTF-8 바이트 길이입니다. `[u8; N]`은 길이 없이 N바이트입니다. action variant의 필드는 선언 순서를 사용합니다. Start는 action 뒤 limits(input u64, output u64, entries u32)입니다.
- Password의 비밀 값은 Debug에 노출되지 않습니다. 이 버전은 PasswordRequired를 보내지 않으므로 들어온 Password는 Protocol 오류입니다. 비밀 값은 로그나 파일에 저장하지 않습니다.
- CancelAck.request_sequence는 요청 헤더 sequence입니다. Requested, AlreadyRequested, TooLate, Finished는 최종 결과와 별개입니다. 종료와 경합한 요청은 읽히지 않아 응답이 없을 수 있습니다. Terminal은 마지막 프레임이며 Failed/Cancelled도 정상 전달 뒤 프로세스 exit=0입니다. 앱은 Terminal과 stdout/stderr EOF, child wait를 모두 확인해야 합니다.

## 검증과 의존성

시험은 `tests/rust-archive-worker`에 있습니다. Python 표준 라이브러리로 만든 ZIP/TAR/GZ fixture와 실제 자식 프로세스를 사용합니다. 전체 검증 명령은 저장소 루트의 `python3 tests/ci/run_rust.py local`입니다. Windows 교차 빌드는 실제 Windows 실행 검증을 대신하지 않습니다.

직접 의존성은 기존 zlib-rs=0.6.8와 허용된 RustCrypto sha2=0.10.9입니다. 2026-11-11에 확인한 [zlib-rs RUSTSEC-2024-0401](https://rustsec.org/advisories/RUSTSEC-2024-0401.html)은 >=0.4.0, [sha2 RUSTSEC-2021-0100](https://rustsec.org/advisories/RUSTSEC-2021-0100.html)은 >=0.9.8을 수정 버전으로 명시합니다. 선택 버전은 이 수정 범위에 들어갑니다. 이는 전체 전이 의존성의 취약점 부재나 완전한 보안 감사를 의미하지 않습니다.

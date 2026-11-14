# 7z 생성 설계와 작업자 계약안

상태: frontend 계약 동의와 독립 설계 리뷰 완료. [앱 계약 승인](https://github.com/Team-LJY/zipelo/pull/127#pullrequestreview-5903269732)과 [설계 리뷰](https://github.com/Team-LJY/zipelo/pull/127#pullrequestreview-5903282961)는 03ca5c5 기준이다. 공용 타입, codec과 contract.json을 반영했으며 실제 작성기, 의존성 기능 변경과 화면 연동은 아직 없다. 현재 모든 CreateSevenZip 요청은 Accepted(encryption=false) 뒤 자원 질문이나 파일 접근 없이 UnsupportedFeature로 종료한다. 아래 작성기 설계와 수용 시험은 구현/호환 통과를 뜻하지 않는다. [PLANS 8.9](PLANS.md#89-7z-만들기)와 [8.13](PLANS.md#813-진행과-완료-판단)을 따른다.

## 1. 이번 지원 범위

- 생성 형식은 단일 .7z이고 기본 압축은 LZMA2다. 일반 파일, 한글을 포함한 UTF-8 상대 이름, 빈 파일과 모든 폴더 항목을 보존한다. 원본 폴더를 지정하면 그 이름 아래의 상대 구조를 보존한다.
- non-solid로 고정한다. 데이터가 있는 파일마다 독립된 LZMA2 coder와 pack stream을 만들고 파일 사이에 사전을 공유하지 않는다. 파일을 골라 풀 때 선행 파일을 처리하지 않으며 여러 파일의 압축률 최적화는 후속 범위다.
- 본문 암호화와 헤더 암호화는 지원하지 않는다. 파일명도 공개된다. 암호가 필요한 사용자는 기존 AES ZIP 생성을 사용한다. 7z를 선택한 상태에서 암호를 무시하거나 ZIP으로 바꾸어 저장하지 않는다.
- solid, 암호화, 분할 생성, SFX, 기존 아카이브 수정, 필터 체인과 사용자 지정 사전/스레드/압축 등급은 이번 범위에서 제외한다. 링크와 특수 파일을 일반 파일로 변환하지 않는다. RAR 제외 결정을 유지한다.
- 파일 내용과 상대 경로, 빈 파일/폴더 구조가 보존 대상이다. 원본 소유자, ACL, xattr와 모든 시각/실행 권한의 복원 및 생성 바이트의 재현성은 약속하지 않는다.

## 2. 라이브러리 선택과 고지

우선안은 이미 고정된 lzma-rust2 =0.25.0을 유지하고 현재 std,xz에 encoder 기능만 더하는 것이다. 컨테이너는 아래의 좁은 7z 작성기를 자체 MIT 코드로 구현한다. 압축 알고리즘 자체를 새로 구현하거나 외부 7z 명령을 제품에서 호출하지 않는다.

- [lzma-rust2 manifest](https://docs.rs/crate/lzma-rust2/latest/source/Cargo.toml.orig)는 열람 시 0.25.0, Apache-2.0을 명시한다. encoder 기능은 별도 전이 의존성이 없는 기능이며 Rust 인코더를 활성화한다. liblzma는 upstream 개발 의존성이므로 제품에 넣지 않는다. default-features=false와 기존 기능을 명시적으로 유지하며 optimization과 다중 스레드 작성기는 사용하지 않는다. [라이선스 원문](https://docs.rs/crate/lzma-rust2/latest/source/LICENSE)을 함께 확인했다.
- [sevenz-rust2 manifest](https://docs.rs/crate/sevenz-rust2/latest/source/Cargo.toml.orig)는 열람 시 0.24.1, Apache-2.0이며 compress만 활성화하는 컨테이너 작성기 후보다. util의 경로 재귀 수집 대신 검증된 핸들을 넘겨야 한다. 이 버전은 lzma-rust2 0.23 계열을 요구하므로 현재 0.25.0과 중복될 수 있고 crc32fast 등도 추가된다. 넓은 작성기 대신 기존 입력/예산/취소 정책에 맞춘 작은 컨테이너를 우선 검토한다. 이는 크기나 성능을 측정해 얻은 결론은 아니다.
- 기존 sevenz-rust는 후보에서 제외한다. [RUSTSEC-2026-0246](https://rustsec.org/advisories/RUSTSEC-2026-0246.html)은 유지보수 중단을, [RUSTSEC-2026-0245](https://rustsec.org/advisories/RUSTSEC-2026-0245.html)는 그 패키지의 해제 API 경로 이탈을 알린다. 이 권고는 이름이 다른 sevenz-rust2나 lzma-rust2의 취약점 유무를 판정하는 근거가 아니다.
- 전체 최신 권고 DB 감사와 새 encoder 실행 시험은 하지 않았다. 구현 PR에서 고정 버전과 실제 기능 트리의 RustSec 권고 적용 여부를 다시 확인하고 확인 불가 항목을 기록한다. 후보 선택만으로 취약점 부재나 배포 적합성을 선언하지 않는다.
- 도입 PR에는 선택 이유와 정확한 활성 기능을 적는다. packaging/licenses.py의 실제 작업자 Cargo.lock 고지에 lzma-rust2의 버전, Apache 원문과 원문 해시가 포함되는지 회귀로 확인한다. encoder 추가 전후의 전이 의존성을 비교하고 새 원문이 빠지면 배포 검증을 실패시킨다. 고지 생성기의 공용 변경이 필요하면 담당 범위를 합의한 뒤 수정한다. 이번 설계 PR에는 라이브러리를 도입하지 않는다.

## 3. 작성기와 헤더

- 입력 수집은 create_sources::Plan과 Source, 검증한 파일 핸들 및 spool::Snapshot을 재사용한다. 라이브러리에 원본 경로를 넘겨 다시 열거나 자체 디렉터리 탐색을 시키지 않는다. 원본은 읽기 전용으로 열며 사용자가 고른 출력 경로를 쓰기/자르기로 직접 열지 않는다.
- 단일 스레드 Lzma2Writer를 사용하고 preset 1에서 사전을 1 MiB로 명시한다. 사용자 옵션으로 확대하지 않는다. 새 인코더는 이전 파일의 상태를 유지하지 않는다. [upstream 작성기와 메모리 추정 API](https://docs.rs/crate/lzma-rust2/latest/source/src/enc/lzma2_writer.rs)를 근거로 인코더 생성 전 추정치의 KiB를 checked 연산으로 바꾸고 32 MiB 이내인지 확인한다. 실제 RSS 목표와는 구분한다.
- 시그니처/StartHeader와 CRC, PackInfo, UnpackInfo, SubStreamsInfo, FilesInfo 및 EOF의 비압축 NextHeader를 작성한다. LZMA2 coder ID는 0x21이며 실제 사전 설정과 맞는 속성을 쓴다. 각 데이터 스트림의 압축/원본 크기와 CRC32를 기록한다. CRC32는 이미 사용하는 zlib-rs를 재사용한다. 오프셋, 길이 합계와 헤더 길이는 u64 checked 연산으로 처리한다.
- 파일명은 유효한 UTF-8 계획에서 UTF-16LE로 변환한다. 빈 파일/폴더에는 데이터 스트림을 만들지 않고 EmptyStream과 EmptyFile 구분 및 디렉터리 속성을 기록한다. 원본 파일이 없고 빈 폴더만 있는 묶음도 유효하게 작성한다. 빈 원본 목록 자체는 InvalidRequest로 거부한다.
- [공식 7z 형식 안내](https://www.7-zip.org/7z.html)와 기존 seven_zip.rs 해독기가 처리하는 기본 구조만 사용한다. encoded header, external metadata, additional streams와 anti 항목은 생성하지 않는다. LZMA2 내부의 비압축 청크는 허용하지만 별도 Copy 압축 선택은 추가하지 않는다.
- finish와 헤더 패치의 모든 오류를 반환하고 성공 여부를 Drop에 의존하지 않는다. 완성한 비공개 파일을 기존 7z 구조 검사기로 열어 시작/다음 헤더 CRC, 스트림 범위와 계획의 이름/종류/길이를 대조한 뒤 게시한다. 이 검사는 전체 payload를 다시 해독하는 무결성 검증을 뜻하지 않는다. 전체 해독과 원본 해시 대조는 아래 수용 시험에서 수행한다.
- 지원 부분의 헤더를 직접 유지하는 비용은 남는다. 계약 동의 후 첫 구현에서 독립 도구의 기본/빈 항목 호환과 인코더 메모리/취소 경계를 먼저 검증한다. 통과하지 않으면 지원 범위를 몰래 바꾸지 않고 이 PR에서 후보 및 계약을 다시 협의한다.

## 4. 작업자와 앱 계약안

현재 기준은 과거 C# 문서가 아닌 [작업자 README](../experiments/rust-archive-worker/README.md), protocol.rs, wire.rs와 contract.json이다. 아래 확장 계약은 합의되었고 공용 타입과 바이트 인코딩은 반영했다. 실제 작성기와 앱 연동의 구현 상태는 위 상태 및 contract.json의 seven_zip_create.status로 구분한다.

### 요청과 바이트 순서

- Action과 ActionKind 끝에 CreateSevenZip=4를 추가한다. List=0, Extract=1, CreateZip=2, Preview=3과 기존 Format::SevenZip=4는 유지한다. 프레임 ZAW1의 version=1, 32바이트 헤더, 1 MiB 본문 상한과 종류 번호는 바꾸지 않는다.
- 새 action의 필드 순서는 sources:Vec<Source>, output_file:NativePath, method:SevenZipMethod, solid:bool, encryption:Encryption이다. Source는 기존 path:NativePath, archive_name:UTF-8 String 순서다. Vec/String의 u32 길이, 정수 little-endian과 NativePath 인코딩도 유지한다. sources와 NativePath 벡터의 기존 32,768개 상한, String의 32,768 UTF-8 바이트 상한과 전체 프레임 상한을 유지하며 자원 승인으로 전송 상한을 늘리지 않는다.
- SevenZipMethod는 별도 u8 enum으로 Lzma2=0만 정의한다. ZIP의 Method(Store=0, Deflate=1)는 변경하지 않는다. solid의 지원 값은 false이고 encryption은 기존 None=0만 지원한다. 기존 Aes256=1과 solid=true를 구조상 유효한 요청으로 받더라도 Accepted 뒤 UnsupportedFeature로 끝내며, 원본 탐색, 자원 질문, 암호 질문과 출력 생성은 시작하지 않는다. 알 수 없는 enum/bool 값과 잔여 본문은 기존대로 Protocol이다.
- 일반 Start(kind 1)와 StartGuarded(kind 6, interactive=false)에 허용한다. StartInteractive와 interactive=true에는 허용하지 않는다. 앱은 기본 생성 요청부터 기존 큰 작업 승인 경로인 StartGuarded를 사용한다. 제한을 낮추는 기존 Start 호출은 자원 질문 없이 유지한다.
- output_file은 사용자가 고른 절대 파일 경로다. 형식은 action으로 결정하며 확장자로 ZIP 생성에 우회하지 않는다. 앱의 운영체제 기본 저장 창은 .7z 형식을 제안한다. 기존 목적지가 있으면 ZIP 생성과 같이 Conflict로 중단하며 이 단계에서는 덮어쓰기 질문/승인이나 자동 이름 변경을 추가하지 않는다.
- 구버전 앱/작업자는 새 action을 거부할 수 있다. 앱과 작업자를 같은 빌드로 배포하고 실패 시 ZIP/일반 Start/다른 작업자에 자동 재시도하지 않는다. 계약 동의 뒤 공용 타입, codec, contract.json과 앱 변환을 함께 갱신한다.

### 이벤트와 완료 판단

- Accepted.action과 ResourceRequired.action에 CreateSevenZip=4를 사용한다. 이 action의 Accepted.encryption은 false다. 기존 ZIP 및 다른 action의 암호 capability 의미는 유지한다. 새 메시지, 오류 코드와 별도 대형 결과 전송은 추가하지 않는다.
- 기존 ResourceRequired/Reply, Progress, Output, Cancel/CancelAck와 Terminal을 사용한다. 7z 생성에는 PasswordRequired와 ConflictRequired를 보내지 않는다. 질문 응답의 job_id/challenge_id/scope와 오래된 응답 거부 규칙을 유지한다.
- Scan은 계획 수집, Read는 스냅샷 입력, Write는 인코딩/스테이징/목적지 복사, Verify는 헤더/계획 검사, Commit은 게시 단계다. 원본 합계가 확정되면 읽기의 total_bytes에 사용한다. 압축 결과 길이나 검증 총량을 모르면 None으로 두고 진행률을 추측하지 않는다. 단계별 바이트를 원본 크기에 중복 합산하지 않는다.
- 정상 Output은 entry_id=None, relative_name=실제 목적지 기본 이름, disposition=Created 한 개다. 그 전에는 내부 파일마다 Output을 보내지 않는다. 정상 Terminal은 Completed, error=None, publication=Complete, created_count=1이며 정리 및 플랫폼 내구성은 실제 결과를 따른다.
- 게시 전 실패/취소에는 정상 Output이 없어야 하며 publication=None, created_count=0이다. 작업 소유 목적지 임시 파일의 정리가 실패하면 기존 PartialRetained 및 retained_partial_count/cleanup으로 실제 이름을 보고한다. 게시 후 동기화/정리 실패와 불명확한 게시에는 기존 Failed, Uncertain/CommitUncertain 및 실제 출력 집계를 유지한다. 성공이나 파일 부재를 합성하지 않는다.
- 앱은 일치하는 Terminal, stdout/stderr EOF와 소유 자식 회수를 모두 확인한 뒤 결과를 확정한다. CancelAck는 취소 완료가 아니며 게시 직전 cutoff 뒤의 TooLate와 실제 게시 결과를 우선한다. 작업자 사망이나 늦은 취소로 저장 완료를 취소 또는 자동 rollback으로 바꾸지 않는다.

## 5. 입력, 자원과 취소

- ZIP과 같은 원본 계획을 사용한다. 파일/폴더 모두 항목 수에 포함하며 파일 논리 바이트 합계에 min(limits.input,limits.output)을 적용한다. 기본은 16 GiB/10,000개, 유효한 자원 승인 뒤에는 요청한 ceiling 안에서 최대 64 GiB/200,000개다. 압축된 생성물 길이를 논리 원본 합계 대신 승인 기준으로 삼지 않는다.
- 기존 64개 경로 구성 요소, 승인 전 8 MiB/32,768 노드 및 승인 뒤 64 MiB/262,144 노드 메타데이터 예산을 유지한다. UTF-16 이름, 스트림 정보, CRC와 오프셋 배열 및 헤더 버퍼도 할당 전에 예산에 반영한다. 입력 전체나 파일 전체를 메모리 배열에 담지 않고 현재 파일만 64 KiB 이하로 복사한다.
- Plan의 경로 안전, 이름/접두 경로 및 대소문자 충돌, 원본 객체/종류/크기/수정 시각 대조를 재사용한다. 심볼릭 링크, reparse point, 특수 파일과 안전하지 않은 이름은 전체 작업을 거부한다. 탐색 뒤 교체와 감지된 변경은 SourceChanged다. 전체 폴더 트리의 원자적 스냅샷이나 같은 길이/시각으로 위장한 모든 변경 감지를 약속하지 않는다.
- 원본 사본, 압축 결과와 게시용 복사본의 동시 공간을 볼륨별로 계산한다. LZMA2 패킹의 보수적 크기 상한은 실제 작성기 분기에서 도출해 checked 연산으로 검사하고 각 실제 쓰기 전에도 여유 공간을 확인한다. 사전 확인은 공간 예약이 아니며 ENOSPC는 DiskFull, 그 밖의 저장 오류는 Io로 유지한다. 생성물 및 디스크 공간을 원본 바이트 예산과 혼동하지 않는다.
- 승인 전 전체 300초, 승인 뒤 실제 진행이 없는 60초와 질문 응답 대기 300초 정책을 유지한다. 성공한 읽기/압축 쓰기/헤더 처리/계획 검증과 정리만 실제 활동으로 기록한다. 단순 check나 UI 알림으로 시간을 연장하지 않는다.
- 입력 청크, 인코더 출력, 파일 경계, finish 전후, 헤더 seek/패치와 게시 직전에 취소 및 부모 생존을 확인한다. 압축 출력 Writer에도 감시기를 연결해 라이브러리 내부 쓰기 오류의 원인을 보존한다. 단일 스레드 인코더 한 호출 안의 CPU 처리와 OS 블로킹 I/O를 즉시 강제 중단한다고 보장하지 않는다. 부모/취소 확인 간격과 작은 파일 다수의 응답성도 시험한다.
- 1 MiB 사전의 대표 1 GiB 표본에서 기존 64 MiB 피크 RSS 목표를 측정한다. 큰 작업 허용은 인코더 사전/스레드를 늘리는 승인이 아니며 프로세스 전체 메모리의 강제 상한을 뜻하지 않는다.

## 6. 저장과 정리

1. 목적지 부모를 기존 Dir로 고정하고 경로/기존 목적지를 사전 검사한다. 원본과 같은 이름, 기존 파일/링크에는 쓰지 않는다.
2. 원본 계획과 소스 스냅샷을 만들고 비공개 spool 임시 파일에 완성된 7z를 작성한다. finish, 구조 검사와 Plan::verify가 실패하면 게시하지 않는다.
3. 완성 길이에 맞춰 목적지 공간을 확인하고 기존 publish_reader_steps의 새 파일 경로(replace=None)에 전달한다. 파일 sync와 close 오류를 확인한 뒤 배타적으로 게시한다. 사전검사 뒤 나타난 목적지도 Conflict로 거부한다.
4. 게시 직전에도 계획과 취소를 확인하며 확인과 게시 사이의 원자적 트랜잭션을 약속하지 않는다. 게시 후 부모 sync와 정리 결과를 실제 내구성/부분 출력으로 보고한다. 불명확한 결과를 덮어 재시도하지 않는다.
5. 비공개 입력/생성물은 핸들을 닫아 제거하고 목적지 임시 파일만 기존 소유 확인 정책으로 정리한다. 무관한 파일이나 이미 게시한 파일을 삭제하지 않는다. 강제 종료/전원 손실 뒤 자동 정리까지 보장하지 않는다.

## 7. 구현 후 수용 시험

아래는 아직 수행하지 않은 검증 계획이다. 시험 코드는 루트 tests/에만 두며 일회성 자료, 도구, 캐시와 로그는 tests/work/7zc 한 곳에 모은다. TMPDIR과 CARGO_TARGET_DIR을 그 하위에 명시하고 사본 안에 작업 폴더나 빌드 캐시를 다시 만들지 않는다. PR 병합/종료 뒤 담당자가 정리한다.

- 일반 파일, 압축되는/난수 내용, 한글 및 여러 단계 경로, 혼합 파일/폴더, 빈 파일, 빈 폴더, 빈 폴더만 있는 묶음과 여러 최상위 원본을 만든다. Zipelo 목록/해제와 독립 7-Zip의 테스트/목록/해제를 각각 실행해 상대 경로, 항목 종류, 파일 크기와 SHA256을 원본 manifest와 대조한다. 압축 아카이브 바이트 자체의 일치는 요구하지 않는다.
- 독립 설계 리뷰의 후속 기준으로 MainStreamsInfo/kEnd 계층, 32바이트 기준 NextHeaderOffset, 가변 UINT64와 고정 REAL_UINT64 구분을 확인한다. EmptyFile 비트 수는 전체 파일 수가 아닌 empty stream 수이며 혼합 빈 항목 7/8/9개와 정수 경계 127/128, 16383/16384를 포함한다.
- 독립 도구의 상세 목록으로 LZMA2, non-solid와 비암호화를 확인한다. 헤더/스트림 CRC와 작은 경계, 64 KiB 경계, 4 GiB 크기/오프셋 인코딩 및 비압축 헤더 상한을 시험한다. 큰 수치는 축소 상한/메타데이터 표본 검증과 실제 대용량 실행을 구분한다.
- 자원 기본/승인 경계, 거부와 취소, 파일/폴더 수 합산, 메타데이터 및 산술 오버플로, 위험 경로, 링크/특수 파일, 원본/부모 교체, 목적지 새 파일 출현과 기존 파일 보존을 검증한다. 파일 크기와 무관하게 큰 배열이 생기지 않는지 확인한다.
- 캡처/압축/finish/헤더 패치/게시 직전 취소, 부모 종료, 임시/목적지 디스크 부족, write/seek/sync/close/게시/정리 실패를 주입한다. 게시 전에는 완성 파일이 없어야 하며 실패/취소를 Completed로 표시하지 않아야 한다. 게시 뒤 실패와 강제 종료에서는 실제 게시 및 정리 상태를 대조한다.
- codec 양방향 및 잘린 본문/알 수 없는 tag/잔여 필드, solid/암호화 미지원, 암호/충돌 질문 부재, 구버전 거부의 무자동재시도, job/challenge/sequence/종료 중복과 EOF/자식 회수 오류를 시험한다. 기존 ZIP의 Store/Deflate/AES 생성과 해제는 회귀를 유지한다.
- 최종 구현에서 python3 tests/ci/run_rust.py local 결과와 라이선스 고지 시험, 메모리/취소 근거를 PR 본문에 적는다. Linux 시험과 Windows 교차 빌드를 Windows 실제 실행과 구분한다. Windows와 Wayland의 기본 파일/폴더 선택, 형식 선택과 저장, 미지원 암호 안내, 큰 작업 허용, 취소와 실패 화면은 별도로 확인한다.

## 8. 합의와 담당 인계

- 하나는 이 계약과 backend 작성기, 공용 타입/codec 및 작업자 회귀를 담당한다. 히나타의 계약 동의와 카논의 설계 리뷰가 확인되어 공용 타입 인계 단계에 착수했다. 실제 작성기는 후속 구현이다.
- 계약 동의 뒤 공용 타입 반영 커밋을 히나타에게 전달한다. 히나타는 기존 생성 화면의 ZIP/7z 형식 선택, .7z 기본 저장 형식, 암호/방식 선택 제한, 새 ActionKind 변환과 수신 검증을 담당한다. 화면이 바뀌는 구현 PR에 --ui-scenario 목록/시험과 docs/ui-scenario-guide.md를 함께 갱신한다. 별도 디자인 변경이나 애니메이션을 추가하지 않는다.
- 카논의 설계와 실패 경계 리뷰는 완료되었으며 구현 후 독립 7-Zip 호환 검증 및 재리뷰는 별도 요청한다. 도구 버전/옵션, 원본과 해제 manifest 및 플랫폼별 실행 범위를 남긴다. 도구를 제품에 번들하지 않는다.
- 설계 문서의 정적 검토와 링크/공백 검사는 제품 빌드나 호환 검증이 아니다. 이번 PR은 UI 변경 없음이며 구현 시작 조건과 아직 수행하지 않은 시험을 명시한 초안으로 유지한다.

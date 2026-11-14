# ZIP 손상 복구 설계와 작업자 계약안

상태: 히나타가 결과/경고 의미와 오류 태그 15/16에 재동의했다. 공용 타입, codec과 ZIP32 복구 작업자를 구현했으며 frontend 연동과 독립 구현 리뷰는 남아 있다. [PLANS 8.10](PLANS.md#810-손상-파일-복구)과 [8.13](PLANS.md#813-진행과-완료-판단)을 따른다. [작업자 계약](../experiments/rust-archive-worker/contract.json)의 zip_recovery_proposal 키는 초기 문서와의 연결을 위해 유지하지만 status에 명시한 태그는 활성 상태다. Frame version 1, revision 3와 기존 태그 및 일반 목록/해제의 엄격한 검사는 유지한다.

결과 계약 [동의](https://github.com/Team-LJY/zipelo/pull/128#pullrequestreview-5904112564)와 오류 숫자 [재동의](https://github.com/Team-LJY/zipelo/pull/128#pullrequestreview-5904158997)를 확인했다. 초기 14/15 제안이 기존 MissingVolume=14와 충돌한 기록은 보존하며 실제 활성 값은 RecoveryIncomplete=15, RecoveryImpossible=16이다.

## 1. 복구 범위와 한계

- 첫 구현 입력은 단일 볼륨 ZIP32의 비암호화 Store/Deflate 일반 파일과 빈 파일/폴더다. ZIP64 입력, 분할 ZIP, SFX, 암호화된 중앙 목록과 그 밖의 압축 방식은 첫 복구 범위에서 제외한다. 정상 ZIP64/암호 ZIP의 기존 목록/해제 지원을 축소하지 않는다.
- 복구 대상은 중앙 디렉터리/EOCD 손상 또는 누락, 온전한 중앙 목록으로 범위를 정할 수 있는 로컬 서명/CRC/크기 필드 손상이다. 본문 손상과 중간 절단에서는 검증 가능한 다른 항목만 구한다. 이름, 경계나 원본 내용 자체를 추측해 되살리지 않는다.
- 정상 중앙 목록 안의 암호/미지원 항목은 내용을 내보내지 않고 Unverified로 보고한다. 아카이브 전체의 ZIP64, 분할, SFX 또는 중앙 암호화가 명확하면 UnsupportedFeature로 중단하며 더 약한 로컬 탐색으로 우회하지 않는다.
- 원본 ZIP은 읽기 전용이다. 성공 여부와 무관하게 원본과 같은 파일, 하드 링크, 기존 목적지에 쓰거나 원본을 덮어쓰지 않는다. 복구본은 새 파일 하나이며 불완전한 내용의 바이트 조각이나 임의 이름의 파일을 추가하지 않는다.
- 파일 내용과 안전한 상대 이름/종류가 보존 대상이다. 서명, ACL, 실행 권한, 소유자, 주석, 알 수 없는 extra field와 모든 시각을 복원한다고 약속하지 않는다. CRC 검증은 원래 작성자의 진위나 악의적인 위조 부재를 뜻하지 않는다.

## 2. 경계와 검증 근거

형식 판단은 [PKWARE APPNOTE 6.3.10](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT)의 4.3.7, 4.3.9, 4.3.12와 4.3.16을 참고한다. 로컬 헤더, 파일 데이터와 선택적 data descriptor 및 중앙 목록을 구분하는 명세이며 손상 복구 알고리즘이나 복구 보증을 제공하지 않는다. 아래의 신뢰 기준은 Zipelo의 보수적인 설계 선택이다.

### CentralDirectory 모드

- EOCD, 중앙 목록 범위/항목 수, 이름/extra field, 디스크 번호와 모든 참조 범위를 독립적으로 검증한다. 참조된 payload가 중앙 목록을 침범하거나 항목 범위가 겹치고 중복되면 정상 목록으로 신뢰하지 않는다. 모든 로컬 항목 범위가 오프셋 0부터 중앙 목록 시작까지 빈틈없이 이어지고 중앙 항목과 1:1로 대응할 때만 전체 목록의 완전성을 확인한다. 공백/불명 범위가 있으면 중앙 항목의 데이터가 맞아도 unknown_loss=true, known_entries=None이며 완전 복구가 아니다. 손상된 목록의 일부 항목 수를 전체 항목 수로 사용하지 않는다.
- 로컬 헤더의 이름/extra 길이, raw 이름, flags와 방식이 온전하고 중앙 목록과 맞아 payload 시작이 하나로 정해져야 한다. 이때 로컬 서명, CRC 또는 크기 필드만 중앙 근거로 재구성할 수 있다. 시작 위치/이름/방식이 모호하면 그 항목은 Unverified이고 payload를 복사하지 않는다.
- bit 3 항목은 중앙 목록으로 정한 정확한 압축 범위와 지원하는 descriptor의 CRC/크기가 모두 맞을 때만 검증한다. 서명 유무를 구분하며 descriptor 서명을 검색해 데이터 끝을 추측하지 않는다. ZIP64 extra/크기 sentinel이 있으면 첫 입력 범위 밖으로 처리한다.
- CRC나 디코딩에 실패하더라도 온전한 중앙 목록이 정한 비중첩 경계로 다음 항목을 검사한다. 범위 자체의 모순은 로컬 목록으로 안전하게 다시 시작할 수 있는지 별도로 판단하며, unsafe path나 자원/저장 오류를 손상 복구로 숨기지 않는다.

### LocalChain 모드

- 사용할 수 있는 중앙 목록이 없으면 오프셋 0부터 연속한 로컬 헤더만 걷는다. bit 3이 꺼져 있고 명시된 ZIP32 크기와 온전한 이름/길이 필드로 다음 위치가 결정되는 항목만 검사한다.
- 본문 CRC가 틀려도 완전한 범위가 알려진 항목은 Lost로 기록하고 그 끝에서 다음 헤더를 검사한다. 범위가 EOF를 넘는 절단, 해석 불가 헤더 또는 경계를 알 수 없는 descriptor를 만나면 탐색을 멈춘다. 구조로 확인한 후보의 절단/불명 경계만 Lost/Unverified로 기록하며, 정체를 알 수 없는 꼬리 바이트에 가짜 항목/이름/개수를 만들지 않고 unknown_loss로 남긴다. 다음 정상 멤버를 찾으려고 본문 안의 PK 바이트를 검색하지 않는다.
- 중앙 목록 위치나 EOF까지 도달해 모든 관측 항목이 검증되어도 원래 전체 목록이 맞다는 증거는 없다. known_entries=None, unknown_loss=true로 유지하며 유실 0개 또는 완전 복구로 표시하지 않는다. 누락된 파일명이나 개수를 만들어내지 않는다.
- 중앙 속성이 없는 경우 안전한 이름의 일반 파일/빈 폴더만 새 일반 속성으로 작성한다. 링크/권한을 추측해 복원하지 않으며 누락 메타데이터를 경고에 남긴다.

### 내보낼 항목의 공통 조건

- 온전한 헤더에서 얻은 압축 범위/예상 크기/CRC와 실제 결과가 모두 맞아야 한다. Deflate는 실제 종료와 정확한 입력 소비를 요구하며 초과/짧은 출력, 남은 압축 바이트와 CRC 불일치는 Lost다. Store도 크기와 CRC를 검사한다. 새로 계산한 CRC로 깨진 원본 CRC를 덮어 정당화하지 않는다.
- 검증된 파일의 SHA256을 계산해 결과에 싣고 손상 표본의 외부 기대 해시와 대조한다. 이 해시 자체는 원본의 손상 전 내용을 증명하지 않는다. 검증에 실패한 부분 내용은 복구본에 넣지 않는다.
- 절대/상위/장치/예약/대체 스트림 이름, 이름/접두 경로의 대소문자 충돌, 확인된 링크와 특수 파일은 기존 전체 작업의 UnsafePath/Conflict 정책으로 거부한다. 겹친 범위나 추측한 이름에 자동 이름 변경/평탄화 예외를 두지 않는다.

## 3. 원본 보존, 출력과 자원

1. 원본 일반 파일을 안전한 읽기 핸들로 고정하고 기존 spool::Snapshot에 전체 SHA256과 함께 캡처한다. 경로, 객체, 길이와 수정 메타데이터 변화는 SourceChanged로 거부한다. 모든 파싱/디코딩은 이 비공개 스냅샷에서만 수행한다.
2. 검증된 데이터는 비공개 파일 spool에만 두고 새 비암호화 Store ZIP을 작성한다. 기존 ZIP/ZIP64 작성기의 컨테이너 경계를 재사용하되 사용자 경로의 임시 원본 파일로 변환하지 않는다. Store 복구본은 원본보다 커질 수 있다. 결과 크기가 요구하면 기존 ZIP64 출력 작성을 쓰며 ZIP64 입력 복구 지원을 뜻하지 않는다.
3. 새 헤더는 안전한 이름, 실제 검증된 크기/CRC와 일반 파일/폴더 속성만 기록한다. 원본의 주석/미확인 extra/권한/서명은 복사하지 않는다. 완성된 비공개 ZIP을 엄격한 일반 판독기로 다시 열어 항목과 SHA256을 검사한다.
4. 실제 게시 직전에 원본 객체/길이/메타데이터와 전체 해시를 한 번 재확인한다. 동시 변경을 발견하면 SourceChanged로 중단한다. 이는 파일시스템 전체의 원자적 잠금이나 변경 후 원상복원까지 감지하는 보장이 아니다.
5. 목적지 부모는 기존 Dir로 고정하고 새 파일의 배타적 게시(replace=None)를 쓴다. 캡처/검증 중 새로 나타난 목적지도 Conflict이며 덮어쓰기 질문이나 자동 대체 이름을 추가하지 않는다. fsync/close/게시/부모 sync와 정리 결과를 실제 상태로 보고한다.
6. 취소와 실패는 소유 비공개 spool 및 게시 임시 파일만 정리한다. 게시 뒤 실패/프로세스 손실에는 실제 출력/내구성/불명확 상태를 보존한다. 이미 저장한 부분 복구본을 성공처럼 표시하거나 자동 삭제/rollback하지 않는다.

- 복구 입력은 min(호출자 input 상한, 4 GiB)로 제한한다. 4 GiB 초과 원본은 최초 복구 지원 범위 밖이므로 UnsupportedFeature이고 승인으로 늘리거나 큰 작업 재시도를 제안하지 않는다. 호출자가 더 낮춘 input 상한 초과는 기존 Limit이다. 기존 다른 작업의 입력 한도를 바꾸지 않는다. decoded bytes/후보 수는 기본 16 GiB/10000, 명시적 승인 뒤 요청 ceiling 안에서 최대 64 GiB/200000이다. 손실/미확인 후보와 실패 전 실제 디코딩 바이트도 예산에 포함한다. 같은 검증 데이터를 ZIP 작성/게시로 복사하는 바이트는 논리 해제량으로 중복 집계하지 않는다.
- 이름, 후보/범위/결과 레코드, SHA 배열, spool 인덱스와 암묵 부모를 할당 전에 기존 메타데이터 예산에 넣는다. 승인 전 8 MiB/32768 노드, 유효한 승인 뒤 64 MiB/262144 노드이며 한 이름 4096바이트, 구성 요소 255바이트, 깊이 64의 기존 경로 제한과 프레임/문자열 제한을 유지한다.
- 복사/CRC/디코딩은 64 KiB 청크와 checked u64 오프셋을 쓴다. 동일 범위를 여러 후보 방식으로 재시도하지 않는다. 기존 압축률 질문, 부모 생존, 승인 전 전체 300초/승인 뒤 실제 진행 없는 60초 및 질문 대기 300초 정책을 유지한다. 반복 check나 하트비트를 진짜 진행으로 세지 않는다.
- 원본 스냅샷, 검증된 멤버 spool, 완성 ZIP과 목적지 임시 파일의 동시 공간을 볼륨별로 검사하고 각 실제 쓰기에서도 확인한다. 권고 여유 공간은 예약이 아니며 ENOSPC는 DiskFull, 다른 저장 실패는 Io다. Limit/UnsafePath/SourceChanged/저장 실패를 Lost로 낮춰 계속 게시하지 않는다.
- 새 의존성은 제안하지 않는다. 이미 쓰는 zlib-rs의 CRC/Deflate, SHA256, 안전한 spool과 ZIP 작성기를 재사용한다. 이번 문서는 의존성 보안 감사나 성능/RSS 검증 결과가 아니다.

## 4. 앱과 작업자 계약 제안

이 절의 숫자와 결과 의미는 공동 합의되었고 공용 타입과 codec에 반영했다. Frame version=1, 32바이트 헤더, 1 MiB body, 방향별 sequence, 32768 문자열/벡터 전송 상한과 기존 revision 3를 유지한다. 기존 Preview kind 111~113과 VolumeProblem kind 114를 재사용하지 않는다. 실제 wire의 MissingVolume=14도 유지한다. 기존 contract.json의 error_tags 색인은 이 오류를 누락하므로 새 태그 검사는 문서 색인뿐 아니라 활성 wire와 대조한다. 초기 제안의 복구 오류 14/15는 충돌하여 아래 15/16으로 정정하고 재합의했다. 결과 의미와 다른 숫자는 동일하다.

### 요청과 이벤트

- Action::RecoverZip 및 ActionKind::RecoverZip=5를 사용한다. 요청 필드 순서는 archive:NativePath, output_file:NativePath다. Start(kind 1)와 StartGuarded(kind 6, interactive=false)에 허용하며 StartInteractive 및 interactive=true는 양쪽 codec에서 거부한다. Accepted.encryption=false이고 PasswordRequired/ConflictRequired를 보내지 않는다. 기존 ResourceRequired.action에 같은 새 태그를 사용한다.
- RecoveryEntry(kind 115): candidate_id:u32, source_offset:u64, name:Option<UTF8>, kind:Option<EntryKind>, status:RecoveryStatus, reason:RecoveryReason, declared_size:Option<u64>, verified_size:Option<u64>, sha256:Option<[u8;32]> 순서다. 중앙 목록의 후보도 로컬 오프셋 순으로 정렬한 0 기반 ID를 한 번씩 보낸다. 일반 Entry/ListEnd/ListId나 preview 프레임은 보내지 않는다.
- RecoveryMode는 CentralDirectory=0, LocalChain=1이다. RecoveryStatus는 Verified=0, Lost=1, Unverified=2다. RecoveryReason은 None=0, HeaderRebuilt=1, Truncated=2, CrcMismatch=3, DecodeFailure=4, BoundaryUnknown=5, UnsupportedFeature=6, MetadataMissing=7이다. 유효 reason은 Verified에서 None/HeaderRebuilt/MetadataMissing, Lost에서 Truncated/CrcMismatch/DecodeFailure, Unverified에서 BoundaryUnknown/UnsupportedFeature/MetadataMissing다. 서로 모순되는 조합은 Protocol이다.
- Verified에는 확정된 이름이 필요하다. Verified 파일은 declared_size와 verified_size가 같고 SHA256을 가지며 Verified 폴더는 두 크기 0, 해시 None이다. Lost/Unverified에는 verified_size나 SHA256을 싣지 않는다. 불명 이름은 None이며 불명 크기를 0으로 만들지 않는다. kind는 Some(File)/Some(Directory)만 허용하고 판별 불가 후보는 None이다. Verified에는 확정된 kind가 필요하며 링크/특수 종류를 일반 파일로 위장하지 않는다. 이름은 평문 UI에서만 제한 길이로 렌더링하고 경로나 markup으로 실행하지 않는다.
- RecoveryEnd(kind 116): mode, source_bytes:u64, source_sha256:[u8;32], known_entries:Option<u32>, verified_entries:u32, lost_entries:u32, unverified_entries:u32, unknown_loss:bool 순서다. 완료된 분석에 한 번만 보내며 실제 게시 전에 보낸다. 집계는 전송한 레코드와 일치해야 하고 중앙 모드에서 연속 범위와 1:1 대응까지 확인한 경우만 known_entries=집계 합, unknown_loss=false다. 중앙 근거가 있어도 불명 공백/범위가 있으면 known_entries=None, unknown_loss=true다. 로컬 모드는 항상 known_entries=None, unknown_loss=true다. 요약 수신은 저장 성공이 아니다.
- candidate_id는 이 복구 작업의 결과 ID이며 원본 List EntryIds로 재사용하지 않는다. 복구본을 열려면 명시적 새 List 작업과 새 ListId가 필요하다. 알 수 없는 태그/필드/추가 본문과 중복/오래된 메시지는 Protocol로 거부한다. 앱/작업자는 같은 빌드로 배포하고 다른 형식, 일반 Start 또는 구버전 작업자로 자동 우회하지 않는다.
- Progress는 기존 Scan/Read/Decode/Write/Verify/Commit을 사용한다. 알 수 없는 총량은 None이며 후보 조사/검증과 실제 파일 저장을 구분한다. 개별 Verified 레코드는 내부 검증 결과일 뿐 저장한 파일을 뜻하지 않는다.

### 최종 결과와 게시

- 복구본 하나의 실제 Output Created는 entry_id=None, relative_name=목적지 기본 이름이다. 멤버별 Output은 보내지 않는다. 원본 전체 목록을 알고 모든 알려진 항목을 검증했으며 유실/미확인/unknown_loss가 없고 게시/마무리가 성공한 경우에만 Completed/None, publication=Complete, created_count=1이다.
- 검증된 항목이 하나 이상이지만 유실, 미확인 또는 불명 전체 목록이 있으면 복구본을 저장한 뒤 Failed/RecoveryIncomplete(새 ErrorCode=15), publication=Complete, created_count=1로 끝낸다. 여기서 Complete는 ZIP 파일 하나의 게시 완료이지 원본 전체의 완전 복구가 아니다. UI는 실패 일반 문구로 복구본을 숨기거나 완전 복구 성공으로 바꾸지 않는다.
- 원본이 엄격한 일반 판독/내용 검증까지 통과하고 재구성할 헤더 손상이 없으면 복구 대상이 아니다. RecoverZip을 직접 요청했더라도 InvalidRequest와 출력 0개로 끝내며 정상 control.zip을 자동 재작성하지 않는다.
- 검증된 항목이 하나도 없으면 Failed/RecoveryImpossible(새 ErrorCode=16), publication=None, created_count=0으로 끝내고 빈 복구본을 만들지 않는다. 저장/경로/자원/변경/프로토콜 오류와 취소가 생기면 그 실제 오류가 부분 복구 업무 결과보다 우선한다.
- 게시 전 실패/취소에는 완성 파일이 없으며 소유 임시 파일 정리 실패만 기존 PartialRetained/cleanup/retained 집계로 보고한다. 게시 후 오류에는 기존 Failed와 실제 Output, durability/cleanup, CommitUncertain/Uncertain을 유지한다. Terminal은 마지막 프레임 하나이고 CancelAck는 완료 증거가 아니다.
- 앱은 일치하는 Terminal, stdout/stderr EOF와 소유 자식 회수가 모두 끝난 뒤 결과를 확정한다. 부분 복구본이 있거나 게시/정리가 불명확하면 대형 한도 자동 재시도/닫기 후 자동 재실행을 하지 않는다. 취소 cutoff 뒤 TooLate와 실제 게시 결과도 기존대로 존중한다.

## 5. 화면 인계 기준

- 일반 목록/해제의 Integrity에 복구본 만들기를 명시적으로 제안할 수 있으나 조용히 느슨한 파서를 적용하거나 자동 저장하지 않는다. 사용자가 별도 복구본 생성을 선택하면 운영체제 기본 저장 창에서 새 ZIP 경로를 고른다. 원본을 바꾸지 않으며 일부 자료는 복구할 수 없다는 안내를 먼저 제공한다.
- 결과는 검증되어 포함됨, 유실됨, 무결성 미확인을 각각 표시하고 사유를 구분한다. 모르는 파일명/원래 총수는 알 수 없음으로 표시하며 유실 0개나 진행률 100%를 추측하지 않는다. 완전한 목록이 있을 때도 파일별 CRC 검증을 원본 진위 보증으로 표현하지 않는다.
- 정상 완료는 복구본 저장 완료, 부분 결과는 일부 항목만 복구함과 전체 목록/무결성 미확인 경고 및 실제 저장 위치를 보여준다. 복구 불가는 복구 가능한 항목 없음으로 구분한다. 세부 결과를 일반 파일 목록 선택과 섞지 않는다.
- 복구본은 일반 ZIP이며 별도 sidecar 보고서나 영구 인증서를 자동 생성하지 않는다. 이번 작업의 경고를 결과 화면과 명시적 복구본 열기 전환에 유지한다. 나중의 독립적인 일반 ZIP 목록은 원본에서 무엇이 유실됐는지 증명하지 않으며 완전 복구 판정으로 다시 사용하지 않는다.
- frontend 구현 시 --ui-scenario에 복구 진행, 완전/부분/불가 결과, 미확인 목록, 취소, 저장 실패와 자원 질문을 추가하고 해당 시험 및 docs/ui-scenario-guide.md를 같은 PR에서 갱신한다. 현재 backend 변경에는 UI 변경 없음이다. frontend의 화면/시나리오 연동은 같은 PR의 후속 작업이다.

## 6. 손상 표본과 수용 기준

카논이 같은 features/zip-recovery 브랜치의 [손상 표본과 기대 manifest](../tests/zip-recovery-fixtures/README.md)를 담당한다. 3844170의 README와 expectations.json을 읽어 설계 분류를 대조했다. missing-directory.zip과 broken-eocd.zip은 각 6개, middle-truncated.zip은 앞 4개, body-corrupt.zip은 CRC 실패 항목을 제외한 5개, unrecoverable.zip은 0개가 기대값이다. 이 손상 표본의 유효 항목은 LocalChain 조건으로 처리하므로 전체 목록 불명 경고를 유지한다. control.zip은 정상 열기 대조군이며 복구본을 게시하지 않는다. 이는 기대값의 정적 대조이며 제품 실행이나 표본 시험 통과를 대신하지 않는다. backend는 담당 표본을 동시에 작성하거나 수정하지 않는다. d525c7c의 broken-local-header.zip은 중앙 목록/EOCD와 본문을 보존하고 로컬 서명만 손상했다. 6개 검증 항목, CentralDirectory, known_entries=6과 unknown_loss=false를 기대하는 완전 복구 판정 표본이며 기대값을 읽어 대조했다. 카논의 표본 검증과 실제 작업자 복구 실행은 별개다.

- 헤더 손상: 중앙 목록/EOCD 손상은 LocalChain으로 검증된 파일이 기대 SHA256/구조와 맞아도 unknown_loss 경고를 유지한다. 온전한 중앙 목록과 로컬 CRC/서명/크기 손상은 모든 항목 검증 시 완전한 복구본 저장 판정이 가능하다.
- 중간 절단: 절단 전 온전한 항목만 포함하고 잘린 항목/이후 불명 부분을 정상으로 세지 않는다. 본문 손상: 온전한 중앙 목록의 다른 항목만 포함하며 CRC 불일치를 Lost로 보고한다. 복구 불가: 검증 항목 0개, 완성 출력 0개다.
- 모든 표본에서 원본의 작업 전/후 SHA256 및 실제 원본 바이트가 같아야 한다. 복구 가능한 파일은 손상 전 독립 기대 manifest와 비교하고 Zipelo 및 독립 도구로 복구본을 풀어 파일 해시/상대 구조를 대조한다. 새로 계산한 결과 해시만으로 원본과 같다고 판단하지 않는다.
- PK 바이트가 든 본문, 위조 EOCD/중앙 목록, 겹친 범위/잘못된 길이, UTF-8/한글/빈 항목, data descriptor 경계, ZIP64/암호/분할/SFX 제외, unsafe path/충돌/링크와 프레임/후보 수/메타데이터 제한을 포함한다. 이름 추측이나 본문 내 서명 재탐색 없이 실패/부분 결과가 결정돼야 한다.
- 취소/부모 종료/원본 변경, 기존 및 검증 중 나타난 목적지, 쓰기/close/sync/부모 sync/정리 오류를 주입하고 실제 파일/Terminal/Output/내구성과 맞는지 확인한다. 손상과 무관한 Limit/DiskFull 등을 Lost로 가려 게시해서는 안 된다.
- 계약 동의 뒤 backend 회귀와 python3 tests/ci/run_rust.py local 전체 결과, frontend 화면/시나리오 시험, Windows/Wayland 실기 및 독립 구현 리뷰를 PR 본문에 기록한다. 현재 직접 검증한 backend 회귀에는 6종 실제 작업자 복구와 독립 Python ZIP 검증, 원본 보존 및 실패 주입이 포함된다. 전체 local 결과와 미실행 Windows/Wayland 실기는 PR 본문에서 구분한다. 성능/RSS의 별도 복구 전용 대용량 실기는 아직 수행하지 않았다.

## 7. 구현 순서와 문서 관리

1. 결과 계약과 오류 숫자의 재합의를 확인하여 제품 타입/codec/탐색기/작성기를 구현했다. 카논의 표본 대조 리뷰와 독립 기대 해시를 backend 수용 시험에 반영했다.
2. 공용 타입/codec과 backend 커밋 및 작업 경로를 전달하며 frontend 결과 화면과 해당 시나리오/시험은 히나타가 담당한다. 프레임 숫자와 결과 조합이 바뀌면 다시 함께 합의한다.
3. 표본 기대 결과를 변경할 필요가 생기면 작성자와 근거를 먼저 합의한다. 실패를 숨기려고 표본이나 기존 엄격한 파서를 약화하지 않는다.
4. 구현/실기 증거가 모인 뒤에만 README/릴리스 노트의 실제 지원 범위를 갱신하고 총관리자의 리뷰/병합 절차를 따른다. 원본 수정과 모든 손상 복구 보장은 완료 기준에 포함하지 않는다.

작업 사본은 tests/work/zip-recovery/r이다. 캐시/임시/검증 자료는 사본 밖 tests/work/zip-recovery 한 곳에만 두고, 이후 실행 시 TMPDIR과 CARGO_TARGET_DIR을 그 하위로 지정한다. PR 병합/종료 뒤 만든 사람이 작업 폴더를 정리하며 진행 중인 다른 담당자 자료는 지우지 않는다.

## 8. 구현과 검증 위치

- backend 탐색/검증/원본 재확인은 zip_recovery.rs, Store 출력은 create.rs의 ZIP64 필드/중앙 목록 마무리를 재사용한다. 검증된 payload는 하나의 비공개 spool로 모으고 손상 항목의 임시 바이트는 되돌린다. CRC 또는 디코더 실패 전에 처리한 바이트도 자원 사용량에 남긴다.
- 결과 파일을 일반 streaming 판독기로 다시 검증하고 항목 SHA256을 대조한다. 이 재검증/복사는 논리 해제량을 중복 과금하지 않으며 취소와 실제 진행 검사는 유지한다. 원본 변경 검사는 마지막 게시 callback 안에서 수행한다.
- 제품 회귀는 tests/rust-archive-worker/recovery.rs와 recovery_checks.rs에 있다. tests/ci/test_zip_recovery_contract.py의 텍스트 읽기는 UTF-8을 명시하며 기존 오류 및 새 태그를 실제 wire와 비교한다. 기존 시험의 미등록 태그 경계는 새 할당 뒤 값으로 조정했다. 긴 소켓 시험은 열린 출력 폴더의 proc fd 경로로 같은 객체를 만들어 SUN_LEN에 의존하지 않는다.
- frontend의 메시지 처리, 결과 화면, --ui-scenario/가이드와 Windows/Wayland 실제 실행은 별도 담당 검증이며 backend 시험 통과로 완료를 대신 선언하지 않는다.

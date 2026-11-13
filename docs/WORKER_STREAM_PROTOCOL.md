# 작업자 스트림 프로토콜 v2 기반

> 과거 C# 구현 기록: C#은 v0.1에서 중단하고 Rust로 대체했다. PR103(f1d9fa5)에서 C# 소스와 전용 시험 및 .NET 설정을 제거했다. 아래 본문의 "현재", 미구현 상태, API와 검증 결과는 작성 당시 C# 스냅샷에 관한 내용이며, 현재 Rust 제품의 지원 범위나 검증 상태를 뜻하지 않는다. 삭제된 경로와 실행 명령은 현재 main에서 사용할 수 없다.
>
> 현재 기준은 [기획서](PLANS.md), [UI 설계](ui-design-v0.1.md), [릴리스 노트](release-notes-v0.1.md)다. 앱은 [Rust UI](../experiments/rust-ui-memory-app/), 압축 처리는 [Rust 작업자](../experiments/rust-archive-worker/README.md)를 사용한다. 과거 검사 재현과 소스 보존 안내는 [기획서의 과거 기록](PLANS.md#7-c-중단과-과거-검증-기록)을 따른다.

## 보존된 당시 기록

## 범위와 상태

기준 커밋은 260428ec8836bf122589d995c7526468a09e190d임.
새 계층은 Zipelo.Runtime.Streaming 네임스페이스에 독립적으로 추가됨.
기존 WorkerProtocol v1, ZipWorkerClient, Worker Program, capability, 16 MiB 상한,
게시 API에는 연결하지 않음. 자동 버전 협상과 v1에서 v2로의 전환도 없음.
새 의존성, 디스크 저장, 압축 생성과 해제 구현을 포함하지 않음.

이 계약은 단일 방향, 단일 작업, 한 번에 한 항목의 바이트 전송만 다룸.
양방향 요청/응답 결합, 파일 이름과 메타데이터, 진행률, 입력 범위 읽기는 후속 계약임.
전송 완료는 ZIP 무결성 검사나 디스크 게시 성공을 의미하지 않음.
길이 일치는 내용의 CRC/SHA 검증 또는 발신자 인증이 아님.

## 공개 API

- WorkerStreamChannel(Stream, WorkerStreamDirection, Guid, WorkerStreamLimits?):
  호출자가 지정한 비어 있지 않은 작업 ID와 Read 또는 Write 방향으로 생성함.
- ReadAsync(CancellationToken): 검증된 WorkerStreamFrame을 반환함.
  EndJob도 먼저 프레임으로 반환하며, 이후 호출에서 EOF를 확인해야 null을 반환함.
  Phase가 InputCompleted이고 TerminalStatus가 Completed일 때만 전송 성공임.
  Failed와 Cancelled도 EOF를 요구하며 성공으로 취급하지 않음.
- WriteAsync(WorkerStreamHeader, ReadOnlyMemory<byte>, CancellationToken):
  헤더와 페이로드를 검증하고 제한된 복사본을 기록한 뒤 FlushAsync까지 기다림.
  완료된 Task 이후의 Terminal 상태는 송신 완료만 의미하며 수신 확인이 아님.
  EndJob 뒤 호출자가 송신 스트림을 닫아 EOF를 전달해야 함.
- Phase, TerminalStatus, TotalDataBytes, TotalWireBytes, FramesAccepted:
  완료된 호출 사이에 조회할 진단 상태임. 동시 상태 관찰로 완료를 판단하지 않음.
- WorkerStreamValidator:
  ValidateHeader, Accept, CompleteInput, Abort와 상태/카운터를 제공함.
  동시 호출을 지원하지 않는 순수 전이 계층이며 실제 I/O는 수행하지 않음.
  ValidateHeader는 예산을 소비하지 않으며 Accept가 한 번 소비함.
  CompleteInput은 실제 EOF를 확인한 호출자만 호출해야 함.
- WorkerStreamException.Error:
  InvalidHeader, InvalidLength, UnexpectedFrame, IdMismatch, SequenceMismatch,
  BudgetExceeded, Truncated, TrailingData, Faulted를 구분함.
  원래 I/O 예외는 그대로 전달하며 호출자 취소는 OperationCanceledException임.

채널의 ReadAsync/WriteAsync/Dispose 동시 호출은 InvalidOperationException으로 거절함.
스트림의 다른 읽기/쓰기 주체를 두면 안 됨. API 사용 오류인 잘못된 방향, 중첩 호출은
진행 중인 정상 호출의 상태를 변경하지 않음. Dispose는 진행 중인 호출의 취소와 완료 대기 뒤 수행함.
Dispose는 원본 스트림을 닫지 않으며 호출자가 스트림과 프로세스 수명을 관리함.

## 고정 헤더

모든 프레임은 64바이트 헤더와 정확히 payloadLength 바이트의 페이로드로 구성됨.
정수는 unsigned little-endian임. 예약 필드는 반드시 0임.

- 오프셋 0, 4바이트: ASCII ZIP2, 정수 0x3250495A.
- 오프셋 4, 2바이트: 버전 2. 다른 버전은 거절함.
- 오프셋 6, 2바이트: 프레임 종류.
- 오프셋 8, 4바이트: payloadLength.
- 오프셋 12, 4바이트: 예약 0.
- 오프셋 16, 16바이트: 작업 Guid.
- 오프셋 32, 16바이트: 항목 Guid.
- 오프셋 48, 8바이트: 작업 전체 순번. BeginJob이 0이며 매 프레임 정확히 1 증가함.
- 오프셋 56, 8바이트: 예약 0.

Guid는 .NET Guid.TryWriteBytes(Span<byte>)와 Guid(ReadOnlySpan<byte>)의 바이트 순서를 사용함.
예를 들어 00112233-4455-6677-8899-aabbccddeeff는
33 22 11 00 55 44 77 66 88 99 aa bb cc dd ee ff로 전송함.
네트워크 순서 Guid와 혼용하지 않음. 빈 Guid는 16바이트 모두 0임.

## 프레임과 전이

- 1 BeginJob: 페이로드 0바이트, 빈 항목 ID. Ready에서만 허용함.
- 2 BeginItem: 페이로드 8바이트, uint64 선언 길이.
  BetweenItems에서 새 비어 있지 않은 항목 ID를 사용함.
  과거 항목 ID 재사용은 거절하며 선언 길이는 남은 데이터 예산 이하여야 함.
  길이 0인 항목도 허용함.
- 3 Data: 페이로드 1~65536바이트의 가공하지 않은 바이트.
  InItem에서 현재 항목 ID만 허용하며 선언 길이와 전체 데이터 예산을 넘을 수 없음.
- 4 EndItem: 페이로드 8바이트, uint64 실제 길이.
  현재 항목 ID와 누적 Data 길이 및 BeginItem 선언 길이가 모두 일치해야 함.
  이후 BetweenItems로 돌아감.
- 5 EndJob: 페이로드 1바이트, 빈 항목 ID.
  1 Completed는 BetweenItems에서만 허용함.
  2 Failed와 3 Cancelled는 BetweenItems 또는 InItem에서 허용하며 활성 항목을 중단함.
  Ready에서 바로 종료하거나 알 수 없는 종료 코드를 보내면 거절함.

정상 수신 흐름은 Ready -> BetweenItems -> (InItem -> BetweenItems 반복)
-> Terminal -> InputCompleted임. 빈 작업도 BeginJob, EndJob, EOF 순서로 허용함.
EndJob 전 EOF, 헤더/페이로드 중간 EOF는 Truncated임.
종료 후 어떤 바이트라도 들어오면 즉시 TrailingData임. 중복 종료도 이에 포함됨.
같은 수신 채널에서 InputCompleted 이후 ReadAsync는 취소된 토큰이나 만료된 기한에도
null을 반환하며 Phase와 기존 TerminalStatus를 보존함. Failed/Cancelled 종료를 Completed로 바꾸지 않음.
Faulted 재읽기는 취소/기한보다 WorkerStreamError.Faulted를 우선함.
최초 실패 호출의 예외 처리와 오류 분류는 그대로 유지하며, 재읽기에서 최초 예외를 재발생시키지는 않음.
확정 상태 검사는 연결 취소 토큰 생성과 I/O보다 먼저 수행함.
EndJob만 받은 Terminal 상태는 EOF가 미검증이므로 취소/기한 검사를 계속 적용함.
EOF 이후 데이터를 다시 공급하는 비표준 스트림은 지원하지 않음.

프레임 헤더는 페이로드 할당 전에 검증함. 알려지지 않은 프레임 종류 또는 종류별
길이 불일치는 InvalidLength로 거절함. 누락/중복/역순 순번, 다른 작업 ID,
활성 항목 ID 불일치를 거절함. 오류나 취소 후에는 Faulted로 고정하고 TerminalStatus를 지움.
부분 읽기/쓰기의 재시도나 재동기화를 지원하지 않음.

## 예산, 메모리와 취소

WorkerStreamLimits는 기본값보다 작게만 설정할 수 있음.

- MaxDataBytes: 작업 전체 Data 합계 16 MiB 이하. 항목마다 초기화하지 않음.
- MaxWireBytes: 헤더와 모든 페이로드 합계 21 MiB 이하, 최소 129바이트.
- MaxItems: 1024 이하. 비어 있는 항목도 계산함.
- MaxFrames: 65536 이하, 최소 2. 작은 청크 반복도 이 예산으로 제한함.
- MaxDuration: 생성 시점부터 최대 5분. 호출 사이 대기 시간도 포함함.

BeginItem 선언 길이를 배열 크기로 사용하지 않음. 수신은 고정 헤더와 검증된
최대 64 KiB 페이로드만 할당함. 송신도 길이/상태 검증 후 최대 64 KiB만 복사함.
직전 프레임 이후 다음 호출을 기다리는 방식이며 내부 큐를 두지 않음.
반환 프레임을 호출자가 무제한 보관하는 것까지 메모리 제한을 보장하지 않음.
항목 ID 집합은 MaxItems로 제한됨. 누적 카운터는 차감 비교 후 증가함.

채널은 전체 기한 토큰과 호출자의 취소 토큰을 I/O에 함께 전달함.
기한 초과는 BudgetExceeded, 호출자 취소는 OperationCanceledException으로 구분함.
취소를 무시하는 사용자 정의 Stream의 강제 중단이나 OS 자원 격리를 보장하지 않음.
실제 프로세스 연결 시 호스트의 시간 제한, 프로세스 트리 종료와 종료 대기가 추가로 필요함.
취소 프레임 전송은 별도 제어 채널이 아니며 일반 EndJob(Cancelled)임.
로컬 취소 중에는 종료 프레임 전달을 보장하지 않으므로 상대는 EOF/시간 초과를 처리해야 함.

예산은 한 채널/한 방향 단위임. 양방향 통합 작업 예산, 압축 해제량/비율/디스크 예산은
후속 통합에서 별도로 부과해야 하며 기존 JobBudget를 대체하지 않음.
이 계층은 경로에 접근하거나 원본/결과 파일을 수정하지 않음.

## 실행 경계와 검증 인계

Windows와 Ubuntu에서 동일한 바이트 계약을 사용함.
기존 고정 Worker 앱호스트, 명시적인 dotnet DLL-host 실행, NTFS/ACL 또는
ext4/XFS/fd 게시 경계는 이번 변경 대상이 아님. stdin/stdout 연결도 아직 없음.
일반 capability와 v1 상한은 그대로 유지함.

필수 별도 검사는 정상/빈 항목, 64 KiB 경계, 여러 항목의 16 MiB 누적 경계,
거대한 선언/프레임 길이의 할당 전 거절, 예약/버전/종류 오류, 잘린 헤더/본문,
ID/순번 오류, 항목 재사용, 조기/중복 종료, 후행 바이트, EOF 누락,
빈 항목/작은 청크의 예산 소진, 느린 소비자, 읽기/쓰기 중 취소와 I/O 실패임.
각 오류 뒤 채널 재사용 거절, 실패/취소 종료가 성공으로 해석되지 않는지도 검사해야 함.
두 OS의 실제 파이프와 DLL-host/앱호스트 검증은 후속 통합 단계임.

현재 로컬 SDK 부재로 새 C# 컴파일과 실행은 미검증임.
기존 v1의 98개 통과 결과를 새 계층의 검증 결과로 재사용하지 않음.
성능, 처리량, 메모리 최대치, 취소 지연의 개선 수치를 아직 주장하지 않음.

# 명시적 ZIP 스트리밍 해제

## 범위와 진입점

기준 main은 626483e8935aaa03d5a34feebc61be2f9180b80d임.
새 API는 Zipelo.Runtime.Streaming.StreamingZipWorkerClient임.

```csharp
var client = new StreamingZipWorkerClient(trustedWorkerPath, explicitDotnetHostPath);
var limits = new JobLimits(
    MaxEntries: 1024,
    MaxProcessedBytes: 128L * 1024 * 1024,
    MaxOutputBytes: 16L * 1024 * 1024,
    MaxDuration: TimeSpan.FromMinutes(5),
    MaxPasswordAttempts: 1);
using var source = new FileStream(zipPath, FileMode.Open, FileAccess.Read, FileShare.Read);
var result = await client.ExtractToNewFolderAsync(
    Guid.NewGuid(), source, newDestinationFolder, limits, cancellationToken);
```

앱호스트 직접 실행은 explicitDotnetHostPath를 null로 전달함.
DLL 실행은 신뢰하는 dotnet 호스트의 절대 경로를 명시해야 함.
PATH나 입력 파일의 위치에서 실행 파일을 찾지 않음.
Worker에 정확히 --extract-stream-v2 인수 하나를 추가함.
기존 무인수 v1 실행, ZipWorkerClient, UI, capability, 게시 API와 프로토콜 v2 기반 파일은 변경하지 않음.
새 API를 호출하지 않으면 새 경로가 실행되지 않음. 자동 전환이나 상한 확대는 없음.

비암호화 단일 디스크 ZIP32의 Stored/Deflate만 허용함.
ZIP64, 암호, 분할, SFX, 알 수 없는 압축 방식, 링크, 미해석 확장 필드는 지원하지 않음.
UTF-8 이름 또는 UTF-8 플래그가 없는 ASCII 이름을 수용함.
지원하는 확장 필드는 0x5455 타임스탬프뿐임.
형식 근거는 [PKWARE APPNOTE 6.3.10](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT)의
로컬/중앙 레코드, CRC, 데이터 디스크립터 계약임.
기존 해제의 제한을 바탕으로 한 보수적 하위집합이며 모든 ZIP 호환성을 주장하지 않음.

## 입력과 임시 저장

입력은 읽기 가능, 쓰기 불가, seek 가능한 Stream이어야 함.
현재 Position부터 Length까지를 처리하고 반환 전에 원래 Position으로 되돌림.
Stream 수명과 동시 접근 방지는 호출자의 책임임. 비협조적인 사용자 정의 Stream의
취소 무시, 무한 지연, 거짓 Length에 대한 OS 수준 격리는 제공하지 않음.

호스트는 목적지 부모에 기존 네이티브 게시 계층으로 별도 임시 디렉터리와 input.zip을 생성함.
모든 입출력은 열린 파일 핸들에 연결됨. 원본 경로를 Worker에 전달하지 않음.
입력을 최대 64 KiB씩 복사하면서 SHA-256을 계산함.
다시 원본을 읽어 스냅샷의 핸들 읽기와 바이트 단위로 비교하고,
스냅샷 재읽기 해시 및 원본 Length도 비교함.
이후 파싱과 Worker의 범위 읽기는 스냅샷만 사용함.

길이 변경이나 두 읽기 사이의 내용 불일치를 거절함.
동시 쓰기가 잠깐 발생했다가 되돌아오는 변경, 두 번 동일하게 관찰되는 혼합 상태까지
검출하는 파일시스템 시점 스냅샷은 아님. 강한 일관성은 호출자가 원본 쓰기를 배제해야 함.
API는 원본 내용을 쓰거나 원본 파일/폴더의 권한을 바꾸지 않음.

입력 스냅샷과 결과 임시 폴더는 독립된 소유 트리임.
최종 부모의 Windows NTFS/ACL 또는 Linux ext4/XFS/소유권/쓰기 권한 조건을 그대로 요구함.
Linux 0775 등 기존 조건 미충족 부모를 chmod로 바꾸지 않음.
성공한 입력 정리와 Worker 종료 확인 뒤에만 결과를 한 번 게시함.

## 호스트와 Worker 계약

stdin은 최초 64바이트 설정과 범위 응답, stderr는 범위 요청, stdout은 기존 v2 출력 프레임 전용임.
stderr를 로그나 자유 텍스트로 쓰지 않음. 모든 정수는 little-endian임.
Guid 순서는 WORKER_STREAM_PROTOCOL.md와 동일함.

설정 64바이트:

- 0: ASCII ZXE2, 4바이트.
- 4: int32 버전 2.
- 8: 작업 Guid, 16바이트.
- 24: int64 입력 길이.
- 32: int64 MaxProcessedBytes.
- 40: int64 MaxOutputBytes.
- 48: int64 MaxEntries.
- 56: int64 MaxDuration.Ticks. 암호 시도는 사용하지 않으며 Worker에서 1로 고정함.

범위 요청/응답 헤더 48바이트:

- 0: ASCII ZXR2, 4바이트.
- 4: int32 버전 2.
- 8: 작업 Guid, 16바이트.
- 24: uint64 요청 순번, 0부터 정확히 증가함.
- 32: int64 입력 오프셋.
- 40: int32 요청 길이, 1~65536.
- 44: 예약 int32 0.

응답은 요청 헤더를 정확히 반복하고 요청한 길이의 원시 바이트를 이어 보냄.
Worker는 헤더 일치를 확인함. 호스트는 ID/버전/순번/예약 필드, 입력 범위,
요청 크기와 누적 예산을 검사한 뒤 핸들에서 읽음.
한 번에 요청 하나만 진행하며 내부 요청 큐는 없음.
알 수 없는 모드로 종료 코드 2를 반환하는 구 Worker는 UnsupportedOperation으로 보고함.
다른 버전/잘린 프레임/잘못된 응답과 비정상 종료는 성공으로 처리하지 않음.

호스트와 Worker가 각각 제한된 ZIP 목록을 읽고 로컬/중앙 헤더와 이름, 길이,
CRC, 디스크립터, 압축 데이터 범위의 중첩/간격, 항목 종류를 검사함.
호스트는 중복 이름, 경로 탈출, 깊이, 파일/디렉터리 충돌도 독립적으로 검사함.
Worker가 경로 또는 CRC 메타데이터를 호스트에 지정하는 메시지는 없음.
호스트 스냅샷에서 얻은 목록이 출력 위치와 기대 CRC의 기준임.

stdout에는 BeginJob, 각 목록 항목의 BeginItem/Data/EndItem, EndJob(Completed)을 보냄.
항목 Guid는 16바이트의 앞 4바이트에 목록 인덱스+1을 int32로 쓰고 나머지는 0으로 둠.
BeginItem의 선언 길이는 호스트 목록과 일치해야 함.
호스트는 순서/ID/길이와 실제 청크 CRC를 목록에 대조하고,
항목 파일을 FlushToDisk한 뒤 다시 읽어 길이/CRC를 검증함.
Worker 또한 실제 해제량과 CRC를 검사함.
전체 항목 수, EndJob(Completed), 뒤따르는 EOF, 정상 종료 코드 0,
범위 서버의 오류 없는 종료가 모두 필요함.
이후 소유 핸들/이름/자식 검사를 거쳐 기존 덮어쓰기 금지 게시를 한 번 수행함.
CRC는 ZIP 무결성 검사이며 악의적 작성자의 진위 또는 암호학적 인증을 보장하지 않음.

## 독립 예산과 취소

- 입력 데이터 최대 16 MiB, 해제 출력 전체 최대 16 MiB임.
- 임시 파일 내용은 입력과 출력 합계 최대 32 MiB임.
  실제 파일시스템 블록과 디렉터리 메타데이터 점유량에 대한 OS quota는 아님.
  노드 수도 제한하며 디스크 부족은 실패와 소유 항목 정리로 처리함.
- 파서가 다루는 메타데이터 읽기/주석 합계는 각 파싱마다 최대 2 MiB임.
  이 값은 관리 힙 전체의 바이트 수를 뜻하지 않음. 이름은 UTF-8 4096바이트,
  해석 후 기존 1024문자 제한을 적용함. 파서의 가장 큰 임시 버퍼는 EOCD 검색용 65557바이트임.
- 목록은 1024항목, 경로 깊이는 32, 결과 노드는 루트 제외 최대 4096개임.
- 범위 요청은 65536회, 누적 응답 데이터는 48 MiB까지임.
- stdout은 v2의 64 KiB 청크, 65536프레임, 21 MiB 전송 예산을 유지함.
  출력 데이터를 제외한 경로/CRC는 호스트 목록에 있으므로 16 MiB 출력에 메타데이터를 끼워 넣지 않음.
- 호출자가 준 MaxEntries/MaxOutputBytes/MaxProcessedBytes가 더 작으면 작은 값으로 제한함.
  전체 처리량의 하드 상한은 128 MiB임.
  호스트는 원본 복사, 원본/스냅샷 재읽기, 목록 읽기, 범위 응답, 실제 출력 수신과 저장 후 재읽기를 누적함.
  Worker도 파싱/범위 읽기/해제량을 자체 예산에 계산함.
- 최종 게시 전까지 원래 호출의 전체 시간 예산과 취소 토큰을 유지함. 최대 5분임.
  Worker 설정과 채널도 별도 제한을 갖지만 호스트의 최초 기한을 연장하지 않음.

호스트는 stdout 수신과 stderr 범위 서비스를 동시에 진행함.
출력 청크를 기록한 뒤 다음 청크를 읽으므로 무제한 큐를 만들지 않음.
Worker의 동기 범위 읽기가 막히더라도 호스트가 취소/시간 초과 시 프로세스 트리를 종료함.
종료와 범위 서비스 정지 확인은 각각 최대 10초를 기다림.
이 정리 대기 시간은 작업 시간 예산 뒤에 추가될 수 있음.
프로세스 분리는 OS 수준 메모리/CPU/권한 샌드박스가 아님.

## 결과와 실패 처리

StreamingExtractionResult.Publication.Published가 true일 때만 새 폴더 게시 완료임.
VerifiedEntries는 임시 항목 검증 수이며 최종 게시 성공과 다름.
Usage는 호스트에서 누적한 JobBudget 사용량임.
WorkerExitConfirmed는 Worker가 시작되지 않았거나 실제 종료가 확인되었음을 뜻함.
WorkerExitCode는 시작된 Worker의 종료 확인 시에만 존재함.

출력 정리 실패는 Publication.CleanupFailed/TemporaryFileName,
입력 정리 실패는 InputCleanupFailed/InputTemporaryName으로 각각 보고함.
확인 가능한 네이티브 실패 코드도 결과에 포함함.
종료 또는 범위 서비스 정지가 확인되지 않으면 임시 트리를 삭제하지 않고 실패로 남김.
정리할 수 없던 이름은 목적지 부모 아래의 이번 작업 임시 이름임.
기존 대상/사용자 파일을 합치거나 삭제해 복구하지 않음.
이미 성공한 게시를 늦은 취소로 실패로 바꾸지 않음.

## 검증 상태

소스 경로는 구현했으나 로컬 SDK가 없어 새 C# 컴파일과 실제 OS 실행은 미검증임.
기존 98개와 프로토콜 46개의 통과 결과를 이 새 기능의 결과로 재사용하지 않음.
성능 개선이나 대용량 지원 완료를 주장하지 않음.

독립 수용 검사는 Stored/Deflate/빈 파일/디렉터리/빈 ZIP, 정확한 청크/입출력 경계,
원본 내용/위치 보존과 원본 변경, 메타데이터/범위/처리량/항목/시간 예산,
불일치 ID/길이/CRC/순번, 누락/추가 항목, 구 Worker/잘못된 프로토콜/비정상 종료,
느린 파이프와 쓰기 중 취소, 저장 실패/디스크 부족, 입력/출력 정리 실패,
기존 대상 충돌과 경로 교체, Windows NTFS 및 Ubuntu ext4 조건을 포함해야 함.
기존 144개 회귀와 새 검사는 고정된 소스 SHA에서 두 OS별로 실행해야 함.

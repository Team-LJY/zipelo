# 기본 ZIP 작업자와 런타임

## 후속 OS 정책과 파일 게시

이 문서의 최초 구현 이후 [OS 정책과 단일 ZIP 게시 API](ZIP_PUBLICATION.md)를 추가함. 현재 코드의 SupportsFilePublication은 Windows/Linux x64 게시 구현 여부를 반환하며, 기존의 항상 false 설명은 최초 구현 기록임. CreateAsync 자체는 계속 메모리 생성만 수행함. 실제 새 파일 저장은 별도 NewZipPublisher.PublishAsync가 담당하며 플랫폼별 실행 검증은 남아 있음. Extract는 계속 비활성임.

## 현재 범위

외부 패키지 없이 .NET 10의 System.IO.Compression을 사용하는 초기 구현임. 기존 Core 공개 시그니처는 유지하며, 후속 변경에서 OS별 경로 정책을 분리함. 실제 C# 빌드와 실행 검증은 이 문서 작성 환경에서 하지 못했으며, 아래 동작은 구현된 코드의 계약임. 기존 29개 테스트의 Windows 통과 보고는 이번 새 작업자의 검증 결과가 아님.

- `src/Zipelo.Runtime`: UI가 호출할 클라이언트와 길이가 제한된 IPC 프레임을 제공함.
- `src/Zipelo.Worker`: 작업 한 건마다 별도 프로세스에서 ZIP 파싱과 생성을 수행함. UI는 ZipArchive를 실행하지 않음.
- 목록은 ZIP 메타데이터만 읽으며 항목 ID를 중앙 디렉터리 순서로 생성함. 같은 이름의 항목도 ID로 구분함. 이름은 표시 데이터이며 안전한 출력 경로로 승인된 것이 아님.
- 생성은 읽기 전용 입력 스트림과 명시한 항목 이름으로 실제 ZIP 바이트를 생성함. 폴더 재귀 탐색과 빈 폴더 생성은 없음. 빈 입력 목록은 거부함.
- 생성물은 메모리에만 존재함. 디스크에 ZIP 파일을 저장한 결과가 아니며, UI에서 저장 완료로 표시하면 안 됨.
- 추출, 검증, 복구, 미리보기, 암호화와 분할 생성은 지원 능력에 포함하지 않음. 이 초기 구현의 범위이며 제품 전체의 최종 지원 범위를 줄이는 결정이 아님.

## UI 진입점

`Zipelo.Runtime.ZipWorkerClient(string workerPath, string? dotnetHostPath = null)`을 사용함. 작업자와 선택적인 dotnet 호스트 경로는 모두 앱이 정한 신뢰할 수 있는 절대 경로여야 함. 압축파일 내용이나 사용자 입력을 실행 파일 경로로 사용하지 않음.

apphost 예시는 앱 폴더 아래 `worker/Zipelo.Worker.exe`임. DLL 방식은 `Zipelo.Worker.dll`과 설치된 dotnet 호스트의 절대 경로를 함께 전달함. DLL, deps.json, runtimeconfig.json과 프로젝트 의존 DLL을 포함한 전체 빌드 출력이 필요함. 런타임 자동 탐색, 다운로드, 설치는 수행하지 않음. `IsAvailable`은 파일 존재 조건이며 실제 실행 성공 검증이 아님.

- `ListAsync(Guid jobId, Stream zip, JobLimits limits, CancellationToken token)`
- `CreateAsync(Guid jobId, IReadOnlyList<ZipCreateInput> inputs, JobLimits limits, ZipCreationOptions? options, CancellationToken token)`
- `ZipCreateInput(string EntryName, Stream Content)`
- `ZipCreationOptions(bool Encrypt = false, long? SplitVolumeBytes = null)`
- `ZipWorkerResult(JobResult Job, byte[]? CreatedZip)`

입력 스트림은 읽기 가능하고 쓰기 불가능해야 함. 현재 위치부터 끝까지 읽으며 호출자가 스트림 수명을 관리함. 호스트는 파일을 읽기 전용으로 열어야 하며 사용자 선택 원본을 수정하지 않아야 함. 스트림 복사 중 원본이 외부에서 바뀌지 않는다는 보장은 호스트의 파일 열기 정책에 달려 있음. 파일 경로는 작업자에 전달하지 않음. 목록 API에는 파일 확장자가 없으므로 JAR 제외 정책은 파일 선택을 담당하는 UI에서 적용해야 함.

`Capabilities`는 작업자 파일이 없으면 빈 목록이며, 있으면 `EngineCapability("zip", List)`와 `EngineCapability("zip", Create)`임. 여기서 Create는 ZIP 바이트 생성 능력임. `SupportsEncryption`, `SupportsSplitVolumes`, `SupportsSafeExtraction`은 false임. 후속 추가된 `SupportsFilePublication`은 Windows/Linux x64 게시 구현 여부이며 OS별 실행 검증 완료를 뜻하지 않음. 생성 저장 버튼은 Create만 보고 활성화하지 말고 FilePublication도 확인해야 함.

암호화 또는 분할 생성 옵션은 입력을 읽기 전에 UnsupportedOperation으로 거부함. 암호화 항목을 발견한 목록 요청도 UnsupportedOperation으로 종료함. 분할 ZIP을 연결하거나 볼륨을 찾아 읽지 않음. 읽을 수 없는 분할 또는 손상 ZIP은 .NET이 거부하며 InvalidDataException은 IntegrityFailure로 전달함. 각 분할 변종을 구분하는 별도 파서는 없음.

반환값의 Job.State, Job.Error, Job.Entries를 함께 사용함. 실패 전에 수집한 목록은 있을 수 있지만 완전한 목록이나 검증 성공을 뜻하지 않음. 실시간 진행 이벤트와 암호 콜백은 아직 IPC에 없음. UI는 작업 중 상태를 유지하되 가짜 백분율이나 암호 요청을 만들지 않음.

## 자원과 결과 계약

원본 입력 합계와 ZIP 생성물은 각각 최대 16 MiB, IPC 프레임은 32 MiB, 입력 개수와 목록 항목은 1024개, 항목 이름은 1024자로 제한함. JobLimits가 더 작으면 그 예산을 적용함. 요청 시간은 5분 이하여야 하며 호스트의 제한 시간은 입력 복사, 프로세스 실행, 응답 수신과 종료 대기를 포함함.

작업자는 기존 ArchiveJob과 EngineContext를 사용함. 목록은 ZIP 입력 바이트를 처리 예산에 누적함. 생성은 원본 바이트와 최종 ZIP의 출력 전달 바이트를 처리 예산에 누적하며 ZIP 바이트를 출력 예산에 반영함. 생성 시 입력 항목마다 Listed 결과를 남기고 최종 archive.zip 항목 하나를 더 등록하므로 MaxEntries에는 입력 수 + 1이 필요함. 더 낮은 한도에서는 전체 작업이 실패하고 생성물을 반환하지 않음.

생성 완료의 artifact 항목은 기존 Core의 Extracted 값으로 반환되지만 여기서는 메모리 트랜잭션 확정을 뜻함. 디스크 해제 또는 저장을 뜻하지 않음. 모든 항목의 검증 결과는 NotChecked이며 CRC나 인증 검증을 통과했다고 표시하지 않음. .NET 10의 읽기 완료만으로 CRC 검증을 보장할 수 없음. [공식 CRC32 변경 설명](https://learn.microsoft.com/en-us/dotnet/core/compatibility/core-libraries/11/ziparchive-entry-crc32-validation), [암호화 여부 API](https://learn.microsoft.com/en-us/dotnet/api/system.io.compression.ziparchiveentry.isencrypted?view=net-10.0)

외부 취소는 Cancelled, 호스트 시간 초과는 BudgetExceeded로 반환함. 응답 반환 전에 실행 중 작업자 프로세스 트리를 종료하고 실제 종료를 기다림. 종료 확인 자체가 실패하면 예외를 숨기지 않으므로 UI는 예외 경로에서도 화면 상태를 복구해야 함. 메모리 생성물은 응답이 완전히 전달되기 전의 취소에서 폐기할 수 있으며 이미 게시된 외부 파일은 없음.

프로세스 분리는 보안 샌드박스가 아님. 현재 사용자 권한으로 실행하며 Windows Job Object, 제한 토큰, OS 메모리 제한, 네트워크 차단은 구현하지 않음. 프레임과 입력 한도는 전체 프로세스 메모리 상한이 아니며 JSON과 메모리 복사, ZIP 메타데이터의 추가 할당이 있음. UI는 작업을 중복 시작하지 않아야 함. Worker 실행 파일 무결성과 배포 폴더 신뢰는 호스트 책임임.

## 실패 폐쇄 출력

Windows 핸들 기반 경로 고정과 단일 ZIP 새 파일 게시는 후속 구현되어 담당자의 실행 검증을 거쳤으나, 새 폴더 게시와 OS별 경합 방어의 실제 검증은 아직 남아 있음. 초기 구현에는 출력 트랜잭션과 공개 해제 진입점이 없었으나 아래 메모리 해제 확장과 새 게시 API가 후속 추가됨. 일반 UI의 Extract capability는 계속 비활성임. 문자열 경로 검사는 ZIP 생성 항목 이름에만 사용하며 파일시스템 안전성의 증거로 사용하지 않음.

CreatedZip을 일반 File.WriteAllBytes로 사용자 목적지에 저장하지 않음. 후속 추가된 NewZipPublisher.PublishAsync를 사용하며, 구체적인 계약과 OS별 검증 공백은 [게시 API 문서](ZIP_PUBLICATION.md)에 기록함.

## 확인과 다음 검증

로컬에서는 새 프로젝트 XML, 참조 경로, 공백과 변경 범위, 기존 Core와의 메서드 시그니처를 정적으로 대조함. Linux PATH에 dotnet, csc, mcs가 없어 컴파일과 실제 ZIP 실행은 하지 않음. Desktop과 tests는 담당자가 별도로 변경하며 이 작업에서는 수정하지 않음. 기존 브랜치를 보존하고 커밋, 설치, 외부 업로드와 VM 접속은 하지 않음.

Windows 검증에서는 Worker와 Runtime을 먼저 빌드하고 전체 작업자 출력을 앱의 worker 폴더로 배치한 뒤 목록을 확인해야 함. 총괄이 실행 환경과 복사를 조율함. 필요한 회귀 사례는 정상 ZIP 목록, 중복 이름의 서로 다른 ID, 빈 ZIP, 암호화 및 손상 ZIP 오류, 명시한 이름의 생성물 재열람과 바이트 대조, 생성 암호와 분할 옵션 거부, 대소문자와 부모/자식 이름 충돌, 입력/출력/항목/시간 한도, 취소 후 프로세스 소멸, 누락된 작업자 및 비정상 종료, 생성 실패 시 CreatedZip 없음, Extract 거부임. 이 문서는 실행 통과를 주장하지 않음.


## 메모리 해제 확장

ExtractAsync와 선택적 ExtractedEntries 결과를 추가함. 경로, 타입, 실제 길이와 CRC 검증은 별도 작업자에서 수행하며 기존 List/Create 계약은 유지함. 일반 해제 capability와 SupportsSafeExtraction은 비활성임. 새 폴더 게시 API는 Windows NTFS의 신뢰 가능한 쓰기 ACL, Linux XFS의 현재 사용자 소유와 외부 쓰기 금지 조건에서만 실행함. ext2/ext3/ext4는 공통 파일시스템 식별값으로 구분할 수 없어 임시 항목 생성 전에 실패 폐쇄함. ext4 지원에는 확실한 fd 기준 판별과 실제 검증이 필요함. 핸들 상대 생성, 전체 바이트 검증, 덮어쓰기 금지 게시와 소유 항목 정리를 구현했으며 실제 C# 실행은 미검증임. 정확한 제한과 검증 대기는 [ZIP_EXTRACTION.md](ZIP_EXTRACTION.md)를 따름.

# OS 정책과 단일 ZIP 게시 API

> 과거 C# 구현 기록: C#은 v0.1에서 중단하고 Rust로 대체했다. PR103(f1d9fa5)에서 C# 소스와 전용 시험 및 .NET 설정을 제거했다. 아래 본문의 "현재", 미구현 상태, API와 검증 결과는 작성 당시 C# 스냅샷에 관한 내용이며, 현재 Rust 제품의 지원 범위나 검증 상태를 뜻하지 않는다. 삭제된 경로와 실행 명령은 현재 main에서 사용할 수 없다.
>
> 현재 기준은 [기획서](PLANS.md), [UI 설계](ui-design-v0.1.md), [릴리스 노트](release-notes-v0.1.md)다. 앱은 [Rust UI](../experiments/rust-ui-memory-app/), 압축 처리는 [Rust 작업자](../experiments/rust-archive-worker/README.md)를 사용한다. 과거 검사 재현과 소스 보존 안내는 [기획서의 과거 기록](PLANS.md#7-c-중단과-과거-검증-기록)을 따른다.

## 보존된 당시 기록

## 상태와 범위

Windows x64와 Ubuntu 24.04/26.04 LTS x64용 소스를 추가한 상태임. 담당자 보고 기준 2026-11-09T13:59:27.796888Z Windows 스냅샷은 빌드 경고/오류 0, C# 57/60 통과이며 게시 관련 3개가 실패함. 이후 아래 Windows 게시 수정은 로컬 SDK 부재로 컴파일과 실제 실행을 검증하지 못했음. Ubuntu 두 LTS에서의 C# 실행 검증도 남아 있음.

이 API는 사용자가 저장 대화상자에서 명시적으로 선택한 단일 새 파일에 이미 생성된 ZIP 바이트를 게시함. 임의 압축 항목을 경로별로 해제하는 출력 트랜잭션과 별개임. Extract capability, 안전한 해제, 미리보기와 복구는 계속 비활성임. IPC 메시지와 기존 ListAsync/CreateAsync 시그니처는 유지함.

## 파일명과 작업자 경로

Core의 `OutputPathPolicy.Validate(string, FileNamePolicy)` 오버로드와 `CurrentPolicy`, `Comparer`, `Comparison`을 추가함. 기존 단일 인수 Validate는 Windows 사전 정책을 유지하여 명시적 Windows 경로 검사의 호환성을 보존함. EngineContext와 ZIP 생성 어댑터는 현재 OS 정책을 명시적으로 사용함.

- Windows: 기존 예약명, ADS, 끝의 공백/마침표, NFC와 금지 문자 검사를 유지함. 대소문자를 구분하지 않는 충돌 검사이며 대소문자만 다른 파일을 조용히 합치지 않고 오류로 반환함. Windows의 선택적 디렉터리별 대소문자 구분 기능은 아직 지원하지 않음.
- Linux: 구분자는 슬래시만 사용함. 역슬래시, 콜론, 예약명처럼 보이는 이름, 끝 공백/마침표, 제어 문자와 비NFC 이름을 Windows 규칙으로 거부하거나 바꾸지 않음. 대소문자는 Ordinal로 구분하며 A와 a를 별도 항목으로 보존함. 절대 경로, 빈 세그먼트, .과 .., NUL, 세그먼트당 UTF-8 255바이트 초과는 거부함. 잘못된 UTF-16과 비UTF-8 원시 파일명은 지원하지 않음.
- 생성 항목명은 실행 OS의 정책을 사용함. Linux에서 합법적인 이름의 ZIP을 다른 OS에서도 같은 방식으로 해제할 수 있다는 보장은 아님. 목록 표시 이름은 안전한 출력 경로 승인을 뜻하지 않음.

`WorkerPlatform.ExecutableName`은 Windows에서 Zipelo.Worker.exe, Linux에서 Zipelo.Worker임. `WorkerPlatform.GetWorkerPath(string applicationDirectory)`는 절대 앱 폴더 아래 worker 디렉터리를 사용함. 작업 디렉터리나 압축파일 폴더를 탐색하지 않음. UI 담당자는 각 프로젝트에서 이 진입점을 사용할 수 있음. Linux 실행 권한과 실제 배포 파일 배치는 설치/빌드 담당 범위이며 이 작업에서 권한을 변경하지 않음.

## 공개 API와 UI 연결

`Task<ZipPublicationResult> NewZipPublisher.PublishAsync(ZipWorkerResult result, string destinationPath, CancellationToken token)`

`ZipPublicationResult(bool Published, ArchiveError Error, bool CleanupFailed, string? TemporaryFileName = null)`

CreateAsync가 반환한 성공 결과와 사용자가 고른 절대 목적지를 전달함. 임의 생성한 DTO나 작업자가 제공한 경로를 사용자 선택으로 간주하지 않음. API는 생성 ZIP을 다시 파싱하거나 CRC 검증하지 않음. 성공 상태와 생성 바이트 유무, 16 MiB 한도를 확인함. 호출자는 배열을 호출 중 변경하지 않아야 하며 API는 첫 await 전에 복사본을 확보함. 원래 JobResult는 메모리 생성 결과로 유지하고 게시 결과를 별도로 UI에 반영함.

Published=true일 때만 저장 완료를 표시함. 기존 파일, 기존 symlink, 기존 hardlink와 원본 입력 경로에 게시하면 덮어쓰지 않고 실패함. 별도의 출력 파일을 새 inode로 만들기 때문에 기존 입력 inode를 쓰기나 자르기로 열지 않음. 입력 파일 경로 자체를 API가 받거나 원본의 외부 변경을 감시하는 것은 아님.

Published=false이면 Error를 표시함. CleanupFailed=true이면 일반 실패와 별도로 임시 파일 정리 실패를 알려야 함. Windows에서 제공되는 TemporaryFileName은 원래 선택한 부모 폴더 내 작업 소유 임시 이름임. 임의 경로 재삭제를 자동 시도하지 않음. 늦은 취소가 실제 게시 성공을 취소로 바꾸지 않으므로 결과의 Published를 우선함.

`NewZipPublisher.IsSupportedPlatform`과 클라이언트의 `SupportsFilePublication`은 Windows/Linux x64 구현 경로가 있다는 뜻임. 해당 LTS, 파일시스템 또는 앱 배포물에서 검증 완료라는 뜻이 아니며 필요한 네이티브 기능이 없으면 호출이 실패함. UI 활성화 전 해당 플랫폼의 빌드와 저장 회귀 검증이 필요함.

## Windows 구현

로컬 고정/이동식 드라이브의 일반 절대 경로만 받음. UNC, 네트워크 드라이브와 사용자 지정 장치 경로는 실패함. 루트부터 각 디렉터리를 핸들로 열고 닫지 않은 상태로 유지함. 자식 열기는 NtCreateFile의 RootDirectory 상대 열기를 사용하고 reparse point를 따라가지 않으며 디렉터리 속성과 reparse 태그를 검사함. 디렉터리 핸들은 삭제 공유를 허용하지 않음. 문자열 검사 후 경로로 다시 파일을 여는 방식이 아님.

같은 부모 핸들 아래 .zipelo-UUID.tmp를 FILE_CREATE와 배타 공유로 생성함. 기존 파일을 열거나 덮어쓰는 disposition을 사용하지 않음. 데이터 쓰기와 FlushToDisk를 마친 뒤 NtSetInformationFile의 FileRenameInformation(10)을 적용하고 RootDirectory를 고정한 부모 핸들로 지정함. ReplaceIfExists=false로 목적지가 있으면 실패함.

게시 전 실패와 취소는 보유 임시 파일 핸들에 FileDispositionInfo를 설정하고 닫아 제거함. 정리 실패는 CleanupFailed로 보고함. 프로세스 강제 종료나 전원 손실에서는 이름 있는 임시 파일이 남을 수 있으며 자동 복구/청소는 아직 없음. 디스크 전체의 전원 손실 내구성이나 디렉터리 메타데이터까지 flush한 트랜잭션을 약속하지 않음.

junction, symlink, 부모 교체, 기존 목적지, 공유 위반, FAT/NTFS/ReFS 차이와 네이티브 구조체 배치는 Windows에서 실제 검증해야 함. 구현이 있다는 이유만으로 이 시험을 통과했다고 표시하지 않음.

## Linux 구현

x64 openat2를 RESOLVE_NO_SYMLINKS로 호출하여 모든 부모 경로의 symlink/magic link를 거부하고 선택된 디렉터리를 고정함. 디렉터리 fd 아래 O_TMPFILE로 이름 없는 파일을 생성하고 0600 이하 권한을 사용함. 기존 권한이나 보안 설정을 바꾸지 않음. 데이터 쓰기와 파일 fsync 후 커널 procfs의 /proc/self/fd/N을 linkat으로 같은 디렉터리 fd의 최종 이름에 연결함. 목적지가 존재하면 EEXIST로 실패하며 그 링크를 따라가지 않음.

openat2, O_TMPFILE, linkat 또는 procfs가 없는 환경과 파일시스템에서는 실패함. 이름 있는 임시 파일이나 단순 File.Move 방식으로 우회하지 않음. 게시 전 실패/취소/프로세스 종료는 이름 없는 inode의 마지막 fd가 닫히면서 제거됨. 게시 성공 후에는 늦은 취소로 삭제하지 않음.

fd를 연 뒤 부모 경로가 교체되더라도 새 경로로 다시 찾아가서 쓰지 않음. Linux는 열린 디렉터리의 이름 변경 자체를 막지 않으므로 게시 위치는 처음 고정한 디렉터리 inode임. 다른 프로세스가 그 폴더를 이동했다면 원래 경로 문자열과 현재 위치가 달라질 수 있음. API는 이 경우 원래 경로 문자열에 저장됐다고 보장하지 않음. 임의 마운트 변경 권한을 가진 공격자나 파일시스템 구현 자체의 악성 동작까지 격리하는 보안 경계는 아님.

링크 생성 뒤 부모 디렉터리 fsync는 수행하지 않으므로 정전 후 파일명의 영속성까지 보장하지 않음. 각 LTS의 기본 파일시스템에서 정상 게시, symlink 부모/목적지, hardlink 목적지, 경로 교체, 취소와 미지원 파일시스템을 별도로 검증해야 함.

## 자원 제한과 미완성

기존 작업자의 16 MiB 입력/생성물, 32 MiB 프레임, 항목 수, 작업 시간 제한은 유지함. 게시 과정은 생성물을 한 번 더 메모리에 복사하고 파일에 씀. 총 프로세스 메모리 상한이나 OS 수준 CPU/메모리 격리는 아님. Windows Job Object/제한 토큰, Linux rlimit/cgroup/seccomp와 네트워크 차단은 미구현임. 작업자와 게시 API 모두 현재 사용자 권한에서 실행함.

게시 취소는 쓰기 사이와 최종 게시 직전에 검사함. 네이티브 open, flush, rename/link 호출을 강제 중단하는 별도 프로세스는 없으므로 OS I/O가 멈추면 즉시 취소 완료를 보장하지 않음. UI는 await 종료 전에 저장 작업을 취소 완료로 표시하면 안 됨.

## 검증 근거와 연결 자료

이 라운드에서는 소스 열람, 프로젝트/문서 정합성 확인만 수행함. UI와 tests는 수정하지 않음. 회귀 담당자가 검증할 정확한 대상은 OS별 Validate 오버로드, EngineContext 충돌 비교, WorkerPlatform 경로와 PublishAsync임. 기존 Windows 전용 테스트를 Linux 정책의 정답으로 사용하지 말고 정책 인수를 명시하거나 OS별 기대를 분리해야 함.

공식 API 근거:

- [NtCreateFile의 상대 디렉터리 핸들과 생성 disposition](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile)
- [FILE_RENAME_INFO의 RootDirectory와 ReplaceIfExists](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_rename_info)
- [FILE_DISPOSITION_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_disposition_info)
- [openat2의 심볼릭 링크 거부](https://man7.org/linux/man-pages/man2/openat2.2.html)
- [O_TMPFILE과 fd 기반 linkat 게시](https://man7.org/linux/man-pages/man2/open.2.html)
- [RandomAccess.FlushToDisk](https://learn.microsoft.com/en-us/dotnet/api/system.io.randomaccess.flushtodisk?view=net-10.0)

## Windows 게시 실패 수정과 재검증 대기

2026-11-09T13:59:27.796888Z 스냅샷의 Windows 실행 결과는 담당자 보고 기준 C# 60개 중 57개 통과, 게시 관련 3개 실패임. 임시 파일 쓰기와 flush 이후 Win32 SetFileInformationByHandle 게시가 실패했으며, 실패 후 임시 파일 제거와 기존 sentinel 보존은 확인됐다고 보고됨. 당시 Win32 오류 숫자가 보존되지 않아 특정 원인을 확정할 수 없음.

수정은 Win32 게시 호출을 부모 핸들 상대 이름 계약이 명시된 NtSetInformationFile로 바꾸는 것임. Win32 FileRenameInfo(3)와 Native FileRenameInformation(10)을 혼용하지 않음. FILE_RENAME_INFORMATION은 x64에서 Flags=0, RootDirectory=8, FileNameLength=16, FileName=20 바이트 오프셋이며 구조체 크기 24바이트임. 버퍼는 구조체 크기와 UTF-16 이름 바이트 수를 더해 할당하고 0으로 초기화함. 이름 길이는 종료 NUL을 제외하며 이름 뒤 공간도 0임. DELETE 권한, 동기 파일 핸들, 부모의 traverse/read-attributes 권한, 삭제 공유 금지, reparse 검사와 덮어쓰기 금지를 유지함. 경로 문자열로 다시 열거나 절대 경로로 게시하는 fallback은 없음.

기존 ZipPublicationResult 생성자와 deconstruction은 유지하고 NativeFailure 및 CleanupNativeFailure init 속성을 추가함. PublicationNativeFailure는 고정 Operation 문자열, uint Win32Error, 선택적 int NtStatus만 포함함. NT 호출은 반환 상태를 즉시 보존하고 RtlNtStatusToDosError로 변환함. Win32 호출은 실패 직후 GetLastPInvokeError를 읽음. 주 실패와 정리 실패 진단을 분리하며 사용자 경로, 생성물, 비밀번호, OS 메시지는 넣지 않음. 80/183은 기존과 동일하게 OutputConflict이며 다른 네이티브 실패는 OutputUnavailable임. UI가 새 속성을 사용하지 않아도 기존 Error 및 CleanupFailed 처리는 유지됨.

이 수정은 로컬 SDK 부재로 C# 컴파일과 Windows 실행을 하지 못했음. 원인이 확정되거나 게시가 통과했다고 표시하지 않음. 재검증에서 새 파일의 정확한 바이트, 기존 파일/원본/최종 링크 충돌 보존, 부모 링크 거부, 실패/취소 정리와 네이티브 오류 진단을 확인해야 함. Ubuntu 코드는 변경하지 않았고 Ubuntu 각 LTS 실행 검증도 별도로 남아 있음. 생성 UI의 일반 배포 활성화와 해제 비활성 정책은 유지함.

- [Native rename의 부모 핸들, DELETE 권한과 버퍼 계약](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information)
- [NtSetInformationFile의 정보 클래스와 NTSTATUS 반환](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntsetinformationfile)

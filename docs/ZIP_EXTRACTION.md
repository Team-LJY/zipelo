# 비암호화 ZIP 해제와 새 폴더 게시

## 상태

메모리 해제와 Windows/Linux x64 새 폴더 게시 소스를 작성함. NewFolderPublisher의 구현 진입점은 연결했지만 일반 UI의 SupportsSafeExtraction=false와 Extract capability 미노출은 유지함. 로컬 .NET SDK가 없어 이번 C# 컴파일과 실제 게시 실행은 미검증임. 이전 Windows 62개 통과는 단일 ZIP 파일 게시에 관한 보고이며 새 폴더 게시의 증거가 아님.

Windows는 로컬 NTFS와 아래 쓰기 ACL 조건에 한정함. Linux는 열린 부모 fd의 mount ID, 장치 번호와 검증된 procfs mountinfo가 ext4 또는 XFS로 일치하는 경우, openat2/statx/renameat2와 아래 소유자/모드 조건에 한정함. ext2/ext3 유형과 확인 불가능한 마운트는 임시 폴더 생성 전에 거부함. Ubuntu 24.04/26.04 LTS의 실제 실행은 각각 별도 검증해야 함. 조건이 확인되지 않으면 OutputUnavailable로 실패하며 다른 경로 API로 우회하지 않음.

## UI와 검증 진입점

- ZipWorkerClient.ExtractAsync(Guid jobId, Stream zip, JobLimits limits, CancellationToken token): 읽기 전용 스트림의 현재 위치부터 입력을 복사하고 별도 작업자에 전달함. 원본 스트림 수명은 호출자가 소유함. 입력 파일과 출력 디렉터리 경로를 작업자에게 전달하지 않음.
- ZipWorkerResult의 기존 Job/CreatedZip 생성자와 기존 ListAsync/CreateAsync 계약을 유지함. 선택적 ExtractedEntries 속성을 추가함. 성공한 해제에서 CreatedZip=null, ExtractedEntries는 빈 배열 또는 ZipExtractedEntry(ArchiveEntry Entry, byte[] Content, uint Crc32) 목록임. 실패에서는 생성물과 해제 목록 모두 null임.
- 메모리 해제 성공은 Job.State=Completed, Job.Error=None이며 각 항목은 Staged/Passed/None임. 이는 디스크에 저장 완료됐다는 뜻이 아님. ExtractedEntries와 Job.Entries는 같은 순서와 메타데이터를 가져야 함. 디렉터리 항목은 끝 슬래시를 제외한 이름, 길이 0, 빈 Content, CRC 0으로 전달함.
- SupportsMemoryExtraction은 작업자 파일의 존재 여부만 표시함. 일반 UI 해제 활성화나 검증 완료 표시는 아님. 구버전 작업자는 실패 응답으로 처리되므로 앱과 작업자 산출물을 함께 배치해야 함.
- NewFolderPublisher.PublishAsync(ZipWorkerResult result, string destinationPath, CancellationToken token): 사용자가 선택한 부모 아래 새 폴더의 전체 절대 경로를 받음. 예를 들어 선택 부모가 C:\output이고 새 이름이 result이면 C:\output\result를 전달함. 이미 있는 파일/폴더와 병합하거나 덮어쓰지 않음. 원본 ZIP 안의 문자열을 이 목적지로 사용하면 안 됨.
- 반환형은 기존 ZipPublicationResult임. Published=true만 저장 완료이며 Error, CleanupFailed와 NativeFailure/CleanupNativeFailure를 분리해 표시함. CleanupFailed의 TemporaryFileName은 이번 작업이 만든 임시 폴더 이름임. 이름을 근거로 자동 재귀 삭제하면 안 됨.
- NewFolderPublisher.IsSupportedPlatform은 Windows/Linux x64에서 true임. 구현 진입 가능 여부이며 대상 파일시스템, 권한 조건, 검증 완료 또는 일반 UI 활성화를 의미하지 않음. API 호출 때 지원 조건을 다시 검사함.
- ZipPublicationResult에 선택적 PosixError/CleanupPosixError를 추가함. 각각 주 실패/정리 실패의 errno이며 경로를 포함하지 않음. 기존 Windows 진단 필드는 유지함.

## 형식과 무결성

초기 지원은 ZIP32 단일 디스크의 저장(method 0)/Deflate(method 8), ASCII 또는 UTF-8 이름, 일반 파일과 디렉터리임. 암호, 분할, ZIP64, SFX/앞부분 데이터, 미해석 추가 필드, 다른 creator OS, Deflate64 등은 실패 폐쇄함. 확장 필드는 링크 의미가 없는 0x5455 타임스탬프만 무시하고 나머지는 거부함. Unix/ASi hardlink 메타데이터도 따라서 거부함. 형식 확대나 암호/복구의 최종 미지원을 결정한 것이 아님.

작업자는 중앙/로컬 헤더의 이름, 방식, 플래그, CRC와 크기 및 데이터 디스크립터를 대조함. 레코드의 범위를 검사하고 중복/겹침/간격을 거부함. 외부 속성의 symlink, FIFO 및 기타 특수 타입, reparse 속성을 거부함. 일반 파일과 디렉터리 이름은 현재 OS 정책으로 검사하며 전체 목록에서 중복, 파일-폴더 조상 충돌을 검사한 후 해제함. Windows에서는 대소문자만 다른 이름도 충돌임. Linux에서는 합법적 콜론, 역슬래시, CON 같은 이름과 대소문자를 보존함. Linux에서 역슬래시는 경로 구분자가 아님.

.NET 10.0.0 ZipArchiveEntry의 GetDataDecompressor/OpenInReadModeGetDataCompressor 소스는 압축 범위와 헤더의 해제 크기를 디코더에 전달하고 CRC를 비교하지 않음. Crc32 속성은 기록된 필드이지 검증 결과가 아님. 따라서 해제에는 선언 길이 제한을 넘겨주지 않는 공개 DeflateStream을 사용하고 실제 EOF까지 읽어 길이와 CRC-32를 직접 비교함. 헤더 길이만 작게 바꾸고 CRC를 유지한 자료도 성공으로 처리하지 않음.

작업자 시작 때 System.IO.Compression.UseStrictValidation을 켜서 .NET 10 Deflate의 비어 있지 않은 미완성 입력을 거부함. 완전히 빈 Deflate 입력은 별도 거부함. 이 스위치의 동작은 아래 고정 버전 소스 근거이며 실제 런타임 업데이트 시 재검증해야 함. 검증 범위는 헤더 일관성, 디코딩한 실제 길이와 ZIP CRC임. CRC는 암호학적 진위 검증이 아니며 Deflate 내부의 모든 패딩/뒤따르는 비트의 정규형까지 검증한다고 주장하지 않음.

모든 항목이 통과할 때만 해제 목록을 전달함. 한 항목이라도 손상/예산/경로 검증에 실패하면 부분 성공이나 앞선 항목을 공개하지 않음. 호스트는 받은 메타데이터, 경로, CRC와 길이를 다시 확인하고 게시 시 바이트를 복사함. UI에서 ZIP 파싱이나 압축 해제를 수행하지 않음. 호출 중 DTO나 배열을 변경하지 않는 것이 호출자 계약임.

## 예산과 원본 보존

입력 ZIP 16 MiB, 실제 해제 바이트 누적 16 MiB, 프레임 32 MiB, 아카이브 항목 1024개, 경로 깊이 32, 암묵적 디렉터리 포함 노드 4096개를 넘으면 실패함. JobLimits의 더 작은 한도도 적용함. JobBudget은 입력 ZIP 크기와 실제 해제 읽기량을 MaxProcessedBytes에 합산하며 실제 출력마다 MaxOutputBytes를 누적함. 파일별 한도가 전체 한도를 대체하지 않음. 이 수치는 개발 한도이며 대용량/경량성 보장이 아님.

부모는 입력에 쓰기나 자르기를 하지 않고 작업자는 복사된 메모리 입력만 읽음. 사용자 원본 경로를 출력 대상으로 다시 열지 않음. 외부 프로세스의 원본 변경을 감시하는 기능은 없으므로 호출자는 읽기 전용 파일 핸들과 적절한 공유 모드를 사용해야 함. 원본 해시 불변의 실제 증명은 담당 테스트의 실행 결과로 별도 기록해야 함.

파싱/해제 중 취소와 시간 초과는 기존 클라이언트의 프로세스 종료 확인을 거침. 게시 단계는 제한된 메모리 결과만 쓰며 각 쓰기/읽기와 최종 게시 직전에 취소를 확인함. 네이티브 파일 I/O의 강제 시간 제한, Job Object/cgroup 격리와 네트워크 차단은 여전히 없음.

## 위협 범위와 쓰기 경계

악성 ZIP 경로/타입/링크, 기존 결과 충돌, 다른 계정의 경로 교체와 정상 동시 작업을 대상으로 함. 이미 실행 중인 같은 사용자 권한의 악성 프로그램, 관리자/SYSTEM/root, 커널 또는 파일시스템 자체의 침해는 방어한다고 주장하지 않음. 같은 사용자 프로그램은 이번 작업의 무작위 임시 트리에 들어가서 이름/내용/권한을 의도적으로 바꾸지 않아야 함. 정상 동시 게시 작업은 서로 다른 UUID 임시 이름을 쓰며 최종 이름 충돌은 OS의 덮어쓰기 금지 연산으로 결정함.

부모 핸들이 가리키는 디렉터리 객체를 선택 경계로 삼음. Linux에서 조상 경로가 이동해도 쓰기가 교체된 디렉터리로 전환되지 않음. 문자열 경로가 계속 같은 객체를 가리킨다는 보장은 없으며 외부 이동 후에는 사용자가 지정했던 텍스트 경로로 결과가 보이지 않을 수 있음. 기존 ACL/소유자/모드를 수정하거나 권한을 올리지 않음.

## Windows 게시

드라이브 루트부터 부모까지 reparse를 거부하고 삭제 공유 없는 핸들로 고정함. 로컬 NTFS만 허용함. 임시 루트와 각 하위 노드는 보유 부모 핸들 상대 FILE_CREATE로 새로 생성하고 파일 ID, 타입, 링크 수, 소유자와 DACL을 읽음. 쓰기 가능한 SID는 현재 사용자, SYSTEM, Administrators만 허용함. 선택 부모도 생성 전 같은 ACL 검사를 통과해야 하며 소유자도 같은 집합에 속해야 함. NULL DACL, 해석하지 않는 ACE/조건부 ACE, 타 계정의 쓰기 허용은 실패 폐쇄함. 읽기 전용 ACE와 상속 전용 CREATOR OWNER는 허용하되 실제 생성된 각 노드를 다시 검사함. 부모에서 상속된 ACL을 변경하지 않으므로 공유 쓰기 폴더에서는 거부될 수 있음.

파일은 배타적 핸들에 쓴 뒤 flush, 길이와 전체 바이트 읽기 비교를 수행함. 최종 검증은 모든 핸들의 ID/ACL/타입과 각 디렉터리의 실제 자식 이름 집합을 대조함. 알 수 없는 항목이 있으면 게시하지 않음. 이름 열거만으로 원자적 보안을 보장하는 것이 아니라 외부 계정의 쓰기를 배제한 ACL 조건과 함께 사용함.

NTFS는 열린 자식 파일이 있는 디렉터리 rename을 제한하므로 검증 후 자식 핸들을 닫고 루트 핸들만 유지함. 루트를 NtSetInformationFile(FileRenameInformation)로 동일 부모 아래 새 이름에 게시하며 ReplaceIfExists=false를 유지함. 실패 시 닫힌 자식은 보유 루트부터 상대적으로 다시 열고 저장된 파일 ID와 비교한 후에만 핸들 삭제를 수행함. 교체된 항목이나 모르는 항목은 삭제하지 않으며 정리 실패를 별도로 반환함. 기존 노드 병합, 경로 기반 재귀 삭제는 없음.

## Linux 게시

openat2(RESOLVE_NO_SYMLINKS)로 부모를 열고 fstatfs의 ext 공통 magic(0xEF53) 또는 XFS magic(0x58465342)을 확인함. magic만으로 ext4를 확정하지 않음. 같은 부모 fd에 statx(AT_EMPTY_PATH, STATX_BASIC_STATS|STATX_MNT_ID)를 호출해 반환 mask, mount ID와 장치 major:minor를 얻음. STATX_MNT_ID_UNIQUE는 mountinfo의 ID와 다르므로 요청하지 않음.

/proc를 링크 없이 열어 fstatfs의 PROC_SUPER_MAGIC(0x9FA0)을 확인하고, 현재 getpid()/gettid()의 task mountinfo를 보유 proc fd 상대 openat2로 열음. RESOLVE_BENEATH|NO_SYMLINKS|NO_MAGICLINKS|NO_XDEV로 하위 링크와 bind mount를 거부하고 정보 fd도 procfs인지 다시 확인함. 외부 명령이나 사용자 지정 정보 파일은 사용하지 않음. 현재 프로세스의 정상 procfs와 PID/mount namespace를 전제로 하며 관리자에 의한 namespace/procfs 조작 방어로 확대하지 않음.

mountinfo를 최대 4 MiB까지 읽고 mount ID에 해당하는 정확히 한 레코드의 장치 번호와 파일시스템 유형을 대조함. ext magic은 유형이 정확히 ext4, XFS magic은 정확히 xfs여야 함. 부모 fd를 계속 보유하며 조회 전후 mount ID, 장치/inode와 magic도 대조함. 경로 접두사나 mount point 문자열은 판별에 쓰지 않음. 분리된 마운트의 레코드 누락, 중복, 불일치, procfs 접근 실패, STATX_MNT_ID 미지원과 크기 한도 초과는 모두 임시 생성 전 OutputUnavailable임. 열린 fd의 마운트 참조가 유지되는 동안 일반 mount ID 재사용을 막는 Linux 수명 계약에 의존함.

여기서 ext4는 커널이 보고하는 마운트 파일시스템 유형임. 디스크를 직접 읽어 포맷 생성 이력이나 ext 기능 비트를 판별한 것은 아님. 유형 ext2/ext3를 허용하지 않으며, ext4 드라이버의 구형 디스크 호환 모드를 별도 검증했다고 주장하지 않음. 조건을 만족하면 기존 핸들 기준 쓰기/검증/게시/정리 경로를 사용하며, 파일시스템 연산 실패를 성공으로 바꾸지 않음.

부모 소유자가 현재 euid이고 그룹/기타 쓰기가 없는 경우만 허용함. 다른 계정이 부모 항목을 교체할 수 있는 공용 디렉터리는 거부함. 새 임시 디렉터리는 mkdirat 0700, 파일은 openat2 O_CREAT|O_EXCL 0600으로 생성함. umask는 권한을 줄일 수 있지만 넓히지 않음. 이후 작업은 보유 디렉터리 fd 상대, RESOLVE_BENEATH|NO_SYMLINKS|NO_MAGICLINKS|NO_XDEV로 수행함.

각 노드의 statx 장치/ inode, 타입, uid/모드와 일반 파일 링크 수 1을 확인함. 쓰기/flush/전체 바이트 재읽기 뒤 getdents64 이름 집합을 비교하고 디렉터리 fsync 후 renameat2(RENAME_NOREPLACE)로 게시함. 루트 이름과 보유 inode의 일치를 게시 직전에 확인함. 파일시스템이 해당 연산을 지원하지 않으면 우회하지 않음.

정리는 소유 노드 목록의 역순으로 수행함. 부모 핸들, 이름과 inode를 다시 비교한 뒤 unlinkat하며 디렉터리는 AT_REMOVEDIR로 빈 경우만 제거함. compare/unlink 자체가 원자적이라는 주장은 하지 않음. 타 계정 쓰기가 배제된 부모와 0700 임시 트리가 안전 조건이며 같은 uid가 의도적으로 임시 트리를 바꾸는 상황은 범위 밖임. mkdirat 후 fd 확보에 실패하면 소유 inode가 확정되지 않으므로 임의로 이름을 삭제하지 않고 CleanupFailed를 반환함.

## 실제 검증과 남은 조건

이번 변경은 소스 검토 단계임. 실제 C# 컴파일, Windows NTFS에서 자식 핸들 종료 후 rename과 충돌 뒤 재열기 정리, ACL 거부, 링크/경합/취소, Ubuntu 각 LTS ext4/XFS 게시와 ext2/ext3 유형 거부 실행은 미검증임. 로컬 tests/security와 tests/runtime을 읽기 전용으로 연 핸들에서는 mount ID 33, 장치 259:1, magic 0xEF53과 mountinfo의 ext4가 일치함을 확인했음. 이 관찰은 C# 판별/게시 실행의 통과 증거가 아님. procfs 또는 mount ID를 확인할 수 없는 환경에서는 기본 ext4도 차단되며, 해당 환경의 procfs/namespace 제공 조건을 검토해야 함. 권한 변경이나 마운트 생성으로 우회하지 않음. 테스트 담당자가 기존 실패 폐쇄 상태 검사와 차단된 8개 게시 사례를 새 조건에 맞춰 연결해야 함. 공개 capability나 일반 UI 해제를 활성화하지 않음.

강제 종료/전원 손실 시 임시 트리가 남을 수 있음. 게시 후 부모 디렉터리 fsync나 전원 장애 복구 트랜잭션까지 보장하지 않음. 실패 정리에서 모르는 노드, 변경된 ID 또는 OS 오류가 발견되면 기존 자료 보존을 우선해 잔존을 보고함. 네이티브 I/O의 강제 시간 제한과 실제 경합 검증도 남아 있음.

공식 근거:

- [.NET 10.0.0 ZipArchiveEntry 소스](https://raw.githubusercontent.com/dotnet/runtime/v10.0.0/src/libraries/System.IO.Compression/src/System/IO/Compression/ZipArchiveEntry.cs)
- [.NET 10.0.0 DeflateStream 소스](https://raw.githubusercontent.com/dotnet/runtime/v10.0.0/src/libraries/System.IO.Compression/src/System/IO/Compression/DeflateZLib/DeflateStream.cs)
- [ZIP Crc32 속성](https://learn.microsoft.com/en-us/dotnet/api/system.io.compression.ziparchiveentry.crc32?view=net-10.0)
- [Native rename의 부모 핸들과 덮어쓰기 정책](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information)
- [핸들 보안 설명자 읽기](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-getsecurityinfo)
- [Linux openat2 경계](https://man7.org/linux/man-pages/man2/openat2.2.html)
- [Linux statx의 fd 조회와 mount ID](https://man7.org/linux/man-pages/man2/statx.2.html)
- [Linux mountinfo의 ID, 장치와 파일시스템 유형](https://man7.org/linux/man-pages/man5/proc_pid_mountinfo.5.html)
- [Linux 6.8 마운트 참조와 ID 해제 구현](https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/namespace.c)
- [Linux 6.8 커널 mountinfo 생성 구현](https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/proc_namespace.c)
- [Linux statfs의 ext 공통 식별값과 XFS 식별값](https://man7.org/linux/man-pages/man2/statfs.2.html)
- [Linux renameat2 덮어쓰기 금지](https://man7.org/linux/man-pages/man2/rename.2.html)

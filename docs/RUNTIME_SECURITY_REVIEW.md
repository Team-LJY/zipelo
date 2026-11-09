# Runtime/Worker와 OS별 회귀 검토

## 실행 증거와 범위

2026-11-09 UTC 기준 담당 테스트와 검토 문서의 기록임. Core/Runtime/Worker와 UI 구현은 읽기 검토만 수행함. 현재 C#은 Core 41개, 기존 Runtime 21개, 메모리 해제 26개, 폴더 게시 10개로 총 98개 등록 상태임. 로컬 .NET SDK가 없어 최신 C# 컴파일과 실행은 미실행임.

- 로컬 Ubuntu 26.04.1 LTS x86_64에서 `python3 -B tests/run_checks.py`의 샘플/정적/실행기 검사 33개 통과 기록이 있음. 실제 C# 작업자나 네이티브 게시의 통과 근거가 아님.
- 총괄의 Windows round5 보고에서 이전 C# 62개가 통과함. 새 ZIP 파일 내용, 기존 파일/입력 충돌 보존과 부모/최종 symlink 거부에 관한 결과임.
- round6은 `2026-11-09T14:44:04.604469Z`의 src/tests 102개 파일 고정 스냅샷임. 총괄 보고상 Desktop/Core/Runtime/Worker 빌드 경고/오류 0, Core41 통과, Runtime56은 CS0266 컴파일 실패로 전부 미실행임. 당시 폴더 게이트는 전체 실패 폐쇄였음.
- CS0266은 `ExtractionPublicationTests.cs`의 `DirectoryInfo info;`를 `FileSystemInfo info;`로 한 줄 수정하여 별도 전달함. 단독 패치 SHA-256은 `3e2eab3a0a61de4aa967f79a87240e5eae067e81cbe5708575a8f59524049ab5`임. 현재 파일은 후속 플랫폼 계약 변경까지 포함하므로 이 단독 패치와 구분해야 함.
- 현재 98개 전체와 Windows 새 폴더 게시, Ubuntu 각 LTS의 C# 실행 결과는 아직 없음. 등록 수, 소스 검토, 과거 스냅샷 통과와 현재 실행을 합산하지 않음.

[플랫폼 행렬](../tests/security/platform_matrix.json), [해제 수용 범위](../tests/extraction/coverage.json)와 [폴더 소스 검토](../tests/extraction/folder_publication_review.json)에 보고 출처, 검토 소스 해시 및 미실행 조건을 남김.

## 새 폴더 플랫폼 계약

`NewFolderPublisher.IsSupportedPlatform`은 Windows/Linux x64의 구현 진입 여부임. 일반 UI의 `SupportsSafeExtraction=false`나 실제 저장 환경의 지원 판정과 별개로 검사함. Windows는 로컬 NTFS 및 현재 사용자/SYSTEM/Administrators 쓰기 경계, Linux는 부모 fd의 mount ID/장치 번호와 검증된 procfs mountinfo의 ext4 또는 xfs 유형 일치, 현재 euid 소유, 그룹/기타 쓰기 금지를 요구함. Linux의 openat2/statx/renameat2와 procfs 확인 실패는 실패 폐쇄임.

ext2/ext3/ext4가 공유하는 `0xEF53`만으로 ext4를 판별할 수 없다는 검토를 backend에 전달함. 이후 XFS만 허용한 임시 수정이 있었고, 최신 구현은 부모 fd의 mount ID/장치/유형 대조를 추가함. ext2/ext3 유형과 일치하지 않는 마운트 정보는 임시 폴더 생성 전에 거부함. 이는 소스 변경 확인이며 실제 마운트별 실행 증거가 아님. [statfs 식별값](https://man7.org/linux/man-pages/man2/statfs.2.html)

[FolderEnvironment](../tests/runtime/FolderEnvironment.cs)는 테스트가 새로 만든 부모의 저장 조건을 제품 API와 별도로 조회함. Windows는 관리형 ACL 조회와 드라이브 정보를 사용하고, Linux는 statfs/statx와 현재 스레드 mountinfo를 대조함. 조회된 조건이 지원 범위라면 `folder.platform_contract`도 실제 게시 성공과 트리/파일 해시를 요구함. 알려진 미지원 조건이면 새 목적지, 기존 원본 파일과 기존 폴더를 대상으로 정확한 실패 오류, 바이트 보존 및 임시 항목 부재를 요구함. [관리형 ACL 조회 계약](https://learn.microsoft.com/en-us/dotnet/api/system.io.filesystemaclextensions.getaccesscontrol?view=net-10.0), [드라이브 정보](https://learn.microsoft.com/en-us/dotnet/api/system.io.driveinfo?view=net-10.0)

저장 전제조건이 미충족인 환경에서 새 폴더 성공, 충돌, 최종 링크 및 동시 게시 수용 검사는 SKIP으로 기록하며 PASS에 합산하지 않음. 제품 `PublishAsync`가 실패했다는 이유로 SKIP하지 않음. 예상 지원 조건에서 게시가 실패하면 FAIL임. 사전 취소, 변조 내용, 실패한 해제 결과와 부모 symlink 거부는 저장 파일시스템 미지원과 분리함. 심볼릭 링크 생성 권한 부족도 해당 사례의 SKIP으로 남김.

테스트 환경 관찰은 보안 경계를 구현하는 코드가 아님. 제품의 부모 핸들 고정, procfs 대체 방어, inode/파일 ID 재검증, 네이티브 원자성이나 경합 안전성을 이 관찰만으로 증명하지 않음. 권한 변경과 새 마운트 없이 관찰하지 못한 부정 조건은 미검증임. 한 번의 플랫폼 계약 통과를 모든 파일시스템/ACL 분기 통과로 표시하면 안 됨.

## 보존, 무결성과 취소

기존 Core 및 단일 ZIP 게시 62개의 성공/오류 기대값을 변경하지 않음. 비정상 응답 상태와 `Completed + Error`는 `EngineFailure` 및 생성물 폐기를 요구하며 backend 수정이 반영된 소스를 확인함. 이 두 사례의 이전 Windows 통과 보고는 해당 고정 스냅샷에 한정함.

해제는 자체 생성한 작은 ZIP 22개로 정상 Stored/Deflate 내용과 SHA-256, 경로 이탈/절대 경로, 파일-폴더 충돌 양방향, OS 이름/대소문자, 링크 타입/메타데이터, CRC/길이 손상, 누적 항목/처리/출력 예산을 검사함. 제품은 별도 작업자에서 실제 디코딩 길이와 CRC를 직접 확인함. `.NET`이 CRC를 자동 검증한다고 가정하지 않음. 메모리 해제 완료를 디스크 게시 완료로 표시하지 않음.

폴더 수용 검사는 새 트리 전체 항목과 파일 바이트, 원본과 기존 대상의 해시/표식, 기존 링크와 링크 대상 보존, 잔류 임시 이름 및 `CleanupFailed`를 각각 검사함. 실패 오류를 `OutputUnavailable` 등 여러 값 중 아무거나 허용하도록 바꾸지 않음. 폴더 쓰기 전에 이미 취소된 요청과 입력 복사 중 결정적 취소를 실제 쓰기 중 취소의 증거로 사용하지 않음.

[동시 게시 검사](../tests/runtime/FolderConcurrencyTests.cs)는 서로 다른 내용의 두 요청을 같은 최종 이름으로 시작함. 정확히 한 성공과 한 `OutputConflict`, 패자 임시 정리, 승자 트리/해시, 원본 및 출력 부모 밖 표식 보존을 요구함. 시작 장벽은 있으나 네이티브 rename 호출의 정확한 겹침은 강제하지 않음. 아직 실제 실행하지 않았음.

## 남은 위험과 검사

- Windows 자식 핸들 종료 후 rename 및 충돌 후 재개방/파일 ID 검사와 삭제, Linux 이름 확인 후 unlinkat는 실제 검증이 필요함. 실행 사용자 전체나 관리자 계정이 장악된 상황까지 방어한다고 주장하지 않음.
- Linux mkdir 성공 후 핸들 획득 실패는 `unboundCreation`과 정리 실패 보고로 연결된 소스를 확인함. 결정적 실패 주입과 실제 잔존 이름/외부 표식 보존은 미검증임.
- 실제 경로 교체, junction/hardlink, 외부 항목 삽입, 마운트/namespace/procfs 변경, 쓰기 중 취소, 늦은 취소, 정리 실패, 강제 종료와 전원 손실은 별도 검증이 남음.
- 입력 16 MiB, 프레임 32 MiB와 항목/출력 예산은 프로세스 메모리 상한이 아님. ZIP 중앙 디렉터리 할당, JSON/Base64/배열 복사, 메타데이터 파싱 비용과 동기 네이티브 I/O의 강제 종료 한계가 남음. Windows Job Object/Linux 자원 제한과 대용량 지원을 검증했다고 표시하지 않음.
- `WorkerPlatform`의 앱 기준 절대 경로는 배포 폴더 신뢰를 전제로 함. 실행 파일 무결성, 설치 패키지, JAR 제외 및 동의 없는 기본 연결 변경 금지는 실제 설치 환경에서 별도 검증이 필요함.

## 재실행과 브랜치 범위

승인된 .NET 10 SDK 환경에서 `python3 -B tests/run_checks.py --core --runtime`로 실행함. 테스트 코드, 대역, 샘플과 빌드/실행 임시 자료는 루트 tests/ 아래에 둠. `tests/security/.work`와 bin/obj, 로그, 내부 잠금 및 개인 설정은 소스 브랜치에서 제외함. 파일시스템과 환경 관찰, PASS/FAIL/SKIP 수 및 소스 해시를 함께 남겨야 함.

`features/archive-security-tests`에는 tests/와 이 문서 및 SECURITY_REVIEW만 포함함. Core/Runtime/Worker 소스가 있는 별도 기능 브랜치와 UI 정적 검사의 UI 소스가 통합되어야 전체 검사를 실행할 수 있음. C# 미실행 상태를 draft PR에 명시하며 일반 해제 활성화나 main 병합은 이 변경의 범위가 아님.

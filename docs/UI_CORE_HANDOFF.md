# Desktop과 Core 연결 계획

1~10장은 2026-11-09 초기 연결 전 소스의 정적 검토 기록이다. 이후 구현 상태는 11장을 기준으로 확인한다. 초기 기록의 행 번호와 미구현 표현은 당시 스냅샷에 해당한다. 이번 변경은 이 문서에 한정하며 실제 연결, 빌드, 테스트 실행이나 Windows 동작 검증을 의미하지 않는다. 저장소는 Team-LJY/zipelo, 확인 브랜치는 docs/initial-project-plan이다. 공유 작업 지침에 따라 브랜치를 전환하지 않는다.

## 1. 현재 연결 상태와 근거

- [Desktop 시작점](../src/Zipelo.Desktop/App.xaml.cs) 10행은 UnavailableWorkspace를 사용한다. [UI 계약](../src/Zipelo.Desktop/WorkspacePort.cs) 37행의 IArchiveWorkspace를 구현하는 실제 어댑터는 없다.
- [Desktop 프로젝트](../src/Zipelo.Desktop/Zipelo.Desktop.csproj)는 net10.0-windows WPF이며 Core 프로젝트 참조가 없다. [Core 프로젝트](../src/Zipelo.Core/Zipelo.Core.csproj)는 net10.0이다. 대상 프레임워크 방향은 연결 가능하지만 컴파일로 확인한 결과는 아니다.
- [Core 작업](../src/Zipelo.Core/ArchiveJob.cs) 17행은 작업 종류, 형식, 엔진, 읽기 전용 입력 스트림, 출력 트랜잭션, 암호 제공자와 진행 관찰자를 요구한다. UI의 경로 기반 요청을 그대로 전달할 수 없다.
- [Core 계약](../src/Zipelo.Core/ArchiveContracts.cs) 54행과 71행에는 미연결 엔진과 출력 구현만 있다. 엔진이나 저장 계층을 UI 코드에 임시로 구현해 연결된 것처럼 표시하지 않는다.

## 2. 타입과 데이터 변환

### 요청과 기능 조회

WorkspaceAction의 List, Extract, Create를 ArchiveAction의 동명 값으로 명시적으로 변환한다. 숫자 캐스팅에 의존하지 않는다. Verify, Preview, Recover는 UI 요청이 없으므로 노출하지 않는다. WorkspaceRequest.JobId로 ArchiveJob을 매번 새로 생성하며 인스턴스를 재사용하지 않는다.

WorkspaceRequest.Sources는 경로 목록이다. 호스트가 읽기 전용 입력 핸들을 열고 작업 반환까지 수명을 소유해야 한다. 파일 선택 이후 경로가 교체되거나 파일이 변경될 수 있으므로 목록을 읽은 파일과 해제할 파일의 동일성 및 재열람 정책도 필요하다. 읽기 전용 스트림이라는 사실만으로 다른 프로세스에 의한 변경이 막히지는 않는다.

WorkspaceRequest에는 format, JobLimits, 선택 항목, 충돌 정책, 압축 수준과 생성 암호 설정이 없다. 형식 식별과 JAR 제외 정책을 호스트와 코어에서 일관되게 적용하고 실제 형식에 대한 기능을 조회해야 한다. IsAvailable 하나로 모든 버튼을 켜면 목록 전용 엔진에서도 생성이 가능해 보인다. 형식별 EngineCapability를 UI에 전달하는 조회 계약이 필요하다.

특히 생성에는 원본의 상대 이름과 폴더 구조를 보존하는 입력 명세가 필요하다. IArchiveEngine은 이름 없는 스트림 목록만 받으므로 안전한 이름과 스트림을 묶는 계약 또는 별도 검증된 생성 문맥을 백엔드와 합의해야 한다. 엔진에 원본 경로를 넘기는 우회로 해결하지 않는다.

Destination은 해제에서는 폴더, 생성에서는 파일이다. IOutputTransaction은 상대 경로 조각으로 파일을 생성하므로 생성 결과 ZIP 한 개와 그 내부 항목 이름을 구분해야 한다. 원본과 같은 파일, 기존 결과, 중복 이름과 링크를 실제 출력 계층에서 차단해야 한다. SaveFileDialog의 덮어쓰기 확인은 저장 계층 정책을 대체하지 않는다.

### 결과와 항목

FileRow는 이름, 크기, 검증 상태를 모두 문자열로 받으며 항목 ID와 종류가 없다. Core의 ArchiveEntry.Id, Kind, DeclaredSize와 EntryResult.Outcome, Verification, Error를 잃지 않는 화면 모델을 먼저 정의한다. 크기 null은 '알 수 없음'으로 표시하고 0바이트로 바꾸지 않는다. Listed는 목록 확인, Extracted는 게시된 출력으로 구분한다. NotChecked와 Unavailable은 정상 검증으로 번역하지 않는다.

WorkspaceResult에는 JobId와 오류 코드가 없다. JobResult.JobId를 활성 작업과 대조하고 Completed, Partial, Cancelled, Failed를 명시적으로 매핑한다. Created, Running, WaitingForPassword, Cancelling은 최종 성공 상태로 취급하지 않는다. 현재 ArchiveJob은 Partial 값을 실제 결과로 만드는 경로가 없으므로 일부 항목이 있다는 이유로 어댑터가 임의로 Partial 성공을 만들면 안 된다.

## 3. 상태와 진행 이벤트

JobEvent는 JobId, Sequence, State, ProcessedBytes를 제공하지만 WorkspaceProgress는 바이트와 TotalBytes만 가진다. 현재 Core에는 전체 크기와 항목 추가 이벤트가 없으며 목록 항목은 최종 JobResult에서 얻는다. 점진적 목록 표시가 이미 구현됐다고 안내하지 않는다.

제안하는 UI 수신 규칙은 다음과 같다.

1. 활성 작업 ID와 일치하는 이벤트만 받는다.
2. 작업별 마지막 Sequence 이하인 이벤트는 버린다. 서로 다른 스레드에서 보고될 수 있으므로 도착 순서가 곧 순번 순서라고 가정하지 않는다.
3. Running, WaitingForPassword, Cancelling을 별도 상태로 반영한다. 암호창이 열려 있거나 취소 중이면 일반 바이트 이벤트가 안내를 덮어쓰지 않게 한다.
4. Core의 ProcessedBytes는 누적 작업량이다. 압축파일 크기, 목록의 DeclaredSize 합계 또는 출력 크기로 임의 나누어 백분율을 만들지 않는다. 신뢰할 전체 작업량이 없으면 불확정 진행 표시와 처리량만 제공한다.
5. 종료 이벤트만 보고 다음 작업을 허용하지 않는다. 작업 Task가 반환하고 호스트 입력과 암호창까지 정리된 뒤 최종 결과를 반영한다. 이후 늦게 도착한 이벤트는 버린다.

## 4. 암호 요청과 취소

Core PasswordChallenge에는 Attempt가 추가되어 있고 PasswordProvider는 ValueTask를 반환한다. UI의 Task 콜백을 await하여 변환하고 작업 ID, 요청 ID와 시도 횟수를 보존한다. 두 네임스페이스의 PasswordChallenge 이름이 같으므로 어댑터에서 별칭 또는 전체 이름을 사용한다.

[UI 암호 처리](../src/Zipelo.Desktop/MainWindow.xaml.cs) 199행은 작업 ID만 검사하며 RequestId를 소비하지 않는다. 요청 ID별 한 번의 응답, 중복 요청 처리와 오래된 창의 응답 폐기를 추가해야 한다. 다른 작업이나 중복 요청에 단순 null을 반환하면 Core가 실제 사용자 취소와 구분하지 못하므로 요청 연결 오류와 사용자 취소의 구분도 합의한다.

[PasswordWindow](../src/Zipelo.Desktop/PasswordWindow.xaml.cs) 19행은 SecurePassword를 얻고 입력칸을 지운다. [EngineContext](../src/Zipelo.Core/EngineContext.cs) 154행은 전달받은 SecureString을 소비 콜백이 끝나면 폐기한다. UI는 성공적으로 소유권을 넘긴 암호를 동시에 폐기하면 안 되고, 인계 실패나 취소로 남은 사본만 폐기한다. 평문 문자열, 로그, 인자, 환경변수나 영구 설정으로 변환하지 않는다.

사용자 취소는 null 응답과 작업 취소로 연결하며 암호 오류로 표시하지 않는다. Core의 내부 연결 토큰과 UI의 외부 토큰은 다르므로 최종 JobState.Cancelled도 반드시 해석한다. 엔진이 지원하지 않는 암호화 방식에는 반복 암호창을 띄우지 않는다. 현재 오류 코드에는 확정된 오입력과 암호화 방식 미지원의 상세 구분이 부족하므로 추측 안내를 피한다.

Core의 MaxDuration은 암호 대기 시간도 포함한다. 대기 시간을 제한에 포함할지, 별도 사용자 대기 예산을 둘지는 결정이 필요하며 어댑터에서 작업 예산을 새로 만들어 우회하지 않는다.

## 5. 오류, 종료와 UI 복구

[MainWindow](../src/Zipelo.Desktop/MainWindow.xaml.cs) 160행은 성공 여부만 표시하고 오류 코드는 전달받지 못한다. EngineUnavailable, UnsupportedOperation, OutputConflict, BudgetExceeded, IntegrityFailure, OutputUnavailable을 허용된 코드 기반 문구와 다음 행동으로 변환하는 계약이 필요하다. 예외 원문과 민감한 경로를 노출하지 않는 현재 원칙은 유지한다.

같은 파일의 다른 위치 저장, 암호 재입력과 단순 재시도는 서로 다르므로 상태별로 행동을 선택한다. 암호와 손상을 구분할 근거가 없으면 한쪽으로 단정하지 않는다. 생성과 해제 결과의 항목별 실패는 현재 목록 화면에 반영되지 않으므로 별도 결과 화면 또는 목록 갱신이 필요하다.

RunJob 154행은 취소 토큰이 설정됐으면 반환 결과 전체를 버린다. Core는 CommitAsync 성공 이후 늦은 취소가 와도 실제 게시가 완료됐을 수 있다. UI는 토큰만 보고 완료된 출력을 없었던 것으로 처리하지 말고 실제 JobResult와 게시 결과를 보존해야 한다. 완료, 취소, 정리 실패를 구분할 출력 게시 상태 계약을 합의한다.

RunJob의 finally 188행은 UI를 다시 활성화하고 ArchiveJob은 출력 DisposeAsync 이후 반환한다. 호스트는 입력 스트림까지 정리한 뒤 반환해야 한다. 어댑터가 중단 요청만 보내고 조기에 반환하면 UI가 새 작업을 시작할 수 있으므로 금지한다. 현재 별도 작업 프로세스와 강제 종료는 없어 비협조적인 엔진의 종료 시간을 보장할 수 없다.

OnClosing 272행은 취소를 요청하고 종료를 보류한다. 정상 정리 후 다시 닫을 수 있어야 하며 정리 실패가 나도 영구적으로 Busy에 갇히지 않아야 한다. 임의 시간 뒤 성공으로 종료하지 않는다. 실패 시 100% 또는 이전 파일의 완료 상태가 남지 않도록 상태와 진행 값을 함께 복구한다.

## 6. 중복 클릭과 새 파일 열기 경합

RunJob 112행의 Busy 검사와 UpdateActions 250행의 버튼 비활성화는 있다. 다만 파일 대화상자는 Busy 상태를 예약하기 전에 열리고 OpenArchive 46행은 RunJob이 요청을 거부하기 전에 기존 목록과 archivePath를 비운다. 정상 클릭에서 실제 재현됐다는 뜻은 아니지만 재진입, 자동화와 향후 끌어놓기 경로를 고려한 검사가 필요하다.

제안은 Idle → Selecting → Running → WaitingForPassword 또는 Cancelling → Finishing → Idle 순서의 단일 UI 작업 게이트다. 대화상자 진입 전에 게이트를 얻고 모든 핸들러가 이를 공유한다. 대화상자 취소는 기존 문서와 상태를 유지한다. 성공적으로 새 파일 열기를 수락한 뒤에만 목록을 교체한다.

RunJob은 finally에서 버튼을 켜고 OpenArchive의 await 이후 코드가 목록을 게시한다. 최종 목록 게시까지 같은 작업 세대와 게이트를 유지하고 오래된 결과가 새 파일 제목이나 목록을 덮어쓰지 못하게 한다. 활성 작업 종료 전 새 파일 요청은 명시적으로 거부하고 병렬 작업으로 몰래 전환하지 않는다.

## 7. 엔진 결정 전 가능한 연결 작업

아래는 후속 구현 제안이며 이번 라운드에서 소스와 테스트를 변경하지 않았다.

- Desktop 측 어댑터, Core 프로젝트 참조, 타입 변환, 코드 기반 오류 문구, 이벤트 순번과 종료 처리, 암호 소유권 및 UI 게이트를 합의한 범위에서 구현할 수 있다.
- 미연결 엔진과 출력은 그대로 명시적으로 실패시킨다. 테스트 전용 가짜 엔진은 tests/ 안에서만 사용하고 제품 시작점에 연결하지 않는다.
- 경로를 입력 스트림과 안전한 출력 트랜잭션으로 바꾸는 호스트 계층, 실제 형식 식별과 이름 명세는 백엔드 소유다. 공용 계약과 프로젝트 참조 변경은 양쪽 범위를 확인한 뒤 진행한다.
- 실제 엔진이 있어야 형식별 목록, 해제, 생성, 암호, 분할, 솔리드와 검증 동작을 확인할 수 있다. Windows 출력 저장 계층과 실행 환경이 있어야 junction, 경로 교체와 취소 시 출력 정리를 검증할 수 있다.

## 8. .NET 기본 ZIP을 이용한 첫 실제 흐름 제안

.NET 기본 ZIP은 주 엔진 선정과 별개인 제한된 첫 흐름 후보다. 일반 비암호화 단일 ZIP의 목록, 파일 선택 ZIP 생성과 해제를 우선 대상으로 삼을 수 있다. ZipArchive.Entries, CreateEntry와 항목 스트림을 사용하는 API가 제공된다. [Microsoft ZIP 안내](https://learn.microsoft.com/en-us/dotnet/standard/io/how-to-compress-and-extract-files), [ZipArchive API](https://learn.microsoft.com/en-us/dotnet/api/system.io.compression.ziparchive?view=net-10.0)

암호 설정이나 ZIP 암호 해제 API를 제공하는 후보가 아니다. .NET 10.0.0 소스에는 IsEncrypted 속성이 있으나 그것이 복호화 기능을 뜻하지 않는다. 암호화 항목과 미지원 압축 방식은 별도 상태로 안내하고 암호 입력을 반복하지 않는다. 분할 및 여러 디스크 ZIP도 초기 후보에서 지원한다고 표시하지 않는다. 해당 버전 소스는 분할 구조를 거부하는 분기를 포함한다. 이는 Zipelo 전체 요구에서 암호와 분할을 철회하는 결정이 아니다. [v10.0.0 항목 구현](https://raw.githubusercontent.com/dotnet/runtime/v10.0.0/src/libraries/System.IO.Compression/src/System/IO/Compression/ZipArchiveEntry.cs), [v10.0.0 아카이브 구현](https://raw.githubusercontent.com/dotnet/runtime/v10.0.0/src/libraries/System.IO.Compression/src/System/IO/Compression/ZipArchive.cs)

목록 읽기 성공을 내용 검증으로 표시하지 않는다. 해제한 데이터의 CRC와 실제 길이 대조가 확인되지 않았으면 VerificationState.Passed로 표시하지 않는다. ZIP의 메타데이터 로딩과 내부 버퍼 사용은 Core 출력 바이트 제한만으로 모두 통제되지 않으므로 별도 메모리 및 입력 크기 한도도 검토한다. ZIP 복구와 RAR, 7Z 처리는 이 후보만으로 충족하지 않는다.

ZipFile.ExtractToDirectory를 UI에서 직접 호출하면 Core의 누적 예산과 출력 트랜잭션을 건너뛴다. 공식 문서에도 기존 목적지나 상위 경로의 junction 및 심볼릭 링크를 따라갈 수 있고 실패 시 일부 출력이 남을 수 있다고 명시돼 있다. 항목별 읽기와 Core 문맥, 실제 안전한 저장 계층을 거쳐야 한다. 문자열 경로 검사만으로 실제 출력 안전성이 확보됐다고 판단하지 않는다. [Microsoft 해제 API 주의사항](https://learn.microsoft.com/en-us/dotnet/api/system.io.compression.zipfile.extracttodirectory?view=net-10.0)

첫 흐름을 만들 경우 순서는 안전한 저장 계층과 생성 입력 이름 계약 합의 → 비암호화 ZIP 어댑터 → UI 연결 → 결과 해시와 원본 불변 확인 → Windows 실기 검증이다. 생성 시 원본을 Update 모드로 열지 않고 새 작업 소유 임시 ZIP을 완성한 뒤 게시한다. ZIP 내부 원본 이름과 외부 결과 ZIP 이름을 구분하고 임시 파일에도 누적 예산을 적용한다. 압축 중 임시 데이터 저장과 최종 ZIP 파일 쓰기를 어떤 누적 항목으로 과금할지는 계약으로 정한다.

## 9. 후속 수용 기준과 남은 결정

후속 테스트 코드는 전부 루트 tests/ 아래에 둔다. 이번에는 테스트 파일을 추가하거나 실행하지 않았다.

- 같은 작업의 순서가 뒤바뀐 이벤트와 종료 후 늦은 이벤트가 화면을 되돌리지 않아야 한다.
- 빠른 중복 클릭과 새 파일 열기 재진입에서 활성 작업은 하나이며 기존 화면이 임의로 비워지지 않아야 한다.
- 암호 취소, 외부 취소, 오래된 요청 응답과 인계 실패에서 암호와 입력 수명이 끝나고 화면이 복구되어야 한다.
- 게시 직전 취소, 게시 직후 취소와 출력 정리 실패에서 실제 결과가 UI 안내와 일치해야 한다.
- 파일 열기 실패 후 재시도, 생성 및 해제 실패 후 다른 작업, 창 닫기 취소에서 Busy가 해제되어야 한다.
- 첫 ZIP 후보는 정상 입력의 결과 해시 일치, 원본 불변과 비암호화 기능을 실제 검증해야 한다. 암호 및 분할 파일은 제한을 정확히 안내해야 한다.

다음 결정은 UI와 Core 공용 데이터 형태, 생성 입력 명세와 출력 게시 상태, 기능 조회 범위, 암호 대기 예산, 실제 저장 계층 및 제한된 기본 ZIP 후보의 도입 여부다. 이 문서는 연결 구현이나 엔진 채택을 확정하지 않는다.

## 10. 실제 확인과 미실행

실제로 수행한 것은 로컬 루트, origin과 Git 상태 확인, Desktop과 Core 소스 열람, 공식 .NET API와 v10.0.0 소스 대조, 이 문서의 로컬 링크 및 형식 검사다. 기존 src/와 tests/는 변경하지 않았다. C# 컴파일, 테스트 실행, 엔진 실행, Windows VM 소스 복사, 설치와 UI 동작 및 성능 측정은 수행하지 않았다. 문서의 코드 위치는 이번 검토 시점의 행 번호다.

## 11. Desktop 연결 구현 현황

이번 변경 범위는 src/Zipelo.Desktop과 이 문서다. Core, 작업자와 tests/는 수정하지 않았다. 공유 브랜치는 유지했다. 아래는 소스 구현 상태이며 새 변경의 컴파일 또는 Windows 실행 통과를 뜻하지 않는다. 총괄이 공유한 이전 스냅샷의 빌드 결과도 이번 변경을 검증한 결과로 재사용하지 않는다.

### 연결 지점

Desktop 프로젝트에 Core 프로젝트 참조를 추가했다. 새 [CoreWorkspace](../src/Zipelo.Desktop/CoreWorkspace.cs)는 IArchiveWorkspace와 선택적 IWorkspaceCapabilities를 구현한다. CoreJobRunner 델리게이트를 받으며 정확한 형태는 다음과 같다.

```csharp
Task<JobResult> Run(
    WorkspaceRequest request,
    IProgress<JobEvent> progress,
    PasswordProvider passwords,
    CancellationToken cancellationToken);
```

런타임 호스트가 실제로 완성되면 Desktop 시작점에서 `new CoreWorkspace(Run, capabilities)`를 생성해 MainWindow에 전달한다. capabilities는 실제 작업 프로세스의 EngineCapability 컬렉션이어야 하며 추정한 지원표를 넣지 않는다. 초기 어댑터 작성 당시에는 런타임이 없었으나 이후 같은 라운드에서 ZipWorkerClient.ListAsync가 추가되어 아래 실제 목록 호출 연결까지 보강했다. 작업자 배포 파일이 없으면 UnavailableWorkspace로 유지하며 가짜 목록이나 압축 결과는 생성하지 않는다.

Run을 제공하는 호스트는 경로 기반 요청을 안전하게 검증하고 별도 작업 프로세스로 보낸다. 요청의 작업 ID를 그대로 사용하며 입력 스트림과 출력 트랜잭션, 프로세스 종료 및 잔여 데이터 정리를 마친 뒤 JobResult를 반환해야 한다. CoreWorkspace는 파서, 파일 출력이나 프로세스 실행 코드를 구현하지 않는다. UI 프로세스에서 IArchiveEngine.ExecuteAsync를 직접 호출하는 연결은 금지한다.

CoreJobRunner는 WorkspaceRequest.Action의 List/Create를 ArchiveAction의 동명 값으로 명시적으로 변환해 호스트 계약에 연결한다. WorkspaceRequest.Format은 현재 zip이며 목록 파일 선택 필터도 ZIP으로 한정했다. 형식 문자열은 실제 서명 검증을 대체하지 않는다. 생성 입력 이름, 출력 위치와 기존 파일 충돌은 호스트에서 검증한다. 호스트의 실제 공개 진입점이 마련되면 이 델리게이트에 맞추는 얇은 호출 변환만 추가하면 된다. 이를 실제 런타임 연결 완료로 표현하지 않는다.

### 계약 변경 범위

Core 공개 타입과 IArchiveWorkspace.ExecuteAsync 시그니처는 변경하지 않았다. Desktop의 기존 위치 인수 생성자를 유지하면서 WorkspaceRequest.Format, WorkspaceProgress.JobId/Sequence/State, PasswordChallenge.Attempt, WorkspaceResult.JobId/Error와 FileRow.EntryId/Kind/Outcome/Error를 init 속성으로 추가했다. Core 정보를 화면까지 보존하기 위한 보강이다. 기존 IArchiveWorkspace 구현이 있더라도 새 메타데이터를 채우지 않으면 유효한 결과로 받아들이지 않는다.

기능 조회는 선택적 IWorkspaceCapabilities.CanExecute와 AvailabilityMessage를 사용한다. IsAvailable만 true인 구현은 자동으로 버튼을 켜지 않는다. List와 Create 각각 ZIP 능력이 있어야 해당 버튼을 사용할 수 있다. Extract는 호스트의 능력표에 있더라도 어댑터와 화면, 요청 진입부 모두에서 닫아 두었다. 실제 Windows 출력 방어가 검증된 뒤 별도 변경으로 열어야 한다.

### 구현된 화면 처리

- 파일 대화상자 이전에 Selecting 게이트를 얻고 결과 게시까지 유지한다. 중복 이벤트는 새 대화상자나 작업을 만들지 않는다. 작업 중 다른 파일 열기를 거부한다.
- 파일 선택 취소, 목록 읽기 실패와 잘못된 결과 ID는 이전 목록과 문서 제목을 유지한다. 새 목록은 실제 Completed 또는 Partial 결과를 받아야 교체한다. 현재 Core가 Partial을 생성하지 않는다는 기존 제약은 유지한다.
- CoreWorkspace와 화면 모두 작업 ID와 증가하는 이벤트 Sequence를 확인한다. Finishing이나 작업 종료 뒤의 이벤트는 화면을 되돌리지 않는다. 종료 이벤트만으로 다음 작업을 허용하지 않고 Task 반환과 화면 결과 반영까지 기다린다.
- Core 전체 크기는 알 수 없으므로 진행 이벤트의 TotalBytes는 null로 전달한다. WaitingForPassword, Cancelling과 종료 확인 상태를 별도로 안내한다.
- 반환 결과는 취소 토큰이 켜져 있다는 이유로 버리지 않는다. 실제 Completed 결과와 게시 확인 항목 수를 보존한다. Failed/Cancelled는 정상 중지와 구분해 정리 실패 가능성을 안내한다.
- 항목 ID, 종류, 추출 및 검증 상태와 오류 코드를 보존한다. 목록 검증 전과 검증 통과를 구분하며, 결과 요약은 항목 수, 게시 확인 수와 실패 수를 표시한다. 상세 항목 오류 화면은 후속 범위다.
- 오류는 [WorkspaceMessages](../src/Zipelo.Desktop/WorkspaceMessages.cs)의 허용된 코드별 문구로 표시하고 엔진 예외 원문은 출력하지 않는다. 실패 후 진행률을 0으로 복구한다.
- 창 닫기와 중지는 실제 종료를 기다린다. 취소 요청만으로 작업을 끝난 것처럼 표시하지 않는다. 비협력 엔진의 강제 종료는 여전히 호스트의 책임이며 이번 UI 작업으로 구현하지 않았다.

### 암호 소유권

작업별 RequestId 집합으로 중복 응답을 차단하고 잘못된 작업 및 요청은 InvalidPasswordResponse로 거부한다. UI 스레드에서만 암호창을 열고 외부 취소가 오면 창을 닫는다. 사용자 취소는 null 응답 및 해당 작업 취소로 전달한다. Attempt는 재입력 안내에 반영하되 잘못된 암호라고 단정하지 않는다.

PasswordWindow.TakePassword는 SecureString 소유권을 한 번만 인계하며 참조를 지운다. 정상 인계 후 Core가 폐기한다. UI 인계 실패, 취소와 예외에는 DiscardPassword로 남은 UI 소유 암호를 폐기한다. 평문 변환과 저장은 추가하지 않았다. 암호창의 표시 전후 상태 복원과 실제 Dispatcher 경합은 Windows 검증이 필요하다.

### 다음 연결과 검증

런타임의 정확한 진입점, 실제 capability 조회와 배포 시 작업자 파일 위치가 아직 필요하다. App의 시작점을 바꾸기 전에 이 세 조건을 확인해야 한다. 작업자가 배포되지 않은 현재 체크아웃에서는 목록과 생성 버튼도 비활성 상태다. 고정된 배포 위치에 실제 작업자가 있으면 목록만 활성화한다. 해제, 생성 암호, 분할, 미리보기와 복구의 지원 완료를 주장하지 않는다.

후속 검증에서는 CoreWorkspace 변환, 순서가 역전된 이벤트, 다른 작업 ID, 중복 클릭, 파일 대화상자 취소, 목록 실패 후 기존 화면 보존, 암호 인계와 취소, 게시 후 늦은 취소 및 실패 뒤 재시도를 확인해야 한다. 테스트는 담당자의 tests/ 범위에서 추가하고 Windows 재검증은 총괄이 조율한다. 이번 라운드는 새 UI 동작 테스트를 작성하지 않았다.

### 같은 라운드에서 추가된 런타임 목록 진입점 연결

[ZipWorkerWorkspace](../src/Zipelo.Desktop/ZipWorkerWorkspace.cs)는 새 [ZipWorkerClient](../src/Zipelo.Runtime/ZipWorkerClient.cs)의 `ListAsync(Guid, Stream, JobLimits, CancellationToken)`을 사용한다. Desktop에 Runtime 프로젝트 참조를 추가했으며 Core와 Runtime의 공개 API는 변경하지 않았다. App은 `ZipWorkerWorkspace.TryCreate() ?? new UnavailableWorkspace()`와 같은 선택을 수행한다.

실행 경로는 앱 배포 폴더 아래 `worker/Zipelo.Worker.exe`로 고정한다. 압축파일 경로나 PATH로 작업자를 탐색하지 않고 dotnet 호스트를 추측하지 않는다. 실제 작업자 실행 파일이 없으면 목록 버튼도 켜지 않는다. 당시 미구현이었던 작업자 배치는 12장의 Desktop 배포 설정으로 추가했다. 실제 산출물 검증은 아직 남아 있다. 이 문서 작성 중 VM 복사나 실행은 하지 않았다.

Desktop은 선택 파일을 FileAccess.Read, FileShare.Read로 열어 런타임에 전달한다. 파싱은 직접 수행하지 않는다. 입력 스트림은 런타임 응답과 작업자 정리가 끝난 뒤 폐기한다. 이 연결의 개발 한도는 항목 1024개, 처리 및 출력 각각 16MiB, 시간 2분, 암호 요청 상한 3회다. 제품 기본값이나 성능 목표가 아니라 런타임 한도 안에서 사용하는 초기 연결값이다. 실제 이 런타임은 암호를 요청하지 않는다.

현재 ZipWorkerClient는 진행 콜백과 PasswordProvider를 받지 않는다. 따라서 CoreJobRunner의 해당 두 매개변수는 이 호출 변환에서 사용하지 않으며 실시간 바이트나 암호 이벤트를 꾸며내지 않는다. UI는 실제 Task가 끝날 때까지 불확정 진행 표시를 유지하고 JobResult만 반영한다. 반환된 CreatedZip은 이 목록 경로에서 사용하지 않는다.

런타임의 Capabilities에는 ZIP Create도 있지만 SupportsFilePublication은 false다. 생성 완료를 디스크 저장 완료로 오인하지 않도록 Desktop은 이 호스트에 대해 List 능력만 게시한다. Create와 Extract 버튼은 비활성이다. 메모리 ZIP 생성과 안전한 파일 게시를 연결하는 추가 계약이 마련돼야 생성 화면을 활성화할 수 있다. 안전한 해제 검증 전 폐쇄 원칙은 유지한다.

## 12. 작업자 배포 연결

이 단계는 Desktop 프로젝트, 새 WorkerDeployment.targets, Desktop의 필수 파일 확인과 오류 안내 및 이 문서만 수정했다. Worker, Runtime과 Core 소스 및 프로젝트, tests/는 변경하지 않았다. 기존 브랜치를 유지하고 설치, 업로드, 빌드와 VM 실행은 하지 않았다.

### 개발 빌드와 개발 실행

Worker 프로젝트의 실제 AssemblyName 기본값은 Zipelo.Worker이고 OutputType은 Exe, 대상은 net10.0이다. Windows에서 일반 SDK 빌드는 apphost exe, 관리 DLL, runtimeconfig/deps JSON과 참조된 Runtime 및 Core DLL을 출력한다. Desktop은 net10.0-windows이므로 Worker를 관리 코드 참조로 추가하지 않고 ReferenceOutputAssembly=false, Private=false인 빌드 의존성으로 추가했다. Worker 소스는 변경하지 않는다.

Desktop의 [WorkerDeployment.targets](../src/Zipelo.Desktop/WorkerDeployment.targets)는 Windows의 일반 Build 이후 Worker의 GetTargetPath로 실제 출력 폴더를 얻는다. Configuration과 RuntimeIdentifier를 맞추고 UseAppHost=true를 사용한다. 작업자 필수 파일 6개가 존재하는지 확인한 뒤 작업자 출력과 하위 의존 파일을 Desktop TargetDir/worker로 복사한다. 이전 기본 publish 하위 폴더는 개발 빌드 복사 대상에서 제외한다. 개발 실행은 이 배치가 끝난 Desktop 빌드 결과를 사용한다. DesignTimeBuild에서는 배치 타깃을 실행하지 않는다.

Windows 개발 환경의 명령 예시는 다음과 같다. 이번 라운드에서 실행한 명령은 아니다.

```powershell
dotnet build src/Zipelo.Desktop/Zipelo.Desktop.csproj -c Release
dotnet run --project src/Zipelo.Desktop/Zipelo.Desktop.csproj -c Release --no-build
```

기본 개발 빌드는 framework-dependent이므로 개발 PC에는 해당 .NET Desktop Runtime과 Worker가 사용하는 .NET 런타임이 필요하다. SDK는 개발 도구일 뿐 최종 사용자 요구사항으로 안내하지 않는다. 이전 산출물과 혼합되지 않은 폴더에서 출력 구성을 검증해야 한다.

### 자체 포함 게시 설정

Desktop의 WindowsX64 프로필은 win-x64, SelfContained=true, PublishSingleFile=false, PublishTrimmed=false다. Desktop Publish 이후 Worker를 같은 RID와 SelfContained 값으로 별도 Publish하며 결과 위치는 Desktop PublishDir 아래 worker로 고정한다. 부모의 PublishProfile, PublishProfileFullPath와 PublishDir을 그대로 상속해 Worker가 Desktop 프로필이나 출력 루트를 사용하는 일을 피한다. Worker는 이 별도 게시 경로에서도 single-file과 trimming을 사용하지 않는다.

게시 타깃은 별도 Restore나 SDK 설치를 수행하지 않는다. Worker는 Desktop의 프로젝트 참조 그래프에 포함되므로 최초 dotnet publish의 복원에 같은 RID와 런타임 속성이 적용돼야 한다. 이를 명확히 하는 검증 명령은 다음과 같다. 필요한 .NET 대상 팩이 없는 오프라인 환경이라면 복원을 실패로 보고해야 하며 누락 파일을 무시한 게시를 성공으로 간주하지 않는다.

```powershell
dotnet publish src/Zipelo.Desktop/Zipelo.Desktop.csproj -c Release -r win-x64 --self-contained true -p:PublishProfile=WindowsX64
```

프로젝트를 중첩 실행할 때 Properties와 RemoveProperties를 명시하는 방식은 [MSBuild 공식 작업 설명](https://learn.microsoft.com/en-us/visualstudio/msbuild/msbuild-task)을 따른다. DLL, deps 및 runtimeconfig와 의존성을 포함하는 게시 방식은 [dotnet publish 공식 설명](https://learn.microsoft.com/en-us/dotnet/core/tools/dotnet-publish)을 근거로 한다. 실제 산출물과 오프라인 실행은 아직 확인하지 못했다.

최소 배치 구조는 다음과 같다. 자체 포함 게시에서는 이 밖의 네이티브 런타임과 관리 런타임 파일도 생성될 수 있으며 게시 폴더 전체를 유지해야 한다.

```text
Zipelo.exe
Zipelo.dll
Zipelo.runtimeconfig.json
Zipelo.deps.json
Zipelo.Core.dll
Zipelo.Runtime.dll
worker/
    Zipelo.Worker.exe
    Zipelo.Worker.dll
    Zipelo.Worker.runtimeconfig.json
    Zipelo.Worker.deps.json
    Zipelo.Core.dll
    Zipelo.Runtime.dll
    [자체 포함 Worker 런타임과 추가 의존 파일]
[Desktop 런타임과 추가 의존 파일]
```

최종 사용자용 독립 설치 패키지 자체를 만든 것은 아니다. 자체 포함 게시 폴더는 설치 프로그램 제작의 입력 후보이며, 이후 서명, 고지, 설치 및 제거와 깨끗한 Windows에서의 동작 검증이 필요하다. 최종 배포는 사용자가 SDK를 설치하는 방식으로 설계하지 않는다. Desktop과 Worker가 각자 런타임을 포함하므로 용량 중복이 생길 수 있다. 설치 크기와 메모리, 시작 시간은 측정 전이며 기존 경량성 목표를 달성했다고 표시하지 않는다. 중복 제거는 Worker의 독립 실행과 런타임 탐색 규칙을 검증한 뒤 별도 설계해야 한다.

### 고정 경로와 필수 파일 실패

ZipWorkerWorkspace는 AppContext.BaseDirectory/worker만 사용한다. 현재 작업 디렉터리, 선택한 압축파일 폴더와 PATH에서 실행파일을 찾거나 없을 때 dotnet을 대신 실행하지 않는다. 필수 파일 목록은 Worker exe, Worker DLL, Worker runtimeconfig/deps JSON, Core DLL과 Runtime DLL이다. 시작 시와 실제 목록 작업 직전에 존재 여부를 확인한다. 이 검사는 파일 내용, 버전이나 서명을 검증했다는 뜻이 아니다.

필수 파일이 없으면 UnavailableWorkspace가 누락 안내를 표시하고 작업 버튼을 비활성화한다. 파일을 복원한 뒤 앱을 다시 실행해야 한다. 필수 파일 확인 후에도 실행 또는 프로토콜 오류가 날 수 있으므로 EngineUnavailable과 EngineFailure를 각각 작업자 시작 실패와 정상 결과 수신 실패로 안내한다. Runtime의 내부 예외와 stderr 원문은 노출하지 않는다. 기존 RunJob 종료 처리는 실제 Task 반환 뒤 Busy를 해제하고 이전 목록을 유지해 정상 화면으로 돌아가게 한다.

ZIP 생성은 메모리 생성과 디스크 게시가 구분되므로 계속 비활성이다. 안전한 출력 저장과 게시의 검증이 끝나기 전에는 생성과 해제를 켜지 않는다.

### 실제 확인과 남은 검증

실제로 확인한 것은 프로젝트와 Runtime API 및 고정 경로의 소스 대조, Desktop과 targets의 XML 파싱, 서식과 문서 링크 및 기존 UI 정적 검사다. 프로젝트 또는 게시 타깃 실행, 실제 exe/DLL 배치, 런타임 누락 상태, 작업자 시작 실패, 프로토콜 실패, 실패 후 재시도와 자체 포함 설치 및 용량 측정은 미실행이다. 다른 스냅샷의 이전 Windows 빌드 성공은 이번 targets 검증을 대신하지 않는다.


## ZIP 생성 UI 연결 후속 상태

이 절은 앞 절의 생성 미연결 상태를 갱신한다. Windows와 Ubuntu는 공유 UI 소스
[ZipCreationWorkflow](../src/Zipelo.Desktop/ZipCreationWorkflow.cs)를 사용한다.
Ubuntu 프로젝트가 이 소스만 링크하며 WPF 프로젝트를 참조하지 않는다.
파일 선택, 읽기 전용 입력 스냅샷, CreateAsync, NewZipPublisher.PublishAsync,
Published/Error/CleanupFailed 결과 표시를 연결했다. 실행 검증 완료를 뜻하지 않는다.

일반 Debug/Release 빌드의 생성은 비활성이다. 명시적으로
`-p:ZipeloCreationValidation=true`를 지정한 개발 검증 빌드만
`ZIPELO_CREATION_VALIDATION`으로 생성 버튼과 실제 호출을 허용한다.
이는 OS별 게시 안전성이나 UI 검증 완료 플래그가 아니다. 배포물에는 지정하지 않는다.
작업자 필수 파일과 x64 게시 구현 대상 여부도 확인한다. 해제는 계속 비활성이다.

- 파일 최대 1,024개와 실제 읽은 누적 바이트 16 MiB를 제한한다. 각 입력 스트림을 읽은 뒤
  닫고 메모리 스냅샷을 작업자에 전달한다. 폴더 재귀, 암호, 분할 옵션은 제공하지 않는다.
- 입력 읽기부터 게시까지 연결된 2분 취소 타이머를 사용한다. 런타임도 입력과 생성물 한도를
  검사한다. 포털 OpenReadAsync 자체는 취소 인수를 받지 않으므로 제공자가 스트림 열기를
  반환하기 전 지연까지 강제 종료한다고 보장하지 않는다. 네이티브 게시 호출의 지연도 별도 검증한다.
- Windows는 OpenFileDialog와 SaveFileDialog를 사용한다. 덮어쓰기 질문은 끄고
  기존 목적지가 있으면 게시 API가 충돌로 거부한다. 기존 파일 교체를 허용하지 않는다.
- Ubuntu는 파일 제공자의 읽기 스트림, 폴더 선택과 새 ZIP 이름 입력을 사용한다.
  SaveFilePicker로 빈 파일을 미리 만들지 않는다. 명시적으로 선택한 로컬 폴더만 허용하며
  원격 포털 URI를 로컬 경로로 추측하지 않는다. 새 파일명은 경로 없이 .zip으로 끝나야 한다.
- 실패 시 정상 압축파일 목록과 생성 파일 선택을 유지한다. 생성 다시 시도로 같은 선택을
  재사용할 수 있다. Ubuntu에서는 이름을 고쳐 재시도하며, Windows의 새 저장 위치 선택은
  새로 압축하기에서 수행한다. 새 입력을 고른 뒤 목적지 선택을 취소하면 이전 목적지로
  자동 저장하지 않는다. 파일 선택부터 종료 정리까지 중복 요청을 막는다.
- Published만 디스크 저장 성공으로 표시한다. 게시 후 늦은 취소는 실제 성공을 유지한다.
  CleanupFailed는 임시 파일 정리 실패를 별도로 안내하며 임의 파일을 삭제하지 않는다.
  중간 메모리 생성 결과를 디스크 게시 성공으로 바꾸지 않는다.

필요한 도구는 .NET 10 SDK와 Windows 검증 환경의 Windows Desktop targeting pack이다.
Ubuntu UI의 직접 패키지는 기존 고정 버전을 유지한다. 추가 패키지는 없다.
공식 NuGet 출처는 [Avalonia 11.3.9](https://www.nuget.org/packages/Avalonia/11.3.9),
[Avalonia.Desktop 11.3.9](https://www.nuget.org/packages/Avalonia.Desktop/11.3.9),
[Avalonia.Themes.Fluent 11.3.9](https://www.nuget.org/packages/Avalonia.Themes.Fluent/11.3.9)다.
종속 패키지 전체는 승인된 복원 후 실제 자산과 라이선스 고지를 검토해야 한다.
최종 사용자에게 SDK를 요구하는 배포 설계가 아니다.

이번에는 기존 frontend 정적 구조 검사 6개와 프로젝트/화면 XML, 이벤트/컨트롤 연결,
서식을 확인했다. SDK가 PATH에 없어 컴파일하지 않았고 패키지 복원도 실행하지 않았다.
Windows와 Ubuntu 24.04/26.04 LTS x64 각각에서 실제 생성/취소, 기존 파일 충돌과 원본 보존,
읽기 실패 후 재시도, 게시/정리 실패, 선택 대화상자 취소와 중복 클릭을 검증해야 한다.
Ubuntu는 X11과 Wayland/XWayland, 배율, 키보드와 파일 포털 동작을 구분한다.
현재 호스트 환경에 대한 전달 기록은 실제 UI 실행 결과를 대신하지 않는다.


## 시작 인수 연결 후속 상태

단일 로컬 ZIP 경로를 창 준비 후 기존 목록 읽기로 전달하는 UI 연결을 추가했다.
오류 처리, 패키지 진입 명령과 미검증 범위는 [로컬 파일 열기 계약](LOCAL_ARCHIVE_OPEN.md)에 기록했다.
시작 인수의 실제 OS 실행 검증 전까지 파일 연결 등록 차단은 유지한다.

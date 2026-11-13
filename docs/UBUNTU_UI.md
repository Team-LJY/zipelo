# Ubuntu UI 선택과 연결 상태

> 과거 C# 구현 기록: C#은 v0.1에서 중단하고 Rust로 대체했다. PR103(f1d9fa5)에서 C# 소스와 전용 시험 및 .NET 설정을 제거했다. 아래 본문의 "현재", 미구현 상태, API와 검증 결과는 작성 당시 C# 스냅샷에 관한 내용이며, 현재 Rust 제품의 지원 범위나 검증 상태를 뜻하지 않는다. 삭제된 경로와 실행 명령은 현재 main에서 사용할 수 없다.
>
> 현재 기준은 [기획서](PLANS.md), [UI 설계](ui-design-v0.1.md), [릴리스 노트](release-notes-v0.1.md)다. 앱은 [Rust UI](../experiments/rust-ui-memory-app/), 압축 처리는 [Rust 작업자](../experiments/rust-archive-worker/README.md)를 사용한다. 과거 검사 재현과 소스 보존 안내는 [기획서의 과거 기록](PLANS.md#7-c-중단과-과거-검증-기록)을 따른다.

## 보존된 당시 기록

## 대상과 현재 상태

Ubuntu 24.04 LTS와 26.04 LTS x64를 개발 및 정식 지원 목표로 준비한다. 각 환경에서 설치와 UI, 공통 기능을 검증하기 전 지원 완료로 표시하지 않는다. Windows WPF는 src/Zipelo.Desktop에 보존했고 이번 변경에서 수정하지 않았다. WPF를 Ubuntu에서 실행하는 구성이 아니다.

이번 추가 범위는 [src/Zipelo.Ubuntu](https://github.com/Team-LJY/zipelo/blob/ae91536/src/Zipelo.Ubuntu/Zipelo.Ubuntu.csproj)의 UI 뼈대와 이 문서다. Core, Runtime, Worker와 tests/는 수정하지 않았다. 실제로 실행한 것은 소스 및 공식 자료 열람과 정적 XML, 이벤트 연결, 프로젝트 참조와 서식 검사뿐이다. OS나 SDK, NuGet 패키지와 시스템 라이브러리를 설치하지 않았고 restore, build, publish 및 UI 실행도 하지 않았다.

로컬 /etc/os-release는 Ubuntu 26.04.1 LTS를 나타내고 XDG_SESSION_TYPE은 wayland다. DISPLAY와 WAYLAND_DISPLAY 환경변수는 있지만 이것만으로 화면 접근이나 XWayland 동작을 입증하지 않는다. dotnet은 PATH에 없다. Ubuntu 24.04 실행 환경은 이번 세션에서 확인하지 않았다.

## 도구 비교와 선택

- WPF: 기존 Windows UI를 유지하기에는 적합하지만 Windows 전용이라 Ubuntu UI 후보에서는 제외한다. [Microsoft WPF 설명](https://learn.microsoft.com/en-us/dotnet/desktop/wpf/overview/)
- GTK와 GtkSharp: GTK의 GDK는 Linux의 X11과 Wayland를 추상화한다. GtkSharp 저장소의 LICENSE는 GNU Library GPL v2를 포함한다. 자유 소프트웨어 후보지만 GTK와 C# 바인딩, 배포 의존성의 별도 버전 검증 및 고지 검토가 필요하다. WPF와 유사한 XAML UI 구조를 사용하려는 이번 최소 뼈대에서는 채택하지 않았다. LGPL이라는 이유로 비자유라고 판단한 것은 아니다. [GTK 구조](https://www.gtk.org/docs/architecture/), [GtkSharp 라이선스](https://raw.githubusercontent.com/GtkSharp/GtkSharp/develop/LICENSE)
- Avalonia 기본 오픈소스 UI: C#과 XAML을 사용하고 Core/Runtime의 net10.0 계약을 별도 Windows 호환 계층 없이 참조할 수 있어 선택했다. 확인한 11.3.9 태그의 프로젝트 라이선스는 MIT다. 저작권 및 허가 고지를 포함해야 한다. 기본 UI와 Fluent 테마만 참조하며 XPF와 상용 확장에 의존하지 않는다. [태그 라이선스](https://raw.githubusercontent.com/AvaloniaUI/Avalonia/11.3.9/licence.md)

프로젝트에는 Avalonia, Avalonia.Desktop, Avalonia.Themes.Fluent를 모두 11.3.9로 고정했다. 이 버전은 확인한 API와 소스 기준으로 뼈대를 재현하기 위한 선택이며 최신 버전이라고 주장하지 않는다. NuGet 페이지에는 더 새 버전이 있다고 표시된다. 실제 패키지 복원 전 소스 참조만 작성한 상태이며 배포 버전은 유지보수와 취약점, 의존성 고지를 확인한 뒤 재평가해야 한다. 라이선스 원문 확인만으로 Skia와 HarfBuzz 등 모든 전이 의존 패키지의 배포 검토가 끝난 것은 아니다. [선택 버전 패키지](https://www.nuget.org/packages/Avalonia/11.3.9)

## X11과 Wayland의 정확한 범위

11.3.9의 UsePlatformDetect 구현은 Linux에서 UseX11과 Skia를 선택한다. 따라서 이 뼈대는 X11 클라이언트이고 Wayland 세션에서는 XWayland가 필요하다. 네이티브 Wayland 클라이언트로 구현했다고 표시하지 않는다. [선택 버전의 플랫폼 초기화 소스](https://raw.githubusercontent.com/AvaloniaUI/Avalonia/11.3.9/src/Avalonia.Desktop/AppBuilderDesktopExtensions.cs)

현행 공식 문서는 Linux 기본 X11, Wayland 세션의 기본 XWayland 경로와 12.1.0 이후 별도 네이티브 Wayland 백엔드를 설명한다. 이 현행 문서의 기능을 고정한 11.3.9가 제공한다고 해석하지 않는다. [Linux 플랫폼 설명](https://docs.avaloniaui.net/docs/platform-specific-guides/linux)

공식 지원표의 Ubuntu 16.04~24.x는 Tier 2이며 열람 시 표에 Ubuntu 26.04는 명시되어 있지 않았다. 라이브러리의 상위 지원표가 Zipelo의 목표 OS 검증을 대신하지 않는다. Ubuntu 26.04를 지원 완료로 추정하지 않고 자체 검증과 필요한 유지보수 범위를 확보한다. [Avalonia 지원표](https://docs.avaloniaui.net/docs/supported-platforms)

## 프로젝트와 공통 계약 연결

Program은 Avalonia AppBuilder로 별도 UI를 시작한다. App과 MainWindow는 WPF를 참조하지 않는다. [UbuntuWorkspace](https://github.com/Team-LJY/zipelo/blob/ae91536/src/Zipelo.Ubuntu/UbuntuWorkspace.cs)는 Core의 EngineCapability, JobLimits, JobResult와 Runtime의 ZipWorkerClient.ListAsync를 사용한다. UI 프로세스에서 ZIP 파서를 직접 호출하지 않는다.

실제 목록 연결은 다음 순서다.

1. StorageProvider.CanOpen과 런타임의 ZIP List 능력을 확인한다.
2. UI 게이트를 잡은 뒤 OpenFilePickerAsync로 파일을 선택한다. 파일 선택 취소는 기존 목록을 유지한다.
3. 선택한 IStorageFile의 OpenReadAsync로 읽기 스트림을 얻는다. 포털이 반환한 URI를 로컬 경로로 바꾸거나 네트워크 URI를 직접 다운로드하지 않는다.
4. 읽기 스트림을 별도 작업 프로세스를 사용하는 ListAsync에 전달하고 작업 반환 및 입력 폐기까지 기다린다.
5. 일치하는 작업 ID의 Completed/None 결과만 새 목록으로 게시한다. 목록 확인과 내용 검증은 구분한다. 실패와 취소는 이전 목록을 보존한다.

[StorageProvider 공식 계약](https://docs.avaloniaui.net/docs/services/storage/storage-provider)은 선택 결과와 취소 결과를 제공하지만 실제 Linux 파일 선택 제공자의 동작은 두 LTS에서 확인해야 한다. 새 UI에는 중복 클릭 차단, 작업 중 새 파일 요청 차단, 중지 요청과 종료 대기 및 실패 후 게이트 복귀를 작성했다. 이번에는 암호 UI나 미리보기를 새로 만들지 않았으며 생성과 해제도 비활성이다. 목록 파싱에서 발생하는 암호와 분할의 한계는 현재 공통 Runtime 구현을 따른다.

WorkerPlatform.GetWorkerPath(AppContext.BaseDirectory)를 사용한다. Ubuntu에서는 앱 배포 폴더의 worker/Zipelo.Worker가 실행 진입점이며 현재 작업 폴더, 압축파일 경로나 PATH를 탐색하지 않는다. Worker apphost, Worker DLL, runtimeconfig/deps JSON과 Core/Runtime DLL이 있어야 목록 버튼을 켠다. 파일 존재 검사는 실행 권한이나 서명 검증과 동의어가 아니다. 실행 비트가 없으면 설정을 자동 변경하지 않고 런타임 실패를 안내한다.

초기 연결의 제한값은 항목 1024개, 처리 및 출력 각각 16MiB, 시간 2분과 암호 요청 상한 3회다. 제품 성능이나 기본값 확정이 아닌 개발 한도다. 현재 ListAsync에는 실시간 진행 또는 암호 콜백이 없으므로 UI는 불확정 진행 표시만 제공하고 가짜 바이트 이벤트를 만들지 않는다. 입력 스트림이 쓰기 가능한 형태로 반환되면 공통 Runtime이 거부할 수 있으므로 파일 제공자별 실제 확인이 필요하다.

Worker 배포 자동화와 .deb 생성은 아직 이 최소 프로젝트에 포함하지 않았다. LinuxX64 게시 프로필은 UI의 net10.0 자체 포함 폴더 게시 시작점이며 설치 파일이 아니다. 설치 및 기본 파일 연결은 별도 플랫폼 작업이다. JAR 필터는 목록 열기에서 거부하며 앱/MIME 등록을 이번 작업에서 추가하지 않았다.

## UI 디자인 기준

기존 Zipelo 화면과 같은 흰색 #FFFFFF, 작업 배경 #F3F6FA, 본문 #17263C와 강조 #185ABD를 사용한다. 폰트는 시스템의 Noto Sans CJK KR 또는 Ubuntu 계열을 이용하며 이번 작업에서 폰트 파일을 다운로드하지 않았다. 상단 제목과 작업 버튼, 중앙 목록, 하단 상태 및 중지의 왼쪽 정렬 구조를 유지한다. 중요한 안내는 본문 16 크기를 사용하며 작은 보조 문구에 숨기지 않는다. 실측 후 배율과 한글 폰트 대체를 조정해야 한다.

## 환경별 검증 항목

아래는 실행할 항목이며 현재 통과 기록이 아니다. 양쪽 LTS x64에서 각각 결과와 세션 종류, compositor 및 XWayland 버전을 기록한다. 다른 아키텍처의 지원을 주장하지 않는다.

### Ubuntu 24.04 LTS

- X11 세션을 제공하는 검증 환경에서는 실제 X11 클라이언트 실행과 소프트웨어/GPU 렌더링을 확인한다.
- Wayland 세션에서는 XWayland 존재, 창 표시, 파일 선택창의 소유 관계와 포커스 복귀를 확인한다. 네이티브 Wayland 시험으로 집계하지 않는다.
- 각 세션에서 100%, 125%, 150%, 200% 배율과 모니터 간 이동, 한글 글꼴 대체, 목록 및 중지 버튼 잘림을 확인한다.
- Tab, Shift+Tab, Enter, Space, 목록 화살표 및 파일 선택 취소 뒤 포커스를 확인한다. 한글 IME 입력과 경로 선택도 실제로 검증한다.

### Ubuntu 26.04 LTS

- Wayland 환경의 XWayland 경로를 우선 검증한다. 순수 Wayland에서 XWayland 없이 실행 가능하다고 약속하지 않는다.
- 독립 X11 검증 환경을 사용할 수 있는 경우 별도 시험한다. 해당 LTS가 X11 로그인 세션을 제공한다고 가정하거나 이를 위해 시스템을 변경하지 않는다.
- 분수 배율과 복수 모니터에서 포인터 위치, 폰트 선명도, 창 크기와 한글 파일명 잘림을 기록한다.
- 파일 선택 포털의 표시, 취소와 재선택, 권한 거부, 파일 삭제, 긴 이름 및 대소문자가 다른 파일 선택을 확인한다. 설치된 포털 제공자를 임의로 교체하지 않는다.

### 양쪽 환경 공통

- 빈 ZIP, 한글 이름, 파일명 대소문자 구분, 지원 밖 입력, 입력 한도 초과와 잘못된 프로토콜에서 실제 결과 및 정상 화면 복귀를 확인한다.
- 파일 선택 중 중복 클릭, 처리 중 중지와 창 닫기, 종료 후 새 파일 열기를 검사한다. 작업자 종료 전 다음 작업이 시작되면 실패다.
- 키보드 포커스 가시성, 충분한 대비와 Orca 접근성을 확인한다. Avalonia 선택만으로 Linux 화면 읽기 프로그램 지원이 검증됐다고 판단하지 않는다.
- 파일 선택 제공자에 따라 필요한 GTK 또는 데스크톱 포털의 유무를 구분한다. 포털 URI, 공백과 한글 경로를 로컬 문자열로 잘못 변환하지 않아야 한다.
- 생성 및 해제 버튼이 아직 비활성인지 확인한다. 다운로드와 설치가 완료된 것처럼 안내하면 안 된다.

## 필요한 도구와 실행 환경

개발 검증에는 승인된 .NET 10 SDK, 위 3개 Avalonia 패키지 및 전이 의존성의 복원 환경, 두 LTS의 x64 데스크톱과 실제 X11 또는 Wayland/XWayland 세션이 필요하다. 공식 지원 문서에 나오는 libX11, libICE, libSM, fontconfig 계열과 실제 Skia 네이티브 의존성, 한글 폰트 및 파일 선택 제공자의 존재를 확인해야 한다. 배포물에 포함할 런타임과 시스템 의존성의 정확한 패키지 이름은 각 LTS에서 점검한다. 이번에 설치하거나 복원하지 않았다.

검증 환경 승인 뒤 개발자가 사용할 명령 예시는 다음과 같다. 지금 실행한 명령이 아니며 복원이 패키지 다운로드를 수반한다.

```text
dotnet build src/Zipelo.Ubuntu/Zipelo.Ubuntu.csproj -c Release
dotnet run --project src/Zipelo.Ubuntu/Zipelo.Ubuntu.csproj -c Release --no-build
dotnet publish src/Zipelo.Ubuntu/Zipelo.Ubuntu.csproj -c Release -r linux-x64 --self-contained true -p:PublishProfile=LinuxX64
```

최종 사용자는 SDK를 설치하지 않는 .deb와 자체 포함 앱 배포를 목표로 한다. UI 및 Worker의 런타임 중복과 네이티브 의존성, 실행 권한, 데스크톱 항목과 사용자 동의에 따른 연결은 실제 설치 검증이 필요하다. 설치 프로그램은 이번에 만들지 않았다.

## Windows ZIP 생성 UI의 의존점

기존 Windows Create 화면은 파일 선택과 저장 위치 선택을 준비했으나 새 파일 게시의 OS별 실행 검증과 결과 계약 연결을 기다린다. 현재 CreateAsync가 반환하는 메모리 CreatedZip만으로는 저장 완료를 표시할 수 없다. CoreWorkspace의 Create 능력은 게시 계약과 실제 구현 및 검증이 확인돼야 게시한다.

연결할 계약에는 명시적으로 선택한 새 목적지, 입력 원본의 읽기 전용 수명, 기존 파일 덮어쓰기 거부, 게시 전 취소와 게시 후 늦은 취소의 구분, 실패 후 임시 파일 및 실제 게시 결과가 필요하다. 임의 압축 항목 해제의 저장 계층과 혼동하지 않는다. 게시 API가 생기면 실제 메서드와 반환 상태, 지원 OS를 확인하고 Desktop의 CoreJobRunner에서 호출 변환을 추가한다. 포털의 SaveFilePicker가 빈 파일을 미리 만들 수 있는 경우 새 파일 전용 게시 API와 충돌하는지도 Ubuntu에서 별도 설계한다.

이번 라운드에서는 게시 API를 UI에서 실행하거나 Create/Extract 버튼을 켜지 않았다. 공통 게시 구현의 실제 서명과 Windows 및 Ubuntu 검증 근거가 준비되면 UI 계약 및 결과 처리를 이어 연결한다.

### 같은 라운드에서 확인한 새 게시 API

문서 작성 중 [NewZipPublisher](https://github.com/Team-LJY/zipelo/blob/ae91536/src/Zipelo.Runtime/NewZipPublisher.cs)가 추가된 것을 확인했다. 실제 진입점은 다음과 같다. 소스 시그니처를 읽은 결과이며 실행 검증 결과가 아니다.

```csharp
Task<ZipPublicationResult> NewZipPublisher.PublishAsync(
    ZipWorkerResult result,
    string destinationPath,
    CancellationToken token);

record ZipPublicationResult(
    bool Published,
    ArchiveError Error,
    bool CleanupFailed,
    string? TemporaryFileName = null);
```

Windows Create의 후속 호출 순서는 새 저장 위치 선택 → 입력별 ZipCreateInput 준비 → ZipWorkerClient.CreateAsync → Job.Completed/Error.None과 CreatedZip 확인 → PublishAsync → Published/Error/CleanupFailed를 보존한 UI 결과 반영이다. IsSupportedPlatform은 Windows/Linux x64 구현 대상 확인값이며 각 OS 안전 검증 완료를 뜻하지 않는다. 현재 Runtime.SupportsFilePublication 값도 실제 계약과 일치하는지 연결 시 다시 확인해야 한다.

기존 CoreJobRunner는 JobResult만 반환하므로 Published와 CleanupFailed를 버리는 단순 연결은 하지 않는다. 별도 게시 결과를 보존하는 Desktop 결과 모델의 보강이 필요하다. Published=true 뒤 늦은 취소는 게시 성공을 취소로 바꾸지 않고, CleanupFailed=true는 남은 임시 결과 확인이 필요하다는 안내로 구분한다. TemporaryFileName을 무조건 로그에 남기거나 임의 파일을 삭제하는 동작은 추가하지 않는다. 이 API는 ZIP 한 개의 새 파일 게시용이며 해제를 활성화하는 근거가 아니다.


## ZIP 생성 UI 연결 후속 상태

이 절은 앞 절의 생성 미연결 상태를 갱신한다. Windows와 Ubuntu는 공유 UI 소스
[ZipCreationWorkflow](https://github.com/Team-LJY/zipelo/blob/ae91536/src/Zipelo.Desktop/ZipCreationWorkflow.cs)를 사용한다.
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

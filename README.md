# Zipelo

쉽고 빠른 압축과 압축 해제를 목표로 만드는 MIT 오픈소스 프로그램입니다.

![Windows용 다운로드 준비 중](assets/download-pending.svg)

**설치 파일은 아직 없습니다.** 위 이미지는 준비 상태를 나타내며 다운로드 링크가 아닙니다.

현재 비암호화 ZIP 목록 열람을 작업자 프로세스에 연결했습니다. 검증 담당자가 Windows 개발
검증 빌드에서 새 ZIP 생성과 저장, 기존 파일 충돌 시 보존, 파일 선택 취소 후 재시도 성공을
보고했습니다. 일반 빌드의 ZIP 생성은 아직 비활성이며 설치 가능한 출시판은 아닙니다.

안전한 새 폴더 게시와 실제 OS 검증이 남아 있어 UI의 압축 풀기는 비활성입니다.
암호화, 분할, 복구, RAR/7z/tar 등 다른 형식과 미리보기는 아직 제공하지 않습니다.
목록 열람은 압축파일 내용의 무결성 검증 완료를 뜻하지 않습니다. 개발 단계의 입력/생성물
한도는 16 MiB이며 대용량 처리나 경량성/성능 목표를 달성했다고 주장하지 않습니다.
JAR와 앱 컨테이너는 기본 연결 대상에서 제외합니다.

## 플랫폼과 구현

Windows 11 x64의 UI는 .NET 10 WPF입니다. Ubuntu 24.04/26.04 LTS x64도 정식 지원
목표이며, 별도 Avalonia 11.3.9 UI 프로젝트를 준비했습니다. Ubuntu UI의 패키지 복원,
컴파일과 실제 실행은 미검증입니다. WPF를 Ubuntu에서 그대로 실행하는 구조가 아닙니다.
다른 아키텍처의 동작은 검증하지 않았습니다.

UI는 공통 Core/Runtime 계약과 고정 앱 폴더 아래 worker/의 작업자를 사용합니다.
압축 파싱을 UI에서 직접 실행하거나 현재 작업 폴더에서 실행 파일을 찾지 않습니다.
ZIP 생성은 기존 파일을 덮어쓰지 않는 NewZipPublisher의 실제 결과를 표시합니다.
파일 선택부터 작업 종료까지 중복 요청을 막고 실패/취소 후 정상 목록과 선택을 보존합니다.
시작 인수의 단일 로컬 ZIP 절대 경로 연결은 작성했지만 실제 OS 실행 검증이 남았습니다.

## 개발 빌드

이 UI 기능 브랜치만으로는 빌드할 수 없습니다. Core/Runtime/Worker 담당 변경을 함께
반영한 뒤 Windows와 .NET 10 SDK가 준비된 환경에서 실행해야 합니다.
SDK는 개발 도구이며 최종 사용자에게 설치를 요구하는 배포 설계가 아닙니다.

```powershell
dotnet build src/Zipelo.Desktop/Zipelo.Desktop.csproj -c Release
dotnet run --project src/Zipelo.Desktop/Zipelo.Desktop.csproj
```

Windows x64 자체 포함 폴더 게시의 시작점입니다. 설치 프로그램, 서명과 릴리스 업로드는
포함하지 않습니다. 앱과 worker의 런타임 중복 용량은 아직 측정하지 않았습니다.

```powershell
dotnet publish src/Zipelo.Desktop/Zipelo.Desktop.csproj -p:PublishProfile=WindowsX64
```

ZIP 생성 연결은 명시적으로 `-p:ZipeloCreationValidation=true`를 지정한 개발 검증
빌드에서만 열립니다. 이 속성을 일반 사용자용 게시에 지정하지 않습니다. 해제는 계속 비활성입니다.
Ubuntu 개발에는 .NET 10 SDK와 승인된 Avalonia 패키지 복원 환경이 별도로 필요합니다.
이 문서 갱신에서 SDK/패키지 설치나 빌드를 실행하지 않았습니다.

## 설치와 기본 앱

설치 파일 더블클릭 → [설치] → 설치 완료 → [기본 앱 설정 열기] 또는 [나중에] 흐름을
준비 중입니다. 설치 초안은 별도 기능으로 분리하며 실제 설치/업데이트/제거 검증은 남아 있습니다.
현재 앱의 설치 안내는 계획만 보여주며 사용자 PC의 기본 앱을 바꾸지 않습니다.
검증된 형식만 연결 후보로 등록하고 Windows 또는 Ubuntu의 시스템 UI에서 사용자가 직접
선택하도록 합니다. 설정 화면 진입을 기본 앱 적용 완료로 간주하지 않습니다.

## 확인과 연결 문서

루트 tests/의 frontend 정적 구조 검사 6개는 이 변경에서 통과했습니다.
테스트 소스는 이 UI 기능 브랜치에 포함하지 않으며 별도 검증 변경이 필요합니다.

```text
python3 -B -m unittest discover -s tests/frontend -p 'check_*.py' -v
```

정적 검사는 C# 컴파일, 실제 Windows/Ubuntu UI, 설치나 접근성 검증을 대신하지 않습니다.
전달받은 Windows 기존 회귀 62개 통과와 새 해제를 포함한 전체 회귀 실행 상태도 구분합니다.
이번 로컬 문서 갱신에서 C# 회귀 검사를 다시 실행하지 않았습니다.

- [UI와 공통 계약 연결](docs/UI_CORE_HANDOFF.md)
- [Ubuntu UI 선택과 검증 범위](docs/UBUNTU_UI.md)
- [로컬 압축파일 시작 인수](docs/LOCAL_ARCHIVE_OPEN.md)

Zipelo 자체 코드는 MIT이며 외부 구성요소의 라이선스는 별도입니다.
추가 외부 압축 엔진은 아직 채택하지 않았고 구성요소별 배포 고지도 별도 검토가 필요합니다.
프로젝트 기획과 지원 상태 문서는 담당 문서 브랜치에서 별도로 관리합니다.

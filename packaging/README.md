# 설치 패키지 준비 초안

현재 파일은 개발 검증 설치 초안이다. 설치 가능한 출시 파일을 만들거나 게시한 결과가 아니다.
`src/`와 `tests/`는 수정하지 않았다. 공유 브랜치 `docs/initial-project-plan`을 유지했다.
최신 Windows 게시 재검증과 실제 UI 결과를 이 초안에서 대신 인증하지 않는다.

## 입력과 중단 조건

[prepare.py](prepare.py)는 이미 게시하고 검증한 전체 산출물 폴더, 지원 형식 manifest,
검증 manifest를 입력받는다. SDK나 패키지를 내려받지 않으며 NSIS, dpkg, 설치 프로그램을
실행하지 않는다. 준비 모드는 새 출력 폴더에 설치 입력을 복사하고, 검사 전용 모드는 읽기만 한다.
현재 [support.draft.json](manifests/support.draft.json)은 등록 형식이 없고
[verification.draft.json](manifests/verification.draft.json)은 검증하지 않은 값이다.
이 값을 사실 확인 없이 true로 바꾸면 안 된다. manifest 자체가 서명된 검증 증명은 아니다.

필수 조건은 다음과 같다.

- support schema=1, 숫자 세 부분 version, releaseReady=false. 개발 초안만 허용한다.
- verification schema=1, platform은 win-x64 또는 linux-x64. validationPassed와 selfContained는 true,
  creationValidationEnabled는 false여야 한다. 생성 검증 빌드를 일반 사용자용 입력으로 받지 않는다.
- validatedSystems는 Windows에 windows-11-x64, Linux에 ubuntu-24.04-x64와 ubuntu-26.04-x64를
  모두 포함한다. 한 LTS 실행 결과를 다른 LTS의 검증으로 간주하지 않는다.
- checks의 ui_launch, worker_protocol, new_zip_publication, cancel_cleanup, original_preserved,
  license_review, dependency_review 각각 true가 필요하다. 환경, 도구 버전, 수행 명령,
  실제 결과와 미검증 범위를 기록한 근거 파일을 산출물 validation/ 아래에 넣고 evidence로 열거한다.
- files는 산출물 전체 파일의 POSIX 상대 경로를 키로 하고 SHA-256 소문자 64자 값을 갖는 객체다.
  추가 파일과 누락, 링크와 이름 충돌도 거부한다. 스냅샷 이후 바뀐 파일은 다시 검증해야 한다.
  준비 중 복사본 해시도 다시 확인한다. 실패한 출력은 완성물로 쓰지 않고 새 폴더에서 다시 준비한다.
- 루트 LICENSE와 THIRD_PARTY_NOTICES.txt가 있어야 한다. 앱 MIT 고지만으로 .NET, Avalonia,
  Skia와 다른 실제 포함 구성요소의 고지를 대체하지 않는다. 현재 저장소에는 준비된 배포 고지가 없다.
- 실제 프로젝트 이름인 Windows Zipelo.exe/Zipelo.dll, Ubuntu Zipelo.Ubuntu/Zipelo.Ubuntu.dll,
  고정 worker/Zipelo.Worker.exe 또는 worker/Zipelo.Worker와 대응 DLL, deps/runtimeconfig,
  Core/Runtime DLL을 요구한다. 앱과 worker 각각 coreclr/clrjit/hostfxr/hostpolicy와
  System.Private.CoreLib를 포함하며 runtimeconfig의 .NET 10 includedFrameworks를 확인한다.
  전체 파일의 실행 검증과 의존성 검토도 필요하다. 파일 존재만으로 독립 실행을 인증하지 않는다.
- Linux dependencies는 각 LTS에서 검토한 Debian 패키지 관계 목록이다. `name`,
  `name (>= version)`, `name | alternative`만 허용한다. 배포판별 ICU 패키지 이름 차이처럼
  대체 관계가 필요하면 실제 두 환경에서 충족되는지 확인한다. 임의 의존성을 자동 추정하지 않는다.

지원 형식은 기본적으로 빈 배열이다. 현재 허용 가능한 유일한 후속 항목은 extension=.zip,
mime=application/zip, validated=true, evidence=검증 근거 파일명이다. 형식 검증 외에
fileOpenArgumentsValidated=true도 필요하다. JAR, APK, APPX/MSIX, EPUB, Office 컨테이너 등
ZIP 계열이라는 이유로 앱 컨테이너를 등록하지 않는다. RAR/7z/tar 등의 등록도 현재 거부한다.
새 형식은 실제 검증과 설치 정책 검토 후 허용 목록 자체를 별도로 갱신해야 한다.

시작 인수 연결의 후속 구현은 [로컬 파일 열기 계약](../docs/LOCAL_ARCHIVE_OPEN.md)에 따른다.
두 UI에 단일 ZIP 절대 경로 연결 코드를 작성했지만 실제 OS 실행 검증은 아직 없다.
따라서 fileOpenArgumentsValidated=false와 빈 formats를 유지한다. 형식을 등록하려면
실제 실행 결과와 함께 fileOpenArgumentsContract=single-local-zip-path-v1을 기록해야 한다.
Windows는 따옴표로 감싼 %1 한 개, Ubuntu는 %f 한 개를 전달한다. %F나 셸 문자열로 바꾸지 않는다.

## Windows 사용자 범위 초안

[installer.nsi.in](windows/installer.nsi.in)은 NSIS 입력 템플릿이다. 토큰이 남은 상태에서는
컴파일하지 않는다. 준비기가 검증한 전체 파일마다 File 항목을 만들므로 필수 파일이 사라지면
NSIS도 컴파일을 중단한다. 설치는 RequestExecutionLevel user와 HKCU만 사용하며 위치는
LOCALAPPDATA/Programs/Zipelo-Draft다. SDK 설치나 외부 실행 파일 검색은 없다.
현재는 새 설치만 대상으로 하고 기존 초안이 있으면 먼저 제거하도록 중단한다.
실제 업그레이드와 실행 중 파일 교체는 아직 준비되지 않았다.

검증된 .zip 형식이 있을 때만 앱 소유 ProgID, OpenWithProgids와 RegisteredApplications
후보를 등록한다. 확장자의 기본값이나 UserChoice는 쓰지 않는다. 완료 화면의
[기본 앱 설정 열기]는 클릭했을 때만 `ms-settings:defaultapps`를 열고 [나중에]로 종료한다.
어느 버튼도 기본값을 직접 바꾸지 않는다. 설정 화면에 표시할 후보가 없을 수 있음을 유지한다.

제거는 설치 목록의 파일과 앱 소유 레지스트리 값만 지운다. 사용자 폴더를 재귀 삭제하지 않는다.
수정된 앱 폴더의 알 수 없는 파일은 남길 수 있다. 현재 프로필의 설정과 사용자가 고른 원본,
생성 ZIP은 삭제하지 않는다. 설치/제거 도중 실패, 실행 중 제거, 다른 앱과 공존은 검증 전이다.

## Ubuntu .deb 초안

[control.in](ubuntu/control.in), [zipelo.desktop](ubuntu/zipelo.desktop),
[launcher](ubuntu/zipelo), [SVG 아이콘](ubuntu/zipelo.svg)을 사용한다.
파일은 /usr/lib/zipelo에, 명확한 실행 진입점은 /usr/bin/zipelo에 둔다. launcher는 고정
앱 경로를 exec하며 인수를 그대로 전달한다. 작업자는 앱 기준 worker/에서 찾는다.
애플리케이션 식별자는 zipelo-draft.desktop이며 아이콘은 hicolor/scalable/apps/zipelo.svg다.
기존 ZIP MIME 형식 application/zip을 재사용한다. 별도의 MIME XML로 시스템 ZIP 인식을
재정의하지 않는다. 검증된 formats가 있을 때만 desktop에 MimeType과 Exec의 %f를 추가한다.
기본 초안은 파일 인수가 없는 앱 실행 항목뿐이다.

postinst/postrm과 xdg-mime default 호출은 없다. mimeapps.list와 사용자 기본 연결을 바꾸지 않는다.
Debian 패키지 관리자가 파일을 추적하고 제거하며 desktop-file-utils 및 shared-mime-info의
배포판 trigger가 캐시 갱신을 담당하는지 각 LTS에서 검증한다. 임의 캐시 디렉터리를 삭제하지 않는다.
아이콘 파일과 desktop 파일도 패키지 소유 파일로 제거된다. 사용자 설정과 ZIP은 유지한다.
이 .deb는 시스템 범위이며 실제 설치에는 OS의 관리자 승인 절차가 필요하다. 여기서 실행하지 않는다.

의존성은 각 ELF의 readelf -d 결과와 .NET 및 Skia의 동적 로딩 의존성을 검토하고 실제 실행으로
확인해야 한다. libX11/libICE/libSM/fontconfig, ICU, OpenSSL, C/C++ 런타임, 한글 폰트,
GNOME 포털/파일 선택 제공자 및 Wayland 세션의 XWayland 존재를 확인한다. ELF NEEDED만으로
모든 런타임 의존성이 확인됐다고 기록하지 않는다. 공유 .deb의 Depends가 두 LTS에서 모두
충족되는지 검증하며 정확한 패키지 이름은 검증 manifest에 남긴다. 런타임 self-contained는
이 OS 라이브러리까지 독립적으로 포함한다는 뜻이 아니다. SDK는 최종 사용자 의존성이 아니다.

## 도구와 다음 실행 경계

준비 기준 도구는 Python 3.14.4, Windows 컴파일러는 NSIS 3.13이다.
[NSIS 공식 다운로드](https://nsis.sourceforge.io/Download)와
[공식 라이선스](https://nsis.sourceforge.io/License)를 확인했다. zlib 압축기를 명시했으며
유료 설치 플러그인이나 자동 다운로드 기능을 사용하지 않는다. NSIS는 현재 설치되어 있지 않다.

Ubuntu 준비 기준은 현재 호스트의 dpkg/dpkg-deb 1.23.7ubuntu1,
desktop-file-utils 0.28-1build1, shared-mime-info 2.4-5build3,
binutils 2.46-3ubuntu2다. 이는 빌드 호스트 기준이며 두 LTS의 실행 의존성 버전 선언이 아니다.
공식 도구/계약은 [dpkg-deb](https://manpages.debian.org/bookworm/dpkg/dpkg-deb.1.en.html),
[Desktop Entry](https://specifications.freedesktop.org/desktop-entry/latest/exec-variables.html),
[Shared MIME-info](https://specifications.freedesktop.org/shared-mime-info/latest/),
[Windows 기본 앱 설정](https://learn.microsoft.com/en-us/windows/apps/develop/launch/launch-default-apps-settings)에 따른다.

검증 자료가 준비되면 먼저 prepare.py의 --check-only로 검사한다. 일반 준비 실행은
새 --output 폴더를 만들고 Linux 패키지 내부 파일 모드를 설정하므로 현재 수행하지 않았다.
Windows는 생성된 installer.nsi를 그 출력 폴더에서 NSIS 3.13으로 컴파일한다.
Ubuntu는 생성 desktop을 검사하고 `dpkg-deb --root-owner-group --build`로 deb-root를 묶는
후속 절차가 필요하다. 이 명령은 여기서 실행하지 않았다. 준비 후 입력이 바뀌면 다시 준비한다.
실제 Maintainer 이름과 이메일은 사용자 또는 관리자에게 확정받고 --maintainer로 전달한다.
이메일이나 서명자 정보를 임의로 넣지 않는다. 서명과 배포 승인도 별도다.

이번 실제 확인은 Python 구문, JSON/XML, desktop-file-validate, sh -n과 읽기 전용 차단 동작이다.
컴파일, 패키지 생성, 설치, 기본 연결 변경, 권한 변경과 서명은 하지 않았다.
깨끗한 SDK 없는 Windows와 Ubuntu 각 LTS의 설치/실행/파일 연결, 한글 및 공백 경로,
동의/나중에, 취소와 오류, 제거/재설치, 업데이트와 캐시 정리는 모두 실제 검증이 남아 있다.
self-contained 앱과 worker의 런타임 중복 용량도 아직 측정하지 않았다.

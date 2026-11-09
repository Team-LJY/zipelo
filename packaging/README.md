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
현재는 새 설치만 대상으로 하며 기존 대상이나 전용 등록이 있으면 보존하고 중단한다.
덮어쓰기 업그레이드와 실행 중 파일 교체는 지원하지 않는다. 아래 실패 처리 계약을 따른다.

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

## Windows 실패 처리와 수용 기준

현재 새 설치만 지원하며 고정 사용자 경로를 사용한다. /D 경로 재지정은 적용하지 않는다.
Uninstall.exe 존재를 설치 신뢰 근거로 사용하지 않는다. CreateDirectoryW의 새 디렉터리 생성
성공으로 설치 대상과 전용 시작 메뉴 폴더를 예약하며, 기존 대상은 빈 폴더, 다른 파일,
이전 Zipelo 설치, 링크 여부와 관계없이 거절한다. 기존 항목을 자동 삭제하거나 덮어쓰지 않는다.
기존 전용 등록 키는 RegCreateKeyExW의 생성 결과로 구별하고 이미 있으면 변경하지 않는다.
공유 키의 Zipelo 전용 값도 모든 레지스트리 자료형에 대해 존재 시 거절한다.
업데이트는 검증된 설치 목록/파일 무결성, 버전/경로, 실행 중 앱 종료, 실패 시 복구 계약과
실제 이전 버전별 검증을 갖춘 별도 절차가 준비된 뒤 지원한다. 현재 설치 초안은 업데이트하지 않는다.
설치 기록 INI는 실수로 이동한 제거 프로그램의 경로/버전 확인용이며 서명이나 위변조 증명이 아니다.

SetOutPath, File, WriteUninstaller, 설치 기록, CreateShortcut와 각 WriteRegStr 뒤 오류를
검사한다. 파일 복사의 무시 버튼을 금지하고 재시도/취소만 허용한다. 예약/등록 실패도 설치
중단으로 처리한다. 모든 필수 단계가 성공해야 InstallComplete가 설정되고 완료 화면 및
기본 앱 설정 버튼에 진입한다. 설치 실패와 취소는 종료 코드 1을 반환한다.

실패 정리는 이번 실행의 생성/쓰기 시도 플래그가 있는 명시적 파일, 전용 값, 빈 디렉터리만
대상으로 한다. 복사 중 생긴 부분 파일도 시도 플래그로 추적한다. 재귀 폴더/레지스트리 삭제,
와일드카드 파일 삭제, 재부팅 뒤 강제 삭제는 사용하지 않는다. 공유 키 자체와 사용자 원본은
삭제하지 않는다. 생성한 부모 Programs 폴더와 공유 등록 키의 빈 상위 경로는 보존할 수 있다.
정리 중 접근 거부/사용 중 파일/알 수 없는 잔류 항목이 있으면 정리 미완료를 안내하고,
앱 실행이나 폴더 전체 삭제 대신 지원 문의를 요청한다. 정리가 끝났을 때만 공간/권한 확인 후
설치 프로그램 재실행을 안내한다. 실패 단계는 사람이 이해할 수 있는 이름으로 표시한다.

설치/제거는 같은 세션의 명명된 mutex로 중복 실행을 거절한다. 다른 프로그램이 설치 중
같은 전용 항목을 임의로 교체하거나 같은 사용자 계정이 장악된 상황까지 방어한다고 주장하지
않는다. 경로 기반 NSIS 작업은 링크/경로 교체 경합에 대한 핸들 기반 보장을 제공하지 않는다.
전원 중단/강제 종료에는 콜백 정리가 실행되지 않을 수 있다. 잔류 경로는 다음 설치에서 거절하고
자동 소유권 판정이나 삭제로 우회하지 않는다. 이러한 조건은 실제 OS 검증 및 출시 전 검토가 남았다.

아래 항목은 실제 Windows 실패 주입 수용 기준이며 아직 수행하지 않았다.

- 깨끗한 설치에서 전체 payload, worker, 제거 프로그램, 설치 기록, 바로가기와 등록이 완료된
  경우에만 완료 화면을 표시한다. 빈 formats는 연결 등록 없이 완료되어야 한다.
- 시작 전 취소는 파일/등록을 변경하지 않는다. 복사 재시도 성공은 정상적으로 계속하며,
  복사 취소는 실패 정리와 종료 코드 1로 끝난다. 완료 화면의 나중에는 정상 종료한다.
- 첫 파일/중간 파일/마지막 파일의 디스크 부족과 접근 거부, 제거 프로그램/INI 쓰기 실패,
  바로가기 실패, 각 레지스트리 쓰기 실패를 따로 주입한다. 실패 뒤 다음 단계와 성공 화면/
  설정 버튼으로 진행하지 않고 시도한 부분 파일, 바로가기와 전용 등록을 정리해야 한다.
- 동일 설치 프로그램의 동시 실행은 한 개만 진행한다. 실패 정리가 끝난 뒤 재실행은 성공해야
  하며, 정리가 불가능하면 구체적인 실패 단계와 미완료 안내를 남기고 다음 실행도 안전하게 거절한다.
- 기존 대상의 임의 sentinel, 빈 폴더, Uninstall.exe만 있는 폴더, 이전 설치, junction,
  전용 바로가기 폴더, 기존 전용 등록과 공유 값은 내용/해시/자료형까지 보존해야 한다.
- 정리 도중 파일 잠금과 등록 삭제 거부를 주입해 거짓 정리 완료를 표시하지 않아야 한다.
  미등록 sentinel과 사용자 ZIP을 추가한 경우 이를 지우지 않고 비어 있지 않은 디렉터리를 남긴다.
- 설치 후 정상 제거와 실패한 제거, 이동한 제거 프로그램, 변조된 설치 기록, 전원 중단 잔류,
  링크/이름 교체 경합은 별도 검증한다. INI만으로 안전한 업데이트나 악성 경합 방어를 인증하지 않는다.

근거: [NSIS 오류/콜백 계약](https://nsis.sourceforge.io/Docs/Chapter4.html),
[새 디렉터리 생성](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createdirectoryw),
[레지스트리 키 생성 결과](https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regcreatekeyexw).
이번 변경은 소스/생성 결과의 정적 확인이며 NSIS 컴파일, 실제 설치/제거와 레지스트리 변경은 하지 않았다.


## Windows 제거 실패와 재시도

제거는 payload 전체의 명시적 파일 삭제, 바로가기/빈 하위 폴더/연결 후보 정리,
제거 등록 정리, 설치표식과 제거 실행파일 삭제 순서다. payload 단계는 모든 파일의 오류를
누적한 뒤 판정하며 잠금/권한 오류가 하나라도 남으면 등록, 표식과 Uninstall.exe를 보존한다.
이미 없는 파일과 등록 값은 정상으로 취급해 부분 제거 뒤 같은 실행파일로 재시도할 수 있다.
자기 manifest 밖 파일을 열거해 지우거나 재귀 삭제하지 않는다. 수정된 파일을 내용 기준으로
사용자 파일과 구별하는 기능은 없으므로 설치 manifest의 경로를 사용자가 덮어쓴 경우에도
그 경로는 제거 대상이다. 앱 폴더에 사용자 자료를 저장하도록 안내하지 않는다.

연결 후보 값 삭제의 접근 거부/닫기 실패도 누적하고 다음 최종 정리로 진행하지 않는다.
비어 있지 않은 키와 디렉터리는 사용자/다른 앱 자료일 수 있어 보존한다. 빈 키인지 조회하지
못하거나 삭제 권한이 없으면 오류로 처리한다. 제거 등록 일부만 삭제된 경우 알려진 자기
DisplayName/DisplayVersion/UninstallString 복원을 시도하고 복원 오류도 누적한다.
이때 원래 설치표식과 제거 실행파일은 아직 삭제하지 않는다. 등록 복원에 실패해도 직접
Uninstall.exe를 다시 실행할 수 있다.

설치표식 삭제 실패에는 제거 실행파일을 보존하고 제거 등록을 복원한다. 자기 실행파일 삭제가
실패하면 표식과 등록을 복원한다. 표식 재작성까지 실패했을 때는 고정 사용자 설치 경로,
정확한 버전과 제거 명령 등록이 일치하는 경우에만 재시도를 허용한다. 표식/등록 복원 권한이
동시에 거부되는 등 복합 장애는 자동 복구를 보장하지 않으며 상세 실패와 지원 문의가 필요하다.
전원 중단, 강제 종료와 악성 경로/등록 교체에 대한 원자적 제거를 구현했다고 주장하지 않는다.

실패는 상세 보기에 항목별 이름을 남기고 오류 건수와 첫 실패를 안내하며 종료 코드 1로 끝난다.
앱과 worker를 닫고 권한을 확인한 뒤 같은 Uninstall.exe를 다시 실행하도록 안내한다.
최종 파일/등록은 모두 없지만 빈 최상위 폴더 삭제만 실패하면 별도 미완료 안내와 종료 코드 1을
사용한다. 이 경우 제거 실행파일 재실행 대신 상세 폴더와 지원 문의를 안내한다.
비어 있지 않은 컨테이너를 보존한 정상 제거는 사용자 자료를 남겼음을 명시한다.
강제 재부팅 삭제나 자동 권한 변경으로 실패를 우회하지 않는다.

정적 수용 기준과 이번 확인 범위:

- 등록 없는 입력과 ZIP 후보 입력의 생성 NSIS에서 payload 삭제가 등록보다 먼저 나오고,
  각 단계의 누적 오류 판정이 표식/자기 삭제보다 앞에 있어야 한다.
- 모든 payload는 정확한 manifest 경로와 삭제 오류 수집을 사용해야 한다. 연결 후보와
  제거 등록은 별도 생성 블록이어야 하며 제거 등록 복원 명령에도 오류 수집이 있어야 한다.
- 없는 파일/값을 허용하는 재시도 계약, 표식/자기 삭제 실패의 복원 분기와 실패 종료를
  소스에서 확인한다. 재귀/와일드카드 삭제와 UserChoice 변경은 없어야 한다.
- Python 구문 및 메모리 생성 결과의 구조 확인은 NSIS 컴파일/Win32 실행 검증을 대신하지 않는다.

실제 Windows 수용 기준은 모두 미실행이다.

- 첫/중간/마지막 payload 잠금 및 접근 거부를 각각 주입한다. 다른 파일 삭제는 시도하되
  오류를 누적하고 제거 등록/표식/실행파일은 보존해야 한다. 잠금 해제 후 재실행은 남은
  파일만 제거하고 정상 완료해야 한다.
- 각 연결 후보/제거 등록 값의 삭제 거부와 일부 삭제 후 실패를 주입한다. 제어판 복원 결과와
  직접 실행 재시도를 구별하고, 복원 권한 거부도 실패 목록에서 숨기지 않아야 한다.
- 표식 삭제 거부, 자기 실행파일 잠금, 표식 재작성 거부와 복합 등록 복원 실패를 확인한다.
  정상 재시도 가능 여부와 지원이 필요한 복합 장애를 실제 결과대로 기록해야 한다.
- 정상 제거는 payload/바로가기/자기 등록/표식/실행파일이 없어야 한다. manifest 밖 sentinel과
  사용자 ZIP은 내용/해시를 보존하고 디렉터리/키가 비어 있지 않으면 남겨야 한다.
- 빈 디렉터리 삭제 권한 거부, 알 수 없는 등록 값, 제거 중 취소/전원 중단과 경합을 구분한다.
  마지막 폴더만 남은 미완료를 프로그램까지 남은 부분 제거와 같은 상태로 표시하지 않는다.

이번에는 NSIS 도구 설치/컴파일, 실제 설치/제거/레지스트리 변경 및 실패 주입을 하지 않았다.

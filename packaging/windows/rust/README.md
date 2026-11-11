# Rust Windows 개발판 설치 파일

NSIS를 사용해 Linux에서 Windows x64 설치 EXE를 생성합니다. Rust 앱과 별도 작업자를
교차 빌드하며 가짜 작업자 기능은 활성화하지 않습니다.

```sh
python3 packaging/windows/rust/build.py \
    --app-manifest /path/to/app/experiments/rust-ui-memory-app/Cargo.toml \
    --worker-manifest /path/to/worker/experiments/rust-archive-worker/Cargo.toml \
    --target-dir /path/to/cargo-target \
    --output /path/to/new-installer-output
```

Cargo 1.99.1, Windows GNU Rust target, MinGW-w64와 makensis가 필요합니다.
도구는 자동 설치하지 않습니다. 기본 profile은 release이고 개발 검증에는 --profile dev를 사용할 수 있습니다.
출력 폴더는 새 경로여야 하며 기존 파일을 덮어쓰지 않습니다. 입력 저장소는 커밋된 상태여야 합니다. 캐시는 실행 파일과 소스 커밋별로 분리하며 빌드 중 소스 변경을 거부합니다.
앱과 작업자 PR이 병합되기 전에는 각 PR의 별도 소스 경로를 인자로 지정합니다.
빌드 정보에는 소스 커밋, 실행 파일 SHA-256, target, profile, 기능 목록을 기록합니다.

설치 파일을 더블클릭하면 현재 사용자의 LocalAppData/Programs/Zipelo-Dev에
Zipelo.exe, zipelo-archive-worker.exe, MIT LICENSE, BUILD-INFO.json과 제거 프로그램을 설치합니다.
기존 폴더나 등록이 있으면 중단하며 덮어쓰기 업데이트는 지원하지 않습니다.
시작 메뉴에 Zipelo 개발판 바로가기를 추가합니다.

마지막 화면의 [적용]을 누르면 HKCU의 ZipeloDev.Archive ProgID와 RegisteredApplications에
.zip, .tar, .gz, .tgz 연결 후보를 등록하고 ms-settings:defaultapps를 엽니다.
.tar.gz는 마지막 확장자 .gz로 연결됩니다. 기본 앱은 사용자가 Windows 설정에서 직접 선택합니다.
JAR, RAR, 7z는 등록하지 않습니다. [마침]만 누르면 연결 후보를 등록하지 않습니다.
관리자 권한, HKLM, UserChoice 변경을 사용하지 않습니다.
제거 시 알려진 앱 파일과 등록 값만 정리하며 폴더 전체를 재귀 삭제하지 않습니다.
실행 파일 삭제 실패 시 등록과 제거 프로그램을 보존하여 재시도할 수 있습니다.

현재는 개발 검증용입니다. PR62 화면의 실제 작업자 연결, Windows에서의 설치/제거와 설정 실행,
지원 형식별 실제 파일 열기, 재설치/권한 실패/잠긴 파일 검증, 포함 의존성의 배포 고지 검토는 남아 있습니다.
이번 EXE 생성만으로 해당 항목을 인증하거나 출시하지 않습니다.
Windows 실행 검증 전에는 README 다운로드 버튼이나 release에 배포하지 않습니다.

검증은 저장소 루트에서 python3 tests/ci/run_rust.py local을 실행하고 위 명령으로
실제 앱과 작업자를 교차 빌드하여 NSIS를 컴파일합니다. BUILD-INFO.json과 설치 EXE의
해시를 함께 확인합니다. NSIS 부트스트랩은 32비트 PE일 수 있으며 설치되는 두 앱은 x64 PE입니다.

연결 등록은 [Microsoft 기본 프로그램 문서](https://learn.microsoft.com/en-us/windows/win32/shell/default-programs)의
Capabilities/RegisteredApplications 구조를 사용하며,
[Windows 기본 앱 설정 열기](https://learn.microsoft.com/en-us/windows/apps/develop/launch/launch-default-apps-settings)의
ms-settings URI를 사용합니다.

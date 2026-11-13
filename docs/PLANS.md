# Zipelo 기획서 (v0.1)

누구나 쉽게 설치해 쓰는 가볍고 빠른 MIT 오픈소스 압축 프로그램을 만든다. 이 문서는 첫 출시판(v0.1)의 범위와 완료 기준을 정한다. 요구사항은 구현이나 검증 완료를 뜻하지 않는다.

## 1. 확정된 결정

- 기술 스택은 Rust 하나로 한다. C# 구현은 중단했으며, 로컬 백업 후 PR103(f1d9fa5)에서 소스와 전용 시험 및 .NET 설정을 제거했다. C# 재개는 v0.1 이후 계획에도 포함하지 않는다.
- 압축 처리는 별도 작업자 프로세스에서 한다. 앱(UI)은 작업자와 요청/응답 메시지로 통신한다.
- 외부 라이브러리는 라이선스와 의존성을 줄이기 위해 최소로 쓴다. 압축 형식 처리는 직접 구현한다.
- 예외: AES 암호화와 암호 해시는 RustCrypto(MIT/Apache, 순수 Rust)를 쓴다. 해제용 zlib-rs(Zlib 라이선스), UI의 eframe wgpu와 운영체제 기본 파일 창용 rfd도 승인된 예외로 사용한다.
- RAR는 라이선스 문제로 v0.1에서 제외한다.
- 설치 마지막 단계의 [적용]은 Zipelo를 연결 프로그램으로 등록하고 Windows 기본 앱 설정 화면을 연다. Windows는 앱이 기본 프로그램을 강제로 바꾸지 못하게 막는다. Ubuntu는 바로 적용한다.
- 자체 코드는 MIT 라이선스다.
- 화면은 main의 experiments/rust-ui-memory-app을 기준으로 한다.

## 2. v0.1 범위

포함:

- 해제: ZIP(Store/Deflate), TAR, GZ, TAR.GZ. 이어서 7z(LZMA).
- 생성: ZIP.
- 암호가 필요한 압축파일이면 작업자가 앱에 암호 요청을 보내고 암호 입력창을 띄운다. 암호 ZIP 생성과 해제는 완료 기준에 포함한다.
- 파일 더블클릭 또는 끌어다 놓기로 열기, 목록 보기, 풀기, ZIP 만들기, 진행률과 취소.
- 안전한 해제: 지정 폴더 밖에 쓰지 않고 기존 파일을 덮어쓰지 않는다. 압축 폭탄과 틀린 암호를 안전하게 처리한다.
- 배포: Windows x64 설치 파일, Ubuntu .deb, README 다운로드 버튼, 설치 마지막의 기본 앱 설정 단계.

보류(v0.1 이후):

- RAR, 손상 파일 복구, 미리보기, 분할 압축, 7z 생성, TAR.BZ2/XZ.
- JAR와 앱/문서용 컨테이너는 계속 지원 대상과 기본 연결 대상에서 제외한다.

## 3. 아키텍처

- 앱: Rust(eframe) 화면. 압축 파싱을 앱에서 직접 하지 않는다.
- 작업자: 별도 Rust 실행 파일. 목록, 해제, ZIP 생성, 암호 요청, 취소를 처리하고 진행, 완료, 실패 이벤트를 보낸다.
- 통신 형식은 backend와 frontend 담당이 합의한 계약을 따른다. 작업 ID, 취소 후 종료 확인, 부분 출력 보고를 포함한다.
- 실제 구현은 experiments/rust-ui-memory-app과 experiments/rust-archive-worker를 기준으로 한다. UI 및 작업자 시험은 tests/ 아래에 둔다.

## 4. 담당과 검증 기준

- backend는 Rust 작업자와 압축 처리, frontend는 Rust 앱과 UI, bugfix는 결함 검토와 회귀 검증, planning은 요구사항과 문서를 담당한다.
- 현재 기능과 알려진 한계는 [릴리스 노트](release-notes-v0.1.md), 화면 동작은 [UI 설계](ui-design-v0.1.md)를 따른다. 과거 C# 시험 수와 미구현 상태를 현재 Rust 상태로 재사용하지 않는다.
- 빌드와 시험은 로컬에서 `python3 tests/ci/run_rust.py local`로 돌리고 결과를 PR 본문에 적는다. GitHub Actions는 수동 실행 Windows 확인용만 쓴다.
- 작업 사본, 빌드 캐시와 임시 파일은 프로젝트의 tests/work/ 아래에 둔다. 각 플랫폼의 실제 실행과 릴리스 게시 여부는 빌드 통과와 별도로 확인한다.

## 5. 완료 기준

- 새 PC에서 설치 파일을 더블클릭해 설치하고 기본 앱 단계까지 진행된다.
- zip, 7z, tar.gz를 더블클릭해 열고 풀 수 있다.
- 암호 ZIP을 만들고 풀 수 있다.
- Linux 시험과 Windows 교차 빌드가 로컬 실행기로 통과한다. Windows 실제 실행은 출시 전에 수동으로 확인한다.
- 폴더 밖 경로, 압축 폭탄, 틀린 암호 회귀 시험이 통과한다.
- 지원하지 않는 형식을 지원한다고 안내하지 않는다. 실제 검증 전에는 지원 완료로 표시하지 않는다.

## 6. 승인이 필요한 항목

- 릴리스 게시, 서명 인증서 같은 비용, 공개 설정 변경은 이준영 님 승인 후 진행한다.
- 새 외부 라이브러리 추가는 위 예외 외에는 사전 확인을 받는다.

## 7. C# 중단과 과거 검증 기록

C# 코드는 v0.1의 빌드, 실행과 배포에 사용하지 않는다. 삭제 전 소스는 git 이력의 `ae91536`에서 확인할 수 있고, linux-desktop-2의 `tests/work/archive/csharp-backup-ae91536/`에도 백업되어 있다. 과거 C# 문서는 설계 및 검증 이력으로 보존하며, 그 문서의 명령을 현재 main의 개발 절차로 사용하지 않는다. .NET SDK, WPF와 Avalonia는 현재 제품을 빌드하기 위한 요구사항이 아니다.

다음 두 정적 검사는 C# 파일 해시를 고정한 과거 기준 검사다.

- `tests/rust-memory-ustar-reader/static_check.py`
- `tests/rust-memory-gzip-reader/static_check.py`

C# 제거 뒤 main에서는 삭제된 경로를 읽다가 실패한다. 이 검사는 정식 검증 `tests/ci/run_rust.py local`과 패키징의 실행 대상이 아니며, 남아 있는 실패를 현재 검사 통과로 표시하지 않는다. 검사와 연결된 해시 기록은 변경하지 않고 보존한다.

당시 결과를 재확인하려면 현재 작업 공간을 되돌리지 말고 `ae91536`의 별도 작업 사본을 프로젝트의 `tests/work/` 아래에 준비한 뒤 그 사본의 루트에서 실행한다. `ae91536`은 지정한 C# 제거 전 재현 스냅샷이며, 각 검사의 계약과 SHA256SUMS도 같은 스냅샷을 사용한다.

```bash
python3 -B tests/rust-memory-ustar-reader/static_check.py
python3 -B tests/rust-memory-gzip-reader/static_check.py \
    --source-head e1ffd7bbffc358b6e53358682a93bf3ce075e892 \
    --test-manifest-sha256 "$(sha256sum tests/rust-memory-gzip-reader/SHA256SUMS | cut -d ' ' -f 1)"
```

이 재현은 과거 정적 기록의 확인이며 현재 Rust 제품의 빌드, 실행 또는 출시 검증을 대신하지 않는다.

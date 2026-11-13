# 로컬 압축파일 시작 인수 계약

> 과거 C# 구현 기록: C#은 v0.1에서 중단하고 Rust로 대체했다. PR103(f1d9fa5)에서 C# 소스와 전용 시험 및 .NET 설정을 제거했다. 아래 본문의 "현재", 미구현 상태, API와 검증 결과는 작성 당시 C# 스냅샷에 관한 내용이며, 현재 Rust 제품의 지원 범위나 검증 상태를 뜻하지 않는다. 삭제된 경로와 실행 명령은 현재 main에서 사용할 수 없다.
>
> 현재 기준은 [기획서](PLANS.md), [UI 설계](ui-design-v0.1.md), [릴리스 노트](release-notes-v0.1.md)다. 앱은 [Rust UI](../experiments/rust-ui-memory-app/), 압축 처리는 [Rust 작업자](../experiments/rust-archive-worker/README.md)를 사용한다. 과거 검사 재현과 소스 보존 안내는 [기획서의 과거 기록](PLANS.md#7-c-중단과-과거-검증-기록)을 따른다.

## 보존된 당시 기록

계약 식별자는 `single-local-zip-path-v1`이다. 실제 OS 실행 검증 전의 UI 연결 구현이다.
일반 사용자용 형식 등록이나 설치 검증을 완료했다는 뜻이 아니다.

## 입력

- 인수 0개는 빈 작업 화면만 연다.
- 인수 1개는 옵션 없는 로컬 ZIP 파일의 절대 경로다. .zip 확장자는 대소문자를 구분하지 않는다.
- 인수 여러 개, 알 수 없는 옵션, 빈 경로, 상대 경로, URI, Windows UNC/장치 경로,
  존재하지 않거나 접근할 수 없는 파일과 미지원 확장자는 화면에 이유를 표시한다.
- JAR는 별도 제외 안내를 표시한다. 앱 컨테이너를 ZIP 계열이라는 이유로 연결 등록하지 않는다.
- `--open`, `--`, `--help` 등 옵션은 정의하지 않았다. 경로가 옵션으로 재해석되지 않도록
  파일 연결은 항상 완전한 절대 경로 하나를 전달한다. 따옴표는 OS 인수 구분용이며
  앱이 받은 문자열에서 임의로 따옴표를 제거하거나 URI를 경로로 바꾸지 않는다.

Windows 예시는 `Zipelo.exe "C:\Users\user\Documents\한글 파일.zip"`,
Ubuntu 예시는 `/usr/bin/zipelo "/home/user/한글 파일.zip"`이다.
이는 실행한 명령 기록이 아니다. Windows ProgID의 명령은 따옴표로 감싼 실행 파일과 %1,
Ubuntu의 검증 후 desktop Exec는 `/usr/bin/zipelo %f`다. %F는 사용하지 않는다.
Ubuntu launcher는 고정 `/usr/lib/zipelo/Zipelo.Ubuntu`를 exec하며 `"$@"`로 전달한다.
파일명이나 경로를 eval, 셸 명령 문자열 또는 실행 파일 검색 경로로 조합하지 않는다.

## 창 준비와 공통 경로

[ArchiveOpenArguments](https://github.com/Team-LJY/zipelo/blob/ae91536/src/Zipelo.Desktop/ArchiveOpenArguments.cs)는 UI 전용 공유 소스이며
Ubuntu가 링크한다. Core/Runtime/Worker 공개 계약은 바꾸지 않았다.
파일시스템의 실제 매체 종류를 증명하는 계층은 아니다. 로컬 경로가 가리키는 마운트와
읽기 권한은 OS 정책에 따르며 문자열 검사만으로 모든 I/O 지연이나 링크 교체를 막았다고
설명하지 않는다. 파일 내용은 기존 런타임의 한도와 분리 작업자로 처리한다.

WPF는 ContentRendered 이후, Avalonia는 Opened 이후 시작 인수를 한 번만 처리한다.
이벤트가 반복되어도 같은 작업을 다시 실행하지 않는다. Windows는 SelectAndRun → RunJob →
workspace List, Ubuntu는 OpenArchiveCoreAsync → workspace.ListAsync로 일반 파일 선택과
같은 결과/취소/화면 복구 처리를 사용한다. Ubuntu의 일반 파일 선택은 계속 제공자의 스트림을
사용하며 포털 URI를 경로로 추측하지 않는다. 시작 경로만 읽기 전용 FileStream으로 연다.

파일 선택 또는 작업이 진행 중이면 시작 요청을 추가로 실행하지 않고 상태 안내를 표시한다.
목록은 성공한 결과에만 교체한다. 오류/취소 시 기존 목록과 생성 선택을 보존한다.
잘못된 인수 때문에 앱을 즉시 종료하지 않고 사용자가 파일 열기를 다시 선택할 수 있게 한다.
창을 닫는 동안의 진행 중 작업 처리는 기존 취소/종료 대기 규칙을 유지한다.
단일 인스턴스 IPC나 기존 프로세스에 파일을 전달하는 기능은 구현하지 않았다.
파일 연결을 다시 실행하면 별도 프로세스와 창으로 시작하는 계약이다.

## 등록 차단과 검증

packaging의 fileOpenArgumentsValidated=false, 빈 formats, 전체 실행 검증 차단은 유지했다.
코드가 존재한다는 이유로 값을 true로 바꾸지 않는다. 등록 시 계약 식별자도 정확히 일치해야 한다.
생성의 개발 검증 플래그와 해제 비활성은 바꾸지 않았다.
ZipCreationWorkflow.cs의 기존 읽기 전용 메모리 스트림 수정도 보존했다.

실제 검증 담당자는 Windows 11 x64와 Ubuntu 24.04/26.04 LTS x64 각각에서
0개/1개/여러 개 인수, 알 수 없는 옵션, 한글/공백/셸 특수문자 이름, 대문자 ZIP,
JAR와 미지원 형식, 없는 파일과 읽기 거부, 손상 ZIP과 예산 초과,
작업 중 중복 요청과 취소 후 재열기, worker 누락/프로토콜 실패 후 정상 복귀를 확인해야 한다.
유효 경로의 파일도 확인과 실제 읽기 사이에 삭제되거나 바뀔 수 있으므로 열기 실패 처리를 확인한다.
Ubuntu는 X11과 Wayland/XWayland 및 실제 파일 관리자 %f 전달을 구분한다.
패키지 진입 명령과 실행 파일 직접 호출을 각각 검증하며 실제 기본 앱 변경은 별도 승인 범위다.

이번에는 기존 frontend 정적 구조 검사와 XML, 이벤트/컨트롤, 참조/서식 검사를 수행했다.
SDK 부재로 컴파일하지 않았으며 실제 두 OS의 시작/파일 연결 실행도 하지 않았다.
Core/Runtime/Worker/tests, 공유 브랜치와 기존 생성 스트림 패치는 수정하지 않았다.

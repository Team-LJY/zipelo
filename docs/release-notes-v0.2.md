# Zipelo v0.2.1 릴리스 노트

v0.2.1은 v0.1.0 다음으로 게시하는 버전입니다. 내부 빌드였던 0.2.0은 게시하지 않았습니다. 이 문서는 main `a78fa3d`(PR152 병합과 버전 0.2.1 통일)까지의 변경을 다룹니다.

## 새로 할 수 있는 일

- **파일 내용 미리보기:** 파일 하나를 고르고 [미리보기]를 누르면 UTF-8 텍스트(`.txt`, `.md`, `.log`, `.csv`, `.json`)와 정적 PNG/JPEG를 읽기 전용으로 봅니다. 텍스트는 1 MiB, 이미지 파일은 8 MiB, 가로와 세로 각각 8,192픽셀, 전체 1,600만 픽셀까지입니다. 큰 작업 허용으로 이 한도를 늘릴 수는 없습니다([PR125](https://github.com/Team-LJY/zipelo/pull/125)).
- **압축파일 안 파일 바로 열기:** 목록에서 파일을 두 번 누르거나 Enter를 누르면 그 파일만 풀어서 기본 프로그램으로 엽니다. 실행되거나 설치될 수 있는 파일은 열지 않고 안내만 합니다. 제외하는 형식은 운영체제마다 다르며, Windows에서는 `.js`, `.ps1`, `.bat` 같은 스크립트도 제외합니다. Linux에서는 스크립트를 편집기로 열 수 있습니다([PR144](https://github.com/Team-LJY/zipelo/pull/144)).
- **끌어서 풀기:** 목록에서 고른 파일과 폴더를 창 밖으로 끌어 파일 관리자나 브라우저의 파일 올리기 영역에 놓으면 그 자리에 풀립니다. 큰 항목은 다 풀릴 때까지 기다렸다가 넘기고, Windows에서는 이때 진행률 창이 나타나며 취소할 수 있습니다. 끄는 도중 Esc를 누르거나 Zipelo 창 위에 놓으면 끌기와 풀기가 함께 취소됩니다. 이름이 겹칠 때의 처리는 받는 프로그램이 정합니다([PR152](https://github.com/Team-LJY/zipelo/pull/152), [PR153](https://github.com/Team-LJY/zipelo/pull/153), [PR155](https://github.com/Team-LJY/zipelo/pull/155)).
- **오른쪽 클릭으로 골라 풀기:** "압축 파일명으로 새 폴더를 만들어 압축 풀기"는 `이름`, `이름 (1)`처럼 비어 있는 폴더에, "특정 폴더에 압축 풀기"는 고른 폴더에 선택한 항목만 풉니다. 마우스 뒤로 버튼은 상위 폴더로 이동합니다([PR144](https://github.com/Team-LJY/zipelo/pull/144)).
- **찾기:** 압축파일을 연 뒤 Ctrl+F는 지금 폴더 아래 모든 하위 폴더에서 이름으로, Ctrl+Shift+F는 압축파일 전체에서 경로로 찾습니다. 결과를 두 번 누르거나 Enter를 누르면 그 위치로 이동합니다([PR152](https://github.com/Team-LJY/zipelo/pull/152)).
- **Esc로 닫기:** 찾기, 작업 결과, 만들기 같은 창이 떠 있으면 그 창을 닫고, 아무 창도 없으면 프로그램을 끝냅니다. 목록에 선택이 있으면 첫 Esc는 선택만 풉니다. 진행 중인 작업은 [취소] 버튼을 쓰세요([PR152](https://github.com/Team-LJY/zipelo/pull/152)).
- **7z 만들기:** 만들기 화면에서 ZIP 또는 7z를 고릅니다. 7z는 LZMA2 방식이며 한글 이름, 빈 파일과 폴더를 보존합니다. 암호가 필요하면 ZIP을 고르세요([PR127](https://github.com/Team-LJY/zipelo/pull/127)).
- **나눠서 7z 만들기:** 7z를 고르고 [쪼갤 용량 설정]을 켠 뒤 크기(1 MB 이상, MB/GB)를 넣으면 `이름.7z.001`, `이름.7z.002`처럼 나눠 저장합니다. 최대 999개이며 같은 이름의 조각이 이미 있으면 만들지 않습니다. 실패하거나 취소하면 만든 조각을 모두 지웁니다([PR152](https://github.com/Team-LJY/zipelo/pull/152)).
- **나뉜 7z와 암호 7z 풀기:** 모든 조각을 같은 폴더에 두고 `.7z.001`을 열면 목록 보기와 풀기를 할 수 있습니다. 조각이 빠졌으면 필요한 파일 이름을 알려 줍니다. `7z a -p`로 만든 AES-256 암호 7z도 풀 수 있고, 파일 이름까지 암호화한 7z는 목록을 읽을 때 암호를 묻습니다. 틀린 암호는 세 번까지 다시 묻고 잘못 풀린 파일을 남기지 않습니다([PR126](https://github.com/Team-LJY/zipelo/pull/126), [PR152](https://github.com/Team-LJY/zipelo/pull/152)).
- **손상된 ZIP 복구:** 4 GiB 이하 ZIP에서 검증할 수 있는 항목만 골라 새 ZIP으로 저장합니다. 원본은 고치지 않으며 전체 복구, 일부 복구와 복구 불가를 구분해 알려 줍니다. 폴더 항목 앞부분의 크기 값만 깨지고 파일 끝의 목록은 멀쩡한 ZIP도 이제 복구합니다([PR128](https://github.com/Team-LJY/zipelo/pull/128), [PR141](https://github.com/Team-LJY/zipelo/pull/141)).
- **TAR.BZ2와 TAR.XZ 풀기:** 기존 TAR, TAR.GZ와 함께 풀 수 있습니다([PR119](https://github.com/Team-LJY/zipelo/pull/119)).
- **폴더 크기:** 목록의 폴더 크기를 안에 든 파일 크기의 합으로 보여 줍니다([PR152](https://github.com/Team-LJY/zipelo/pull/152)).

## 더 안전하고 편해진 점

- 같은 이름의 파일이 있으면 덮어쓰기, 건너뛰기, 이름 바꿔 저장 중에 고르고 나머지 파일에도 같은 선택을 적용할 수 있습니다([PR114](https://github.com/Team-LJY/zipelo/pull/114)).
- 기본 한도(16 GiB, 10,000개)를 넘는 작업은 허용을 물은 뒤 최대 64 GiB, 200,000개까지 처리합니다. 압축률, 진행 시간, 여유 공간과 경로 안전 검사는 그대로 적용합니다([PR121](https://github.com/Team-LJY/zipelo/pull/121)).
- 압축파일을 처음 열 때 진행 창이 Zipelo 창 뒤로 숨지 않습니다. Wayland에서는 진행 창을 Zipelo 창 안에 띄웁니다. 목록을 읽는 동안 "이 폴더는 비어 있습니다"를 잘못 보여 주던 문제도 고쳤습니다([PR142](https://github.com/Team-LJY/zipelo/pull/142)).
- 큰 압축파일을 풀 때 시스템 임시 폴더(`/tmp`) 공간이 모자라 실패하던 문제를 줄였습니다. 풀기는 풀 위치에서 임시 파일을 쓰고, 목록 보기와 미리보기는 원본 전체를 복사하지 않습니다. 공간이 모자라면 권한 문제가 아니라 공간 부족으로 안내합니다([PR132](https://github.com/Team-LJY/zipelo/pull/132), [PR133](https://github.com/Team-LJY/zipelo/pull/133), [PR135](https://github.com/Team-LJY/zipelo/pull/135)).
- Ubuntu에서 폴더 선택 창으로 새로 만든 폴더(권한 775)에 풀지 못하던 문제를 고쳤습니다. 다른 계정이 쓸 수 있는 폴더는 계속 거부합니다([PR144](https://github.com/Team-LJY/zipelo/pull/144)).
- Windows는 기존 설치 위치에 업데이트하며 기본 앱 연결을 유지합니다. 제거할 때는 다른 앱을 고르도록 안내합니다. Ubuntu는 같은 사용자가 제거하고 그동안 기본 앱을 바꾸지 않았다면 기록된 이전 기본 앱을 되돌립니다([PR112](https://github.com/Team-LJY/zipelo/pull/112), [PR116](https://github.com/Team-LJY/zipelo/pull/116)).
- Ubuntu `.deb`의 X11 실행에 필요한 라이브러리를 의존성에 넣었습니다([PR122](https://github.com/Team-LJY/zipelo/pull/122)).
- 문제를 조사할 때 `ZIPELO_LOG`(off, error, warn, info, debug, trace)로 로그 단계를 바꿀 수 있습니다. 배포 빌드의 기본은 오류만 남기며, 암호는 어느 단계에서도 남기지 않습니다([PR152](https://github.com/Team-LJY/zipelo/pull/152)).

## 남은 한계

- RAR 압축과 풀기는 지원하지 않습니다. 라이선스 걱정 없이 쓸 방법이 확인될 때만 다시 검토합니다.
- 암호 7z 만들기, solid 방식 7z 만들기, BCJ 같은 필터가 더해진 암호 7z 풀기, 나뉜 ZIP(`.zip.001`, `.z01`) 열기와 만들기, 나뉜 압축파일의 미리보기는 지원하지 않습니다. 나눠서 만들기는 7z로만 됩니다.
- ZIP 복구는 4 GiB 초과, 파일 끝 목록의 ZIP64, 나뉜 ZIP, SFX와 지원하지 않는 압축 방식을 제외하며 모든 손상을 복구하지는 못합니다. 원본을 보관하고 복구본 내용을 확인하세요.
- 미리보기는 HTML/SVG/PDF, 애니메이션과 UTF-8 이외 인코딩을 지원하지 않습니다.
- 압축된 TAR(TAR.GZ, TAR.BZ2, TAR.XZ)은 TAR 헤더와 빈 공간도 한도에 포함하므로 파일 크기 합계보다 먼저 한도에 닿을 수 있습니다.
- 목록 보기와 미리보기는 사용자 캐시 폴더, 압축파일 폴더, 시스템 임시 폴더 순서로 임시 파일을 씁니다. solid 7z 미리보기와 압축 TAR는 여전히 큰 임시 공간이 필요할 수 있습니다.
- 끌어서 풀기와 파일 바로 열기는 압축파일 옆 임시 폴더(Ubuntu는 `.이름.zip (Zipelo 임시)` 숨김 폴더, 홈 폴더 바로 아래는 숨기지 않음)에 먼저 풀고, Zipelo를 닫을 때 지웁니다. 압축파일이 있는 폴더에 쓸 공간과 권한이 필요합니다.
- Wayland에서는 진행 창이 Zipelo 창 안에만 뜨고 창 밖으로 옮길 수 없습니다.
- Zipelo로 암호를 건 ZIP은 Windows 탐색기의 기본 압축 기능으로 열 수 없습니다. Zipelo나 7-Zip을 사용하세요. 암호를 잊으면 열 수 없습니다.
- 자원 제한은 모든 압축 폭탄이나 메모리 사용을 완전히 막는다는 보장이 아닙니다. 공간 검사는 공간을 미리 잡아 두지 않으므로 실제로 쓰는 중에 실패할 수 있습니다. 실패나 취소 뒤에는 결과 안내를 확인하세요.
- 설치 파일에는 코드 서명이 없어 Windows에서 "Windows의 PC 보호" 창이 뜰 수 있습니다. 다른 Ubuntu 버전과 모든 Windows 환경에서 확인한 것은 아닙니다. JAR 같은 앱과 문서용 형식은 제품 범위가 아닙니다.

## 검증 근거

아래는 각 PR과 팀 실기 기록이며, 이 문서 변경에서 다시 실행한 결과가 아닙니다.

- **자동 검증:** 각 PR은 `python3 tests/ci/run_rust.py local`(Linux 빌드, 시험, Windows 교차 빌드) 통과 기록이 있습니다. PR152 최종 내용과 같은 `beba6ae`([PR155 리뷰](https://github.com/Team-LJY/zipelo/pull/155#pullrequestreview-5944969211))와 버전을 0.2.1로 맞춘 `dca139c`([PR157 리뷰](https://github.com/Team-LJY/zipelo/pull/157#pullrequestreview-5949371301))에서 build, test, Windows 교차 빌드가 모두 6/6 통과했습니다.
- **마지막 전체 실기 (`3a77cfd`):** 2026-11-15 Windows 11 Pro build 26300, Ubuntu 26.04.1 LTS Wayland와 X11에서 텍스트 미리보기, 나뉜 7z 풀기, 7z 만들기, ZIP 복구, 틀린 암호 재입력과 v0.1 위 덮어 설치를 확인했습니다. 결과 파일은 독립 도구와 기대 SHA256으로 대조했습니다([검토 기록](https://github.com/Team-LJY/zipelo/pull/129#pullrequestreview-5905758223)). 이때 설치 파일의 버전 표시는 0.1.0이었습니다.
- **최종 실기 (`a78fa3d`):** 2026-11-18 이 커밋으로 만든 `Zipelo-0.2.1-windows-x64-setup.exe`와 `zipelo_0.2.1_amd64.deb`를 Windows 11(build 26300), Ubuntu 26.04.1 LTS Wayland와 X11 VM에서 직접 조작했습니다. 세 환경 모두 차단 결함 없이 아래 항목을 통과했습니다([Wayland](https://github.com/Team-LJY/zipelo/pull/156#issuecomment-6967793527), [X11](https://github.com/Team-LJY/zipelo/pull/156#issuecomment-6968245084), [Windows](https://github.com/Team-LJY/zipelo/pull/156#issuecomment-6968448684)).
    - v0.1.0 위에 덮어 설치하면 0.2.1로 바뀌고, 압축파일을 더블클릭하면 기본 앱인 Zipelo로 열립니다.
    - 찾기(Ctrl+F, Ctrl+Shift+F)와 Enter 이동, Esc로 선택 풀기, 창 닫기, 프로그램 끝내기가 동작합니다.
    - 더블클릭으로 열 때 진행 창이 Zipelo 창 뒤로 숨지 않습니다. Windows와 X11은 따로 뜬 진행 창이 위에 있고, Wayland는 Zipelo 창 안에 뜹니다.
    - 탐색기와 Nautilus로 끌어서 풀면 원본과 같은 파일(SHA256 일치)이 나오고, Zipelo 창 위에 놓으면 바로 취소됩니다.
    - 1 MB로 나눈 7z를 만들고 다시 열어 풀면 원본과 내용이 같습니다.
    - 설치된 실행 파일은 최종 빌드와 같고, 앱 로그에 0.2.1이 찍힙니다.
- 실기는 모두 VM에서 했으며 실제 PC에서는 확인하지 않았습니다. 설치 파일에는 코드 서명이 없습니다. Windows에서 7z를 만드는 도중 취소하는 실기, 모든 오류 화면, 다른 OS 버전과 성능은 확인 범위 밖입니다.

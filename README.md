# Zipelo

쉽고 빠른 압축과 압축 해제를 목표로 만드는 MIT 오픈소스 프로그램입니다.

[![Windows용 Zipelo 다운로드](assets/download-windows.svg)](https://github.com/Team-LJY/zipelo/releases/download/v0.1.0/Zipelo-0.1.0-windows-x64-setup.exe)

버튼을 누르면 Windows 설치 파일(v0.1.0)을 바로 받습니다. Ubuntu용은 [.deb 설치 파일](https://github.com/Team-LJY/zipelo/releases/download/v0.1.0/zipelo_0.1.0_amd64.deb)을 받으세요. 모든 버전은 [릴리스 페이지](https://github.com/Team-LJY/zipelo/releases/latest)에 있습니다.

![Windows에서 샘플 ZIP을 연 Zipelo 화면](assets/screenshots/windows-archive-open.png)

## 이런 걸 할 수 있어요

- **압축 풀기:** [열기]로 압축 파일을 고르거나 창에 끌어 놓으세요. [압축해제]를 누르고 저장할 곳을 고르면 됩니다. 원하는 파일과 폴더만 선택해서 풀 수도 있습니다.
- **ZIP 만들기:** [압축하기]를 눌러 묶을 폴더를 고른 뒤, ZIP 만들기 창에서 [압축하기]를 누르고 저장할 이름과 위치를 정하세요.
- **암호로 보호하기:** ZIP을 만들 때 [암호로 보호하기]를 켜면 암호를 설정할 수 있습니다. 암호가 걸린 ZIP을 풀 때는 암호를 입력합니다.
- **여러 형식 열기:** ZIP, 7z, TAR, GZ, TAR.GZ를 풀 수 있습니다. 새로 만드는 압축 파일은 ZIP입니다.
- **기본 프로그램 설정:** 설치 마지막 화면에서 [적용]을 누르면 Windows 설정이 열립니다. 사용할 압축 파일 형식의 기본 앱을 Zipelo로 직접 선택하세요.

## 설치 방법

Windows에서는 아래 순서로 설치합니다.

1. 위 다운로드 버튼을 눌러 Windows 설치 파일(`.exe`)을 받으세요.
2. 받은 설치 파일을 더블클릭하고 화면 안내에 따라 설치하세요. "Windows의 PC 보호" 창이 뜨면 [추가 정보]를 누른 뒤 [실행]을 누르세요. 아직 코드 서명이 없는 설치 파일이라 이 창이 나올 수 있습니다.
3. 기본 압축 프로그램으로 쓰려면 마지막 화면에서 [적용]을 누르고 Windows 설정에서 Zipelo를 선택하세요. 설정을 바꾸지 않으려면 [마침]을 누르세요.
4. Zipelo를 실행하고 [열기]에서 압축 파일을 고르세요. 기본 앱으로 설정했다면 압축 파일을 더블클릭해 열 수도 있습니다.

Ubuntu에서는 [.deb 설치 파일](https://github.com/Team-LJY/zipelo/releases/download/v0.1.0/zipelo_0.1.0_amd64.deb)을 받아 `sudo apt install ./zipelo_0.1.0_amd64.deb`로 설치하세요. 처음 설치할 때 ZIP, 7z, TAR, GZ, TAR.GZ의 기본 앱이 Zipelo로 바뀝니다.

## 아직 안 되는 것과 사용 전 알아둘 점

- RAR 열기, 손상된 압축 파일 복구, 파일 내용 미리보기는 지원하지 않습니다.
- 여러 조각으로 나뉜 압축 파일, 7z 만들기, TAR.BZ2와 TAR.XZ는 지원하지 않습니다.
- 한 번에 푸는 파일의 전체 크기와 ZIP으로 묶는 원본의 전체 크기는 각각 16 GiB까지입니다.
- Zipelo에서 암호를 걸어 만든 ZIP은 Windows 탐색기의 기본 압축 기능으로 열 수 없습니다. Zipelo나 7-Zip을 사용하세요. 암호를 잊으면 파일을 열 수 없습니다.
- 같은 이름의 파일을 덮어쓰지 않습니다. 이름이 겹치면 다른 저장 위치나 이름을 선택하세요.
- Windows에서 새 버전을 설치할 때는 기존 버전을 먼저 제거하세요. 기존 설치 위에 덮어쓰는 업데이트는 지원하지 않습니다.

자세한 지원 범위와 제한은 [v0.1 릴리스 노트](docs/release-notes-v0.1.md)를 참고하세요.

## 개발자용

Rust로 개발하며 압축 처리는 별도 작업자 프로세스에서 수행합니다. 로컬 빌드와 시험은 `python3 tests/ci/run_rust.py local`로 실행합니다.

[기획서](docs/PLANS.md)와 [UI 설계](docs/ui-design-v0.1.md)를 참고하세요. 소스는 [MIT 라이선스](LICENSE)로 공개합니다.

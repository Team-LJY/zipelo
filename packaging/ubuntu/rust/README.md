# Ubuntu Rust 패키지

Ubuntu 26.04 amd64에서 현재 저장소의 실제 앱과 작업자를 빌드합니다. 배포 게시와 서명은 수행하지 않습니다.

```sh
python3 packaging/ubuntu/rust/build.py --target-dir tests/ci/package-target --output tests/release-packages/runs/ubuntu
sudo dpkg -i tests/release-packages/runs/ubuntu/zipelo_0.1.0~dev1_amd64.deb
zipelo /path/to/archive.zip
sudo dpkg -r zipelo
```

Rust 1.99.1, dpkg-deb와 desktop-file-validate가 필요합니다. 출력은 새 경로여야 하며 기본은 release 빌드입니다. 개발 검증은 --profile dev를 명시합니다. 앱과 작업자는 같은 커밋에서 빌드하며 BUILD-INFO.json에 커밋과 SHA-256을 기록합니다.

sudo로 처음 설치하면 postinst가 SUDO_USER의 ZIP, 7z, TAR.GZ, GZ, TAR 기본 앱을 xdg-mime으로 적용합니다. 최초 설치에서 기존 기본 앱 선택을 바꿉니다. 업그레이드와 재설정에서는 사용자의 기본 앱 선택을 유지합니다. 다른 패키지 관리자로 설치해 SUDO_USER가 없으면 사용자 설정을 추측하지 않고 연결 후보만 등록합니다. 이 경우 사용자 계정에서 xdg-mime default zipelo.desktop application/zip 등을 실행해야 합니다. 제거는 사용자 MIME 설정을 지우지 않으며 다른 앱을 기본 앱으로 다시 선택할 수 있습니다.

이 패키지의 검사 범위는 Ubuntu 26.04입니다. 다른 Ubuntu 버전과 Windows 실기는 별도 확인이 필요합니다.

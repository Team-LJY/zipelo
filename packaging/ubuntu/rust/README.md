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


앱과 작업자의 Cargo.lock 전체 항목을 cargo metadata --locked --offline --all-features와 대조한 뒤 packaging/licenses.py로 THIRD-PARTY-LICENSES.txt를 생성합니다. 로컬 registry 원문과 캐시 crate의 잠금 체크섬을 확인하며 LICENSE, COPYING, NOTICE 등의 원문을 함께 보존합니다. 원문이 없거나 변조됐거나 metadata 대조에서 누락되면 패키지 생성을 중단합니다.

처음 준비할 때 각 manifest에 cargo +1.99.1 fetch --locked --manifest-path 명령으로 모든 플랫폼의 의존성을 캐시에 받아 두어야 합니다. 고지 생성 중에는 네트워크를 사용하지 않습니다. Windows에는 설치 폴더에, Ubuntu에는 /usr/share/doc/zipelo/THIRD-PARTY-LICENSES.txt에 포함합니다. 파일 동봉 자체가 별도 라이선스 준수 검토를 대신하지는 않습니다.

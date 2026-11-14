# Ubuntu Rust 패키지

Ubuntu 26.04 amd64에서 현재 저장소의 실제 앱과 작업자를 빌드합니다. 배포 게시와 서명은 수행하지 않습니다.

```sh
python3 packaging/ubuntu/rust/build.py --target-dir tests/ci/package-target --output tests/release-packages/runs/ubuntu
sudo apt install ./tests/release-packages/runs/ubuntu/zipelo_0.2.0_amd64.deb
zipelo /path/to/archive.zip
sudo dpkg -r zipelo
```

Rust 1.99.1, dpkg-deb와 desktop-file-validate가 필요합니다. 출력은 새 경로여야 하며 기본은 release 빌드입니다. 개발 검증은 --profile dev를 명시합니다. 앱과 작업자는 같은 커밋에서 빌드하며 BUILD-INFO.json에 커밋과 SHA-256을 기록합니다.

sudo로 처음 설치하면 postinst가 SUDO_USER의 ZIP, 7z, TAR.GZ, GZ, TAR 기본 앱을 xdg-mime으로 적용합니다. 최초 설치에서 기존 기본 앱 선택을 바꿉니다. 업그레이드와 재설정에서는 사용자의 기본 앱 선택을 유지합니다. 다른 패키지 관리자로 설치해 SUDO_USER가 없으면 사용자 설정을 추측하지 않고 연결 후보만 등록합니다. 이 경우 사용자 계정에서 xdg-mime default zipelo.desktop application/zip 등을 실행해야 합니다. 최초 적용 전에 이전 기본 앱 ID를 해당 사용자의 ~/.local/state/zipelo-default-apps-v1에 기록합니다. 같은 사용자가 sudo로 제거하면 prerm이 현재 선택이 여전히 Zipelo인지 확인하고, 이전 데스크톱 파일이 존재하며 숨김 상태가 아닐 때만 이전 앱으로 복원합니다. 설치 후 선택한 다른 앱과 다른 사용자의 설정은 건드리지 않습니다.

이전 버전에서 설치해 기록이 없거나 이전 앱이 삭제된 경우에는 자동 복원하지 않습니다. 그래픽 패키지 관리자처럼 SUDO_USER를 제공하지 않는 제거도 사용자를 추측하지 않습니다. 사용자 지정 XDG 설정 경로와 하위 디렉터리 데스크톱 ID는 자동 복원 대상으로 보장하지 않습니다. 제거 전에 ZIP 파일을 오른쪽 클릭하고 [다른 프로그램으로 열기]에서 사용할 앱을 선택하세요. 다른 앱이 없다면 먼저 설치하세요. 제거 명령도 이 안내를 출력합니다. 사용자 MIME 설정 파일 전체를 지우거나 다른 사용자 홈을 순회하지 않습니다.

제거된 desktop 파일을 가리키는 선택은 [freedesktop 기본 앱 규격](https://specifications.freedesktop.org/mime-apps/1.0/default.html)에 따라 다음 유효한 후보로 넘어갑니다. 기존 기록이 없을 때 임의의 앱을 새 기본값으로 강제하지 않습니다. 자동 복원은 이전 기본 앱 ID 한 개를 대상으로 하며 MIME 설정 파일 전체의 과거 내용을 되돌리지 않습니다.

이 패키지의 검사 범위는 Ubuntu 26.04입니다. 다른 Ubuntu 버전과 Windows 실기는 별도 확인이 필요합니다.


앱과 작업자의 Cargo.lock 전체 항목을 cargo metadata --locked --offline --all-features와 대조한 뒤 packaging/licenses.py로 THIRD-PARTY-LICENSES.txt를 생성합니다. 로컬 registry 원문과 캐시 crate의 잠금 체크섬을 확인하며 LICENSE, COPYING, NOTICE 등의 원문을 함께 보존합니다. 원문이 없거나 변조됐거나 metadata 대조에서 누락되면 패키지 생성을 중단합니다.

처음 준비할 때 각 manifest에 cargo +1.99.1 fetch --locked --manifest-path 명령으로 모든 플랫폼의 의존성을 캐시에 받아 두어야 합니다. 고지 생성 중에는 네트워크를 사용하지 않습니다. Windows에는 설치 폴더에, Ubuntu에는 /usr/share/doc/zipelo/THIRD-PARTY-LICENSES.txt에 포함합니다. 파일 동봉 자체가 별도 라이선스 준수 검토를 대신하지는 않습니다.

로컬 crate에 원문이 없으면 packaging/license-supplements.json의 해당 버전 공식 upstream 원문을 사용합니다. 보충 파일에는 고정 출처 URL, crate 버전과 VCS 커밋, Cargo SPDX 식, UTF-8 원문과 SHA-256이 들어 있습니다. 생성기는 SPDX와 게시된 crate 커밋 및 원문 해시를 오프라인으로 검증합니다. 승인된 표준 보충도 없는 항목은 링크 설명만으로 대체하지 않고 계속 빌드를 차단합니다.

upstream에도 원문이 없는 승인된 12개 항목은 SPDX의 MIT 표준 원문과 해당 버전 Cargo.toml의 저자 고지로 보충합니다. 고지에 표준 원문 보충 사유, 선택한 라이선스, 원문과 저자 고지의 출처 및 SHA-256을 표시합니다. Cargo 메타데이터의 전체 라이선스 식은 그대로 보존합니다.

제거 회귀 시험은 python3 -m unittest discover -s tests/ci -p test_default_apps.py와 python3 -m unittest discover -s tests/release-packages -p test_ubuntu_lifecycle.py로 실행합니다. 실제 패키지 검증은 python3 tests/release-packages/uninstall_defaults_smoke.py <deb 경로>입니다. 기존 Zipelo 설치가 있으면 중단하며, 호스트 기본 앱은 바꾸지 않고 별도 설정 공간에서 이전 앱 복원과 xdg-open을 통한 unzip 실행을 확인합니다. 그래픽 데스크톱 창 조작이나 실제 SUDO_USER 홈의 복원까지 검증하는 명령은 아닙니다.

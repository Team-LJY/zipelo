# 패키지 검증

저장소의 packaging/windows/rust/build.py와 packaging/ubuntu/rust/build.py로 두 플랫폼의 실제 앱과 작업자를 생성합니다. 기본 profile은 release이며 ui-fixture는 사용하지 않습니다.

```sh
python3 tests/ci/run_rust.py local
python3 -m unittest discover -s tests/release-packages -p "test_*.py"
python3 tests/release-packages/ubuntu_smoke.py tests/release-packages/runs/ubuntu-release/zipelo_0.1.0~dev1_amd64.deb
```

Ubuntu 실기 스모크는 이 패키지를 sudo로 설치하고 ZIP, 7z, TAR.GZ의 기본 앱을 확인합니다. 별도 Xvfb 화면에서 공백이 있는 경로의 합성 ZIP을 실제 앱으로 열어 소유 창을 캡처합니다. 종료 후 패키지를 제거하고 기존 MIME 설정을 복원합니다. 설치나 제거가 실패해도 MIME 설정 복원을 시도하고 원래 실패와 정리 실패를 함께 보고합니다. 설치된 Zipelo나 같은 경로의 기존 파일이 있으면 중단합니다. 시험 중 다른 MIME 설정 변경이 감지되면 해당 파일을 덮어쓰지 않고 나머지 파일은 복원합니다.

sudo 비대화형 권한, Xvfb, xdotool과 ImageMagick import가 필요합니다. 이 검사는 실제 시스템 패키지를 설치하고 사용자 기본 앱을 임시로 바꾸므로 실행 전에 해당 작업의 승인이 필요합니다. 출력은 runs/에 남고 Git에는 포함하지 않습니다.

Windows 설치와 설정 화면, 제거의 실기 검증은 이 Linux 시험에 포함되지 않습니다. release 게시와 다운로드 버튼 갱신도 수행하지 않습니다.


## 라이선스 대조

```sh
python3 -m unittest discover -s tests/release-packages -p "test_*.py"
python3 packaging/licenses.py --manifest experiments/rust-ui-memory-app/Cargo.toml --manifest experiments/rust-archive-worker/Cargo.toml --output tests/release-packages/runs/THIRD-PARTY-LICENSES.txt --audit tests/release-packages/runs/license-inventory.json
```

고지의 crate 목록은 두 Cargo.lock과 metadata의 합집합을 기준으로 합니다. 플랫폼이나 활성 기능을 기준으로 항목을 줄이지 않습니다. 원문 파일이 없는 crate는 라이선스 식만으로 대체하지 않으며 최종 파일을 생성하지 않습니다. audit JSON은 누락 원인 확인용이며 배포 고지가 아닙니다. 대조 시험은 원문 보존, 변경된 캐시 거부, metadata 누락과 두 패키지의 조기 중단을 검증합니다.

로컬 crate에 원문이 없으면 packaging/license-supplements.json의 해당 버전 공식 upstream 원문을 사용합니다. 보충 파일에는 고정 출처 URL, crate 버전과 VCS 커밋, Cargo SPDX 식, UTF-8 원문과 SHA-256이 들어 있습니다. 생성기는 SPDX와 게시된 crate 커밋 및 원문 해시를 오프라인으로 검증합니다. upstream에도 원문이 없는 항목은 링크 설명만으로 대체하지 않고 계속 빌드를 차단합니다.

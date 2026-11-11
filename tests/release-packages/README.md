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

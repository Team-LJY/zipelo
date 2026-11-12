# Zipelo 화면 회귀 확인

설계 기준은 docs/ui-design-v0.1.md의 수용 기준 23개이며, 비교 기준 커밋은 b10c7eedddd202f83766aa8f001142cdfdb4097c임.

## 기능 시험

프로젝트 루트에서 실행함.

```sh
python3 tests/ci/run_rust.py local
```

tests/rust-app-ui/presentation.rs에서 결과 안내, 오류 문구, 폴더 생성과 정리, 선택창, 암호 확인, 키보드 입력과 최소 크기를 검증함. tests/rust-app-real-worker/flows.rs와 password_flows.rs는 실제 작업자를 사용해 파일 생성과 재해제, 암호 재입력 및 취소 후 파일 보존을 검증함.

## 전후 화면 재현

Linux의 Xvfb, xdotool, ImageMagick import와 시스템 CJK 글꼴이 필요함. 제품 의존성은 추가하지 않음. 실행 파일 준비는 로컬 Git 객체와 기존 Cargo 오프라인 캐시를 사용함. 기존 산출물은 지우지 않으며 새 output 폴더를 지정함.

```sh
python3 tests/ui-redesign/prepare.py --output tests/ui-redesign/runs/reproduce --target-dir tests/ui-redesign/runs/target
python3 tests/ui-redesign/capture.py --binary tests/ui-redesign/runs/reproduce/before-capture --variant before
python3 tests/ui-redesign/capture.py --binary tests/ui-redesign/runs/reproduce/after-capture --variant after
python3 tests/ui-redesign/capture.py --binary tests/ui-redesign/runs/reproduce/after-capture --variant after --width 760 --height 540 --output tests/ui-redesign/screenshots/minimum
```

촬영기는 전용 Xvfb를 만들고 X11을 지정하며 종료 시 자신이 만든 창과 서버만 종료함. 결과는 screenshots/before, screenshots/after, screenshots/minimum에 저장함. 15개 상태를 각각 촬영하며 capture.json에 크기와 경로를 기록함.

촬영 화면은 tests/rust-app-ui/capture.rs의 합성 입력임. 글꼴과 eframe 렌더러 및 실제 화면 컴포넌트를 사용하고 "화면 시험 모드"를 표시함. 암호 불일치 화면에는 고정된 시험 문자열만 입력함. 합성 작업자는 실제 압축 파일을 읽거나 결과 파일을 저장하지 않으며, 기본 풀 위치 생성은 tests/ui-redesign/runs/fixtures 아래에서 확인함. 실제 파일 작업 검증은 별도의 real_worker 시험 결과와 구분함.

기준 화면 준비 시 같은 촬영 도우미를 기준 커밋 소스에 넣고, 새 화면 전용 메서드 두 곳만 기존 공개 모델 호출로 바꿈. 기준 제품 소스는 바꾸지 않음. --states 옵션으로 일부 상태만 다시 촬영할 수 있으며 다른 상태의 기록은 보존함.

최소 크기에서 ZIP 만들기 본문은 내부 스크롤로 모든 설정에 접근함. 주 버튼과 취소 버튼은 본문 스크롤 밖에 고정함. 폴더 열기는 셸 없이 운영체제 명령에 경로 하나를 인자로 전달하며, 특수 문자가 있는 경로도 시험함. Windows 실제 화면과 운영체제 파일 탐색기 실행 확인은 이 Linux 촬영 기록에 포함하지 않음.

실제 X11 창에서 파일 이름을 눌러 행 선택을 확인하는 별도 검증임.

```sh
python3 tests/ui-redesign/check_row_selection.py --binary tests/ui-redesign/runs/reproduce/after-capture
```

validation.json에는 실행한 소스 트리, local 결과와 화면 해시를 기록함. 기록 파일과 이 설명의 추가는 시험 후 문서 변경임. 대용량 작업자 시험은 원본 스냅샷과 임시 공간을 함께 확인하므로 실행 파일 시스템에 최소 12GB 이상의 여유 공간이 필요하며 빌드 캐시 공간은 별도임.

PR85 리뷰 회귀는 presentation.rs의 keyboard_file_pickers_enter_child_before_confirming_file, keyboard_folder_pickers_navigate_before_explicit_folder_confirmation, changed_zip_sources_retry_creation_with_or_without_open_archive에서 확인함. 폴더 행의 Tab 및 Enter 탐색은 6개 선택 모드에서, ZIP 원본 변경 뒤 생성 재시도는 기존 압축파일 유무 두 경우에서 검증함.

ZIP 만들기의 취소 우선 처리는 keyboard_cancel_and_escape_never_start_zip_creation과 zip_creation_enter_only_activates_focused_make_button에서 검증함. Tab으로 취소와 만들기 버튼에 초점을 옮긴 뒤 Enter를 누르며, Esc와 이름 입력칸의 Enter가 생성 요청을 보내지 않는지도 확인함.

Enter는 egui 버튼의 초점 활성화만 사용하며 암호 입력란의 제출만 예외임. secondary_button_enter_matrix_never_starts_creation_or_extraction은 기본 화면, 생성 창, 결과, 진행, 종료 확인과 선택창의 탐색 버튼을 한 번에 검증함. password_secondary_enter_does_not_submit_valid_input은 유효한 암호가 있어도 보기, 숨기기와 취소 버튼이 제출하지 않음을 검증함.

폴더 생성 보고는 추출 시작 시 기록한 파일 대상과 예상 폴더 및 상위 폴더로 구분함. 빈 폴더만 있는 ZIP은 실제 작업자로 만들고 새 폴더 추출과 기존 폴더 재사용 모두 성공 표시를 확인함. 파일 저장 누락, 알 수 없는 생성 보고와 저장 불확실 결과의 경고는 유지함.

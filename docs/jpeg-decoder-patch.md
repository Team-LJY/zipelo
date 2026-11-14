# JPEG 스캔 완전성 패치

미리보기 작업자는 `experiments/rust-archive-worker/vendor/zune-jpeg`의 `zipelo-jpeg 0.5.15`를 사용한다. 순수 Rust인 zune-jpeg 0.5.15의 MIT 조건을 선택한 로컬 패치이며, PNG와 통신 계약은 변경하지 않는다.

원본은 crates.io의 [zune-jpeg 0.5.15](https://crates.io/crates/zune-jpeg/0.5.15)이다. 원본 crate의 SHA-256은 `27bc9d5b815bc103f142aa054f561d9187d191692ec7c2d1e2b4737f8dbd7296`이다. `LICENSE-MIT` 원문과 소스의 저작권 고지를 보존했다. 패키지 이름을 구분하고 미리보기 작업자만 이 패치를 사용한다. 앱의 기존 이미지 의존성은 유지한다.

## 수정 이유와 범위

원본은 strict 모드에서도 스캔 종료 마커 뒤에 비트를 보충한다. 헤더에 해당하는 앞 623바이트와 EOI만 있는 시험 JPEG도 이 보충 비트로 픽셀을 만들고 성공한다. 출력 버퍼 길이나 초기값 검사로는 실제 그림 데이터와 보충 데이터를 구분할 수 없다.

- `bitstream.rs`는 실제 엔트로피 비트 수를 별도로 집계한다. Huffman 조회의 미리 읽기는 허용하고 실제 소비가 입력 비트를 넘을 때 오류를 기록한다. 보충 비트는 입력량으로 세지 않는다.
- `mcu.rs`와 `mcu_prog.rs`는 MCU 행, progressive MCU 및 재시작 경계에서 오류를 확인한다. baseline의 직접 출력 경로는 기록한 픽셀 바이트 수도 확인한다.
- 작업자의 `preview.rs`는 모든 색상 성분의 최초 DC 스캔을 요구하고, baseline에서 중복되거나 불완전한 성분 스캔을 거부한다. 원본과 결과 크기 상한, 암호, 자원 승인과 취소 정책은 유지한다.
- Cargo manifest를 프로젝트 경로 의존성으로 바꿨고 사용하지 않는 upstream 시험 모듈과 주석 처리된 시험, 잘못된 crate 단위 `macro_use` 속성과 주석의 이모지를 제외했다. upstream 시험 코드는 복사하지 않았다. 회귀 시험과 새 표본은 루트 `tests/rust-archive-worker/`에만 둔다.

`packaging/licenses.py`는 프로젝트 경로 패키지에 명시된 라이선스 원문을 해당 패키지 내부에서 읽으며, 경로 이탈과 라이선스 파일의 심볼릭 링크를 거부한다. 배포 고지에는 `zipelo-jpeg`의 upstream MIT 원문이 포함된다. 새 registry 버전이나 네이티브 라이브러리는 추가하지 않는다.

## 검증과 유지

기존 baseline 표본의 앞 623바이트와 EOI, 이미지 높이만 늘린 불완전한 스캔, 각 progressive 스캔의 데이터 누락, 여러 MCU 행과 재시작 마커를 시험한다. PNG는 완전한 IEND와 올바른 CRC가 있지만 픽셀 행이 부족한 입력도 손상으로 거부해야 한다. 실제 ZIP 작업자는 오류 시 Begin/Chunk/End를 전송하지 않아야 한다.

여러 행의 JPEG 표본 5개는 시험용 RGB 그라데이션을 baseline, progressive, 회색조, CMYK와 재시작 마커 형식으로 직접 생성했다. upstream 버전을 바꾸거나 패치를 제거할 때 이 회귀와 `python3 tests/ci/run_rust.py local`, 라이선스 시험을 다시 실행한다. 동일한 스캔 완전성 보장이 확인된 upstream 릴리스가 있으면 경로 의존성을 제거할 수 있다.

로컬 패키지 이름은 `zipelo-jpeg`이므로 보안 권고를 확인할 때 원본 이름인 `zune-jpeg 0.5.15`도 대조해야 한다. 패치는 엔트로피 완전성만 보강하며 다른 upstream 결함이나 향후 권고의 부재를 보장하지 않는다.

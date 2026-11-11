# 대용량 압축파일 회귀 표본

Python 표준 라이브러리로 실행할 때 합성 표본을 생성합니다. 생성 데이터는 커밋하지 않으며 읽기와 쓰기는 64 KiB로 나눕니다. revision 2의 입력 u64::MAX, 해제 총량 16 GiB, 압축비 1000:1을 기준으로 합니다.

## 독립된 경계

- large-input.zip과 large-input.tar.gz: 1 GiB payload를 Store ZIP과 Gzip level 0의 TAR로 저장합니다. 압축파일 자체가 1 GiB 이상이며 다른 한도 이내이므로 실제 해제가 성공해야 합니다.
- output-only.zip: 유효한 ZIP64에 16 GiB + 1바이트를 level 1로 압축합니다. 해제 총량만 초과하며 항목 압축비는 1000:1 미만입니다. 요청은 Failed/Limit으로 끝나고 출력이 없어야 합니다. Python 참조 판독기가 전체 16 GiB + 1바이트와 CRC를 스트리밍 검증합니다. 제품의 해당 크기 해제 성공을 검증하는 시험은 아닙니다.
- boundary.zip과 boundary.tar.gz: 실제 해제 크기 65536바이트에 요청 output을 65537, 65536, 65535로 설정해 경계 직전, 일치, 초과를 실제 해제로 검사합니다. TAR.GZ는 TAR 헤더와 패딩 및 종료 블록도 총량에 포함합니다.
- ratio-below.gz, ratio-equal.gz, ratio-above.gz: FEXTRA를 포함한 유효한 GZ 멤버 길이를 정확히 2000바이트로 맞춥니다. 해제 크기는 각각 1999999, 2000000, 2000001바이트로, 멤버별 1000:1 경계보다 정확히 1바이트 작음, 일치, 1바이트 큼입니다. 총량은 16 GiB보다 작습니다.
- ratio-only.zip과 ratio-only.tar.gz: 16 MiB payload를 level 6으로 압축해 비율만 초과시킵니다. ZIP의 분모는 항목의 압축 payload, GZ의 분모는 헤더와 트레일러를 포함한 멤버 전체 바이트입니다.

생성 시 실제 archive_bytes, decoded_bytes, ratio_input_bytes와 요청 한도를 비교해 violated_limits가 정확히 일치하는지 검사합니다. 한 표본에서 여러 한도를 넘으면 생성을 실패시킵니다. 입력, 출력, 항목 수는 manifest의 실제 값이 모두 작업자 요청에 전달됩니다. 생성 결과만으로 검증 완료를 주장하지 않도록 worker_execution_verified=false를 기록합니다.

## 실행

저장소 루트에서 실행합니다. --large는 1 GiB 성공 2건과 16 GiB 초과 거부를 포함해 총 14건을 검사합니다. 생성을 포함한 기본 실행은 tests 안의 전용 임시 디렉터리를 쓰고 종료 시 정리합니다. 압축 표본, 입력 사본과 출력에 수 GiB의 임시 디스크 공간이 필요합니다.

```sh
python3 tests/ci/run_rust.py local
python3 tests/large-archive-fixtures/verify.py --large
```

CARGO_TARGET_DIR을 지정했다면 검증기도 같은 환경 값을 사용합니다. --probe와 --worker로 바이너리 경로를 따로 지정할 수도 있습니다. --large를 생략하면 작은 경계 11건만 검사합니다. 기본 local 실행에는 작은 표본의 참조 무결성, 경계 독립성과 생성기 경로 및 버퍼 제약 검사가 포함됩니다.

표본을 보존하려면 아래처럼 새 경로를 지정합니다. 기존 디렉터리는 생성기가 덮어쓰지 않으며, 검증기는 기존 expectations.json이 있으면 재사용합니다.

```sh
python3 tests/large-archive-fixtures/generate.py --large --output tests/large-archive-fixtures/runs/full
python3 tests/large-archive-fixtures/verify.py --large --directory tests/large-archive-fixtures/runs/full
```

검증기는 전체 표본을 참조 판독기로 읽어 합성 내용, CRC와 TAR 구조를 확인한 후 실제 작업자의 종료 코드, Terminal, 오류, 게시 상태, Output 개수와 정리를 확인합니다. 성공 파일은 길이와 모든 바이트를 검사하며 거부한 경우 출력 폴더가 비어 있어야 합니다. 입력의 전후 SHA-256도 확인합니다. 작업자 피크 RSS, 디스크 부족과 고아 작업자 정리는 tests/rust-archive-worker/hardening.rs에서 별도로 검사합니다. Windows 실기는 별도 검증이 필요합니다.

# ZIP 복구 표본

PLANS 8.10의 네 손상 종류를 자체 생성한 작은 ZIP으로 구분함. 정상 대조군 1개와 손상 표본 6개를 포함함. 실제 복구기 구현이나 제품 검증 결과가 아니라 복구 수용 시험용 입력과 기대값임. UI 변경 없음.

## 표본과 기대값

정상 원본 `control.zip`은 순서대로 `hello.txt`, `한글/자료.txt`, `empty.txt`, `empty-dir/`, `middle.bin`, `after.txt`를 포함함. Store와 Deflate, 한글 경로, 빈 파일과 빈 폴더를 함께 검사함. 항목별 원본 길이, SHA-256, CRC32, 압축 방식과 바이트 위치는 `expectations.json`의 `source_entries`에 기록함.

- `missing-directory.zip`: 중앙 디렉터리 시작부터 잘라낸 헤더 손상임. 로컬 헤더와 본문이 완전한 6개 항목을 원본 해시대로 복구해야 함.
- `broken-eocd.zip`: EOCD 서명 4바이트만 바꾼 헤더 손상임. 6개 항목을 원본 해시대로 복구해야 함.
- `broken-local-header.zip`: `한글/자료.txt`의 로컬 서명 4바이트만 바꾸고 중앙 목록, EOCD, 이름/extra 길이와 모든 본문을 유지함. 중앙 목록과 연속 로컬 범위가 1:1 대응하므로 CentralDirectory에서 6개 항목 전부를 해시대로 복구하고 known_entries=6, unknown_loss=false여야 함. 실제 게시와 마무리까지 성공하면 Completed/None, publication=Complete, created_count=1이 기대값임. 일반 판독기는 중앙 목록을 읽을 수 있어도 손상 항목의 내용을 읽을 때 실패해야 함.
- `middle-truncated.zip`: 다섯 번째 항목 `middle.bin`의 본문 중간에서 잘라냄. 앞의 4개 항목만 복구하고 `middle.bin`을 정상 복구로 표시하면 안 됨. 뒤의 `after.txt`는 이름과 존재 자체를 손상본에서 관측할 수 없음.
- `body-corrupt.zip`: 중앙 디렉터리를 제거하고 `middle.bin` 본문의 1바이트를 뒤집음. CRC가 틀린 `middle.bin`을 제외한 5개 항목을 해시대로 복구해야 함. 뒤의 정상 `after.txt`까지 확인하여 손상 항목 이후의 탐색을 검증함. 중앙 디렉터리 제거는 일반 열기의 실패 후 복구 경로에 도달시키기 위한 별도 손상임.
- `unrecoverable.zip`: 첫 로컬 헤더의 20바이트만 남김. 검증 가능한 항목이 없으므로 복구본을 게시하거나 빈 ZIP 생성을 성공으로 표시하면 안 됨.

`oracle_states`는 원본을 알고 있는 시험의 판정임. `verified`만 복구본에 포함하고, `truncated`, `integrity_failed`, `not_observable`은 제외함. 이는 작업자 프로토콜의 enum 이름을 제안하는 것이 아님. 특히 중앙 목록이 없는 표본에서 앱이 사라진 항목의 이름이나 정확한 유실 개수를 알아낼 수 있다고 가정하지 않음. LocalChain 표본은 복구된 파일들의 CRC와 해시가 일치해도 원본 전체의 완전성을 증명하지 못하므로 경고를 유지해야 함. `broken-local-header.zip`만 완전한 중앙 목록에 근거한 완전 복구 기대값을 가짐. warning_required=false는 유실/목록 불명 경고가 필수가 아니라는 뜻이며 헤더 재구성 안내나 원본 진위에 대한 설명을 금지하지 않음.

모든 실행은 손상 입력을 그대로 두고 별도 경로에 복구본을 게시해야 함. 실행 전후 입력 SHA-256을 manifest의 값과 비교하고, 검증 실패 항목의 부분 출력이 게시되지 않았는지 검사해야 함. 경고, 종료 상태, 취소, 저장 실패, 링크와 경로 제한, 자원 제한은 이후 실제 작업자 통합 시험에서 별도로 확인해야 함. 이 묶음은 그 검증을 대신하지 않음.

## 실행

저장소 루트 기준임. 검증은 파일을 풀거나 제품 작업자를 실행하지 않음.

```sh
python3 -B tests/zip-recovery-fixtures/generate.py --check
python3 -B tests/zip-recovery-fixtures/verify.py
python3 -B -m unittest discover -s tests/ci -p test_zip_recovery_fixtures.py
```

기존 `tests/ci/run_rust.py local`의 Python 시험 탐색에도 연결함. 검증기는 정상본을 Python zipfile로 읽고 손상 연산, 고정 입력 해시, 각 로컬 항목의 CRC와 SHA-256, 불완전한 항목의 배제를 검사함. 로컬 항목 검사는 manifest의 알려진 위치를 사용하므로 범용 복구 알고리즘이나 손상 탐색 성공의 근거가 아님. 생성기는 고정 시간과 순서 및 Python 표준 라이브러리를 사용하며, `--check`는 현재 환경에서 커밋된 바이트의 재현성을 확인함.

완전 복구 표본은 zipfile이 읽은 중앙 목록으로 위치와 연속 범위를 확인한 뒤 메모리 사본의 로컬 서명 4바이트만 되돌려 원본과 바이트 단위로 비교함. 이 참조 재구성에서 6개 항목을 독립 판독하고 해시를 확인하며 디스크의 손상 입력은 수정하지 않음. 제품 복구기 실행과는 구분함.

실제 작업자가 복구본을 만든 뒤 아래처럼 항목 집합, 파일과 폴더 구분, 해시를 검사할 수 있음. 출력은 지정된 작업 폴더 안에 두어야 함.

```sh
python3 -B tests/zip-recovery-fixtures/verify.py \
    --recovered missing-directory.zip /home/user/ssd-2/projects/zipelo/tests/work/zip-recovery/recovered.zip
```

이 명령은 fixture 입력 해시도 확인하지만 작업자의 다른 입력 사본이나 UI 경고까지 검증하지 않음. `control.zip`은 정상 열기의 대조군이며 자동 복구 대상이 아님. 표본 갱신이 필요할 때만 `generate.py`를 `--check` 없이 실행하여 이 폴더의 ZIP과 manifest를 재생성함.

# 대용량 압축파일 회귀 표본

Python 표준 라이브러리로 실행 시점에 표본을 생성합니다. 생성된 압축파일은 커밋하지 않습니다. 생성기는 64 KiB씩 읽고 쓰며 전체 입력이나 해제 데이터를 메모리에 보관하지 않습니다. 제품 코드와 통신 revision 2는 변경하지 않습니다.

## 현재 기대값

기준은 main d6b1a45입니다. 기본 요청의 input=67108864, output=134217728, entries=10000을 사용합니다. 현재 작업자는 이보다 큰 요청 한도도 허용하지 않습니다. 별도 압축률 한도 필드는 없으므로 비율 기준으로 거부한다고 판정하지 않습니다.

- large-input.zip: 1 GiB 단일 파일을 Store로 저장합니다. 압축파일 자체가 1 GiB 이상이므로 입력 한도에서 Failed/Limit이어야 합니다.
- large-input.tar.gz: 1 GiB 단일 파일의 USTAR를 Gzip level 0으로 저장합니다. 압축파일 자체가 1 GiB 이상이며 입력 한도에서 Failed/Limit이어야 합니다.
- large-expansion.zip과 large-expansion.tar.gz: 같은 1 GiB 영 바이트를 Deflate로 압축합니다. 입력은 64 MiB 미만이며 해제 한도로 Failed/Limit이어야 합니다. ZIP은 목록 메타데이터, TAR.GZ는 Gzip 해제 중 한도에 걸립니다.
- boundary.zip: 실제 해제 크기는 65536바이트입니다. output 한도를 65537, 65536, 65535로 설정해 각각 Completed, Completed, Failed/Limit을 요구합니다.
- boundary.tar.gz: 파일 내용은 64000바이트이고 USTAR 헤더, 패딩, 종료 블록까지 해제한 크기는 65536바이트입니다. 위와 같은 output 한도에서 같은 결과를 요구합니다. TAR.GZ 한도는 파일 내용만이 아니라 바깥 Gzip 해제 크기에도 적용됩니다.

경계 표본도 해제/입력 비율이 100배 이상인 유효한 고압축 스트림입니다. below/equal/above는 해제 크기가 요청 한도보다 작음/같음/큼을 뜻합니다. 스트리밍 변경 뒤에는 입력 크기 거부 기대값을 별도로 갱신해야 하며, 이 기준을 새 구현의 성공 기준으로 고정하지 않습니다.

## 생성과 검증

저장소 루트에서 실행합니다. output은 tests 아래의 존재하지 않는 경로여야 합니다. 기존 경로나 파일을 덮어쓰지 않습니다. 큰 입력 2개를 위해 약 2.1 GiB의 디스크가 필요하고, 검증 중 작은 경계 표본만 실제 파일로 해제합니다.

```sh
python3 tests/large-archive-fixtures/generate.py --large --output tests/large-archive-fixtures/runs/full
python3 tests/ci/run_rust.py local
python3 tests/large-archive-fixtures/verify.py --directory tests/large-archive-fixtures/runs/full --probe tests/ci/target/debug/zipelo-large-archive-probe --worker tests/ci/target/debug/zipelo-archive-worker
```

CARGO_TARGET_DIR을 별도로 지정했다면 probe와 worker 경로도 그 캐시에 맞춥니다. --large를 생략하면 작은 경계 표본만 생성합니다. 전체 생성 및 실제 작업자 검증은 명시적으로 실행하며, 기본 local 자체 검사에는 작은 표본의 무결성, 경계 기대값과 생성기 경로 및 버퍼 제약 검사가 포함됩니다.

생성된 expectations.json에는 각 표본의 실제 크기, 해제 크기, SHA-256, 요청 한도와 기대 결과를 기록합니다. worker_execution_verified=false는 생성만으로 작업자 검증을 주장하지 않음을 뜻하며 검증기는 이 파일을 바꾸지 않습니다.

검증기는 먼저 Python ZIP/Gzip 참조 판독기로 전체 표본을 64 KiB씩 읽어 CRC와 합성 내용, TAR 헤더 및 종료 블록을 확인합니다. 디스크에 참조 해제물을 쓰지 않습니다. 이어 실제 작업자를 별도 프로세스로 실행하고 exit 0, stderr 비어 있음, 정확히 한 번의 마지막 Terminal, outcome/error/publication, 생성 개수와 Output 이벤트를 확인합니다. 거부 시 최종 출력 폴더가 비어 있어야 하며 입력은 전후 SHA-256이 같아야 합니다. 성공 경계 표본은 해제 파일 길이와 모든 바이트가 0인지 확인합니다. 파일시스템 호출 추적은 하지 않으므로 일시적인 쓰기가 전혀 없었다고 증명하지는 않습니다. 표본의 payload는 모두 합성 데이터이며 사용자 압축파일은 사용하지 않습니다.

이 검증은 현재 크기 제한의 거부 동작을 확인합니다. 수 GB 해제 성공이나 작업자의 고정 메모리 상한, 디스크 고갈, 고아 프로세스 정리를 검증하는 시험은 아닙니다. Windows 실행은 별도 검증이 필요합니다.

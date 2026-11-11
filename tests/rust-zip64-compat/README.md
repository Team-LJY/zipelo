# ZIP64 호환 표본

PR74의 스트리밍 작업자를 실제 자식 프로세스로 실행해 모든 표본의 목록과 해제를 확인함.
제품 구현 변경 없이 별도 시험 crate에서 공개 runtime과 wire codec을 사용함.
표본과 결과는 실행 시 runs/에 생성하며 저장소에는 압축파일을 넣지 않음.

## 구성

총 1066개이며 모든 기대 결과는 목록과 해제 성공임.

- 원본 5개: Python 일반 ZIP, Python force_zip64의 탐색 가능/불가능 출력, libarchive zip64=1, Info-ZIP -fz 일반 파일 출력.
- 필드 조합 1024개: 중앙 디렉터리의 원래 크기, 압축 크기, 로컬 오프셋, 시작 디스크 번호 16조합과 EOCD의 현재 디스크, 중앙 시작 디스크, 디스크 내 항목 수, 전체 항목 수, 중앙 크기, 중앙 오프셋 64조합의 곱.
- 로컬 크기 두 필드가 함께 확장된 중앙 필드 조합 16개.
- ZIP64 종료 레코드 없이 중앙 필드만 확장한 조합 16개.
- 기존 회귀 표본 5개: 260바이트 강제 ZIP64, 서명 유무별 오프셋 전용 32비트 디스크립터, EOCD 디스크 번호 확장, 중앙 시작 디스크 번호 확장.

fields-cXX-eYY의 마스크 비트는 위 필드 나열 순서이며 최하위 비트부터 적용함.
디스크 번호의 실제 값은 항상 0이고 항목 수는 1임.
4 GiB 파일을 만들지 않고 sentinel을 사용해 작은 값의 확장 표현을 검사함.
실제 4 GiB 초과 오프셋, 다중 볼륨, 암호 ZIP은 이 묶음의 범위가 아님.

[APPNOTE 6.3.10](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT)의
4.4.13, 4.4.19~4.4.24, 4.5.3을 기준으로 대응 필드가 sentinel인 경우에만
중앙 ZIP64 확장값을 순서대로 넣음. 로컬 크기 확장은 두 크기 값을 함께 넣음.
도구 원본과 수동 필드 변형은 provenance.json에서 구분함.
Info-ZIP은 -fz와 stdout을 함께 사용할 때 이 환경에서 손상된 종료 레코드를 만들므로
정상 일반 파일 출력에 -fz를 적용함.

## 실행과 판정

Linux에서 Python 3, Info-ZIP zip, 시스템 libarchive가 필요함.
누락된 도구나 참조 판독 실패는 성공으로 건너뛰지 않음.
제품의 추가 의존성은 없음.

```sh
cargo +1.99.1 test --manifest-path tests/rust-zip64-compat/Cargo.toml --locked -- --nocapture
python3 tests/ci/run_rust.py local
```

지정 실행기는 tests/*/Cargo.toml을 검색하므로 별도 실행기 설정 없이 포함됨.
Windows 교차 빌드는 수행하지만 이 표본 실행 시험은 Linux 전용임.

Python zipfile로 전체 표본의 내용과 CRC를 먼저 확인함.
작업자 목록의 이름, 크기, ListId와 해제 결과의 전체 바이트, 게시 수, 정리 상태를 확인함.
일부 표본이 실패해도 나머지를 계속 실행하고 마지막에 시험을 실패시킴.
runs/<실행 ID>/results.tsv에 모든 표본의 목록/해제 결과가 남음.
같은 위치의 corpus/provenance.json에는 도구 버전과 표본 출처가 남음.

ZIPELO_ZIP64_WORKER 환경 변수로 비교할 작업자 실행 파일을 지정할 수 있음.
기본값은 같은 worker 의존성으로 빌드한 zip64-test-worker임.
이 환경 변수는 시험 코드에만 적용되며 제품 작업자에는 적용되지 않음.

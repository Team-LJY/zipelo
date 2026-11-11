# 악성 압축파일 회귀 표본

합성 표본 15개와 revision 2 작업자 요청의 거부 기대값을 담습니다. 제품 코드, codec 또는 작업자 동작은 수정하지 않습니다. 이 디렉터리의 검사는 표본 자체의 구조와 기대값을 검증하며 작업자의 실제 거부를 입증하지 않습니다.

- 경로: ZIP의 ../, POSIX 절대 경로, Windows 드라이브 절대 경로, 역슬래시 ../, TAR의 ../ 및 절대 경로, TAR.GZ의 ../.
- 링크: ZIP Unix 심볼릭 링크와 TAR 심볼릭 링크가 ../outside를 가리킨 뒤 link/escape.txt를 포함합니다.
- 압축 폭탄: 유효한 ZIP Deflate, GZ, TAR.GZ 스트림입니다. 실제 파일 내용은 64 KiB로 제한하고 요청의 output 한도를 1 KiB로 낮춥니다. TAR.GZ의 컨테이너를 포함해 해제 크기는 128 KiB 이하이며 입력 대비 100배 이상입니다. 거대한 파일이나 중첩 압축은 만들지 않습니다.
- 손상: ZIP의 두 번째 로컬 헤더 이름 길이 초과, TAR의 두 번째 헤더 체크섬 불일치, GZ의 예약 플래그 설정입니다.

ZIP/TAR 경로, 링크, 손상 표본과 ZIP/TAR.GZ 폭탄에는 정상 파일 before.txt를 먼저 넣었습니다. 잘못된 뒤쪽 항목을 발견하기 전에 앞쪽 파일을 게시하는 구현도 실패로 판정해야 합니다. GZ는 단일 스트림입니다.

## 기대 결과와 실행 조건

expectations.json의 request_defaults에 각 case의 limits와 gzip_output_name을 덮어써 Extract 요청을 구성합니다. file은 fixtures/ 아래의 입력 파일이며 실제 요청에는 절대 경로를 사용합니다. 목록 재사용 없이 Selection::All과 list_id=None으로 시작합니다. 각 case의 error_any_of는 허용하는 오류 코드이며 나머지 공통 expected 조건은 모두 충족해야 합니다. 링크 표본 2개는 UnsafePath 또는 Conflict를 허용합니다. 링크 아래 파일과 비디렉터리 부모의 충돌을 먼저 감지할 수 있으므로 오류 종류보다 쓰기 없는 거부를 확인합니다.

- 새 시험 디렉터리 안에 비어 있는 소유 destination과 sibling outside를 준비합니다. Linux의 새 디렉터리는 처음부터 0700으로 생성하며 기존 경로 권한을 변경하지 않습니다.
- 두 폴더와 입력 파일의 바이트, 이름, 링크, 메타데이터를 실행 전후 비교합니다. 작업자 프로세스는 제한된 시험 환경에서만 실행하고 사용자 파일이나 실제 압축파일은 사용하지 않습니다.
- 정확히 한 번의 Terminal Failed, publication=None, created_count=0, retained_partial_count=0, Output 이벤트 0개를 요구합니다. Terminal이 마지막 프레임이어야 하며 양쪽 EOF와 child wait/exit 0까지 확인합니다. exit 0만으로 성공 판정하지 않습니다.
- destination과 outside에 파일, 디렉터리, 링크 생성이나 수정, 삭제가 없어야 합니다. 임시 파일을 썼다가 지우는 것도 허용하지 않습니다. 최종 폴더 비교만으로 이를 입증할 수 없으므로 후속 작업자 검증에서는 출력 경로에 대한 파일시스템 호출 관측 또는 쓰기 계측이 필요합니다.
- 절대 경로 표본은 권한 오류가 나서 쓰지 못했다는 이유만으로 통과시키지 않습니다. 지정된 UnsafePath 거부와 쓰기 부재를 함께 확인합니다.
- 입력 파일이 바뀌거나 worker가 시간 초과, crash, 프로토콜 오류로 끝나면 거부 시험 통과로 처리하지 않습니다. 임의 추출 도구로 표본을 디스크에 풀지 않습니다.
- 심볼릭 링크는 실제 파일시스템 링크가 아니라 압축파일 안의 메타데이터입니다. 별도 조건인 기존 destination 부모 링크 교체 경합은 이 표본의 검증 범위가 아닙니다.

## 표본 검사

저장소 루트에서 실행합니다.

```sh
python3 -m unittest discover -s tests/ci -p test_malicious_archive_fixtures.py -v
python3 tests/ci/run_rust.py local
```

로컬 실행기의 자체 검사 탐색에 포함됩니다. 검사에서는 표본을 디스크에 추출하지 않고 크기가 제한된 메모리에서 ZIP CRC, TAR 헤더와 링크, GZ 스트림, 크기 한도 초과와 손상 지점을 확인합니다. 압축파일은 fixtures/에, 기대값과 SHA-256은 expectations.json에 있습니다.

```sh
python3 tests/malicious-archive-fixtures/generate.py
```

생성기는 Python 표준 라이브러리만 사용합니다. 현재 환경에서 재생성한 바이트가 저장된 표본과 같아야 합니다. 다른 Python/zlib 버전에서 압축 바이트가 달라지면 원인을 확인하고 검토 후 갱신하며 검사를 우회하지 않습니다. 생성기를 실행하면 이 디렉터리의 알려진 표본과 기대값만 갱신합니다.

worker_execution_verified=false는 생성기가 제공하는 기대값 자체를 작업자 실행 증거로 취급하지 않는다는 뜻입니다. 실제 작업자 검증 결과는 사용한 커밋과 함께 PR 본문에 기록합니다. 후속 리뷰에서는 사용한 작업자 커밋, 개별 표본 결과, 쓰기 관측 방식과 실행 환경을 별도로 보고해야 합니다. Windows 실기, 암호, 파일시스템 경합, OS sandbox 검증은 포함하지 않습니다.

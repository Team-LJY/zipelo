# Rust archive worker

별도 프로세스의 stdin/stdout으로 revision 3 요청을 처리합니다. 제품 앱이 공유 codec으로 연결하며 프로세스 분리는 OS sandbox를 뜻하지 않습니다. 암호 기능은 capability=true이며 ZipCrypto 해제, WinZip AES-256 AE-1/AE-2 해제와 AE-2 생성을 지원합니다.

- ZIP32/ZIP64 Store/Deflate 목록과 해제, ZIP32/ZIP64 생성, 기본 TAR/USTAR, GZ, TAR.GZ, TAR.BZ2, TAR.XZ, 비암호화 7z 해제를 지원합니다. GZ 연결 멤버는 하나의 논리 스트림으로 합칩니다. 다중 볼륨, UTF-8로 해석할 수 없는 ZIP 이름, PAX/GNU 확장과 특수 항목 해제는 미지원입니다.
- ZIP 이름: UTF-8 플래그(비트 11)가 있으면 이름이 UTF-8이어야 하며 아니면 Integrity입니다. 플래그가 없으면 CRC가 원래 이름과 맞는 Info-ZIP Unicode Path 확장 필드(0x7075, 버전 1)를 우선 쓰고, 다음으로 UTF-8로 유효한 이름 바이트를 그대로 씁니다(Linux/macOS 도구의 기본 동작). CRC가 다르거나 버전이 다른 확장 필드는 무시합니다. 그 밖의 CP437/CP949 등 레거시 이름은 코드 페이지를 추측하지 않고 UnsupportedFeature로 거부합니다. 어느 경로로 얻은 이름이든 같은 경로 안전성 검사와 충돌 검사를 거칩니다.
- ZIP/TAR/GZ/TAR.GZ/7z 목록과 해제는 입력 기본 한도 u64::MAX, 해제 총량 16 GiB, 항목 10,000개입니다. 기존 Start와 StartInteractive의 limits는 이 값을 낮출 수 있습니다. 별도 StartGuarded의 승인 흐름은 아래 절을 따릅니다. 64 KiB 버퍼와 임시 파일로 입력과 검증 완료 payload를 처리해 모든 출력의 CRC, 크기와 경로를 게시 전에 검사합니다. checkpoint마다 취소, 부모 프로세스 생존과 300초 한도를 확인합니다. ZIP 생성은 원본을 하나씩 스냅샷으로 읽어 스트리밍으로 압축하며 원본 크기 합계에 16 GiB 한도를 적용합니다. ZIP64 크기와 오프셋은 필요한 항목에만 기록합니다. slice 기반 archive parser는 입력 64 MiB/출력 128 MiB, slice ZIP builder는 원본 및 결과 128 MiB 한도를 유지합니다.
- 경로 깊이는 64개 구성 요소, 이름/출력 계획 메타데이터 예산은 8 MiB, 출력 계획 노드는 32,768개로 제한합니다. 이름 복제와 인덱스 비용을 보수적으로 계산해 할당 전에 Limit으로 거부합니다.
- GZip 내부 형식은 완전한 USTAR magic이 있을 때만 TAR로 판별합니다. magic이 없는 영값 데이터는 길이와 무관하게 raw GZip으로 유지하며 명시 출력명이 필요합니다.
- ListId는 열린 파일 객체, 전체 입력 SHA-256과 catalog version에 연결됩니다. 입력을 다시 읽어 검증한 전용 임시 파일 스냅샷만 해독합니다. 불일치는 출력 전 SourceChanged이며 목록을 다시 열어야 합니다. 원본의 사후 변경 감시나 파일 잠금 보장은 아닙니다.
- 출력 폴더는 미리 존재해야 합니다. Linux는 소유 UID가 같고 group/other 쓰기가 없는 폴더만 허용합니다. openat/O_NOFOLLOW로 디렉터리를 고정하고 새 임시 파일을 배타적으로 만든 뒤 hard link로 게시합니다. 기존 파일과 링크를 덮어쓰지 않습니다. Windows는 reparse point를 거부하고 부모 디렉터리 핸들에서 delete sharing을 제한합니다.
- 파일 sync/close는 게시 전에 확인합니다. 게시 후 부모 sync와 임시 파일 정리 실패는 게시 사실과 따로 보고합니다. Windows의 디렉터리 내구성은 Unconfirmed입니다. hard link의 결과가 불명확하면 CommitUncertain/Uncertain이며 자동 재시도나 rollback은 없습니다.
- 생성한 디렉터리와 게시한 파일은 취소 시 유지하고 Output으로 보고합니다. 실패한 임시 파일 정리는 실제 임시 이름과 PartialRetained로 보고합니다. 임시 파일 생성과 일부 쓰기 뒤 ENOSPC, 파일 sync와 교체 호출 실패, 게시 후 디렉터리 sync 실패 주입 시험은 Linux에서 수행합니다. close 장애 주입과 Windows 실기 검증은 아직 수행하지 않았습니다.

## 스트리밍과 자원 정리

revision 2의 기존 필드와 오류 tag 0~12 및 입력/출력 기본 한도를 유지하고, 히나타 님과 합의한 공간 부족 오류 DiskFull(tag 13)을 추가합니다. 구버전 앱은 새 tag를 거부하므로 앱 오류 변환과 화면 매핑을 작업자와 함께 갱신해야 합니다. 앱이 공유하는 Limits::default()도 같은 값을 사용합니다. 세부 계약은 contract.json의 streaming_limits에 있습니다.

- 기존 Start와 StartInteractive는 압축비를 거부 조건으로 사용하지 않습니다. ZIP 선언 크기와 실제 해제 크기, CRC와 AES 인증을 검사하며, GZ 연결 멤버의 전체 해제량도 16 GiB 총량 한도에 포함합니다. TAR.GZ의 헤더와 padding도 계산합니다. u64 누계와 offset은 checked 연산을 사용합니다.
- 입력 캡처 전에 임시 저장소 여유 공간을 확인합니다. 기존 Start는 ZIP/7z 해제 전에 전체 임시 해제량과 선택한 출력량을 볼륨별로 합산해 검사하고, 게시 전에도 출력 공간을 재검사합니다. StartInteractive는 선택 전에는 무결성 검증에 필요한 임시 해제 공간만 확인하고, 선택 뒤 건너뛰지 않은 파일의 실제 저장 이름으로 게시 직전 공간을 확인합니다. 따라서 목적지 공간이 부족해도 충돌 파일을 전부 건너뛰거나 질문 중 취소할 수 있습니다. 미리 해제 크기를 알 수 없는 GZ/TAR.GZ는 각 해제 청크를 쓰기 전에 임시 저장소 공간을 확인합니다. Linux 64비트는 열린 핸들의 [fstatvfs](https://man7.org/linux/man-pages/man3/fstatvfs.3.html), Windows는 핸들에서 얻은 디렉터리의 [GetDiskFreeSpaceExW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getdiskfreespaceexw)를 사용합니다. 공간 부족은 DiskFull(13), 조회 실패와 권한 등 다른 I/O 오류는 Io(8)로 반환합니다. 실제 생성/쓰기/동기화의 공간 부족에도 같은 구분을 적용합니다. Windows의 공간 부족 코드 39와 112는 [공식 오류 목록](https://learn.microsoft.com/en-us/windows/win32/debug/system-error-codes--0-499-)을 따릅니다.
- 공간 검사는 논리적 파일 크기에 따른 사전 확인이며 예약이나 모든 메타데이터/할당 단위 비용의 보장이 아닙니다. 다른 작업과 경쟁해 나중에 ENOSPC가 발생하면 기존 부분 출력 정리 정책을 적용합니다. 16 GiB 안에서는 압축비가 매우 큰 파일도 디스크에 풀릴 수 있습니다. 오류 문구는 "디스크 여유 공간이 부족합니다. 공간을 확보한 뒤 다시 시도하세요"이며, detail 문자열을 오류 판정에 사용하지 않습니다.
- 입력과 검증된 해제 내용은 임시 파일에 저장합니다. Linux는 [O_TMPFILE](https://man7.org/linux/man-pages/man2/open.2.html), 이를 지원하지 않는 Linux 파일시스템에서는 0600 난수 파일을 배타적으로 만든 즉시 unlink하고 링크 수 0을 확인합니다. Windows는 배타적 [DELETE_ON_CLOSE 파일](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)을 사용합니다. 마지막 핸들이 닫히면 제거되며 임시 파일명에 아카이브 내부 이름을 사용하지 않습니다. 디스크에는 입력 사본, 해제 내용과 현재 출력 임시 파일이 동시에 필요할 수 있습니다. 임시 폴더가 tmpfs이면 해당 파일 내용은 프로세스 RSS 외의 시스템 메모리를 사용합니다.
- 64 MiB는 ZIP/TAR/GZ/TAR.GZ/7z 해제와 ZIP 생성 작업자 프로세스의 피크 RSS 시험 목표입니다. OS 메모리 강제 격리나 모든 아카이브의 보장을 뜻하지 않습니다. 입력 크기와 무관한 메타데이터 예산은 유지합니다.
- Linux는 시작 시 부모 PID를 캡처하고 [getppid](https://man7.org/linux/man-pages/man2/getpid.2.html) 변경을 확인합니다. Windows는 부모 프로세스 핸들과 생성 시간을 확인한 뒤 [WaitForSingleObject](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitforsingleobject)로 생존을 검사합니다. 부모가 사라지면 checkpoint에서 Io로 중단하고 임시 출력을 정리합니다. 이미 게시한 파일/디렉터리는 유지하며 OS의 블로킹 I/O 강제 중단을 보장하지 않습니다. 기존 단발 요청을 위해 stdin EOF만으로 취소하지 않습니다.
- tests/rust-archive-worker/hardening.rs는 공용 대용량 생성기를 사용해 Store ZIP/Deflate ZIP/TAR/GZ/TAR.GZ의 1 GiB payload를 실행 시 생성하고 wait4의 ru_maxrss로 피크 RSS를 측정합니다. Linux의 시험 전용 LD_PRELOAD로 ENOSPC와 게시 중 정지를 주입하고 앱 강제 종료 후 작업자 종료/회수와 부분 출력 제거를 확인합니다. 이 시험을 위해 실제 공유 디스크를 채우거나 제품용 환경 변수 우회를 추가하지 않습니다.

## 7z 해제

비암호화 7z의 단일 입력/출력 coder인 Copy, LZMA와 LZMA2를 Rust 코드로 해독합니다. solid substream, 빈 파일/디렉터리, UTF-16LE 이름과 한 단계 압축 헤더를 처리합니다. 추가 라이브러리나 제품 외부 명령을 사용하지 않습니다. 형식 근거는 [7z 명세](https://www.7-zip.org/7z.html)와 [LZMA SDK 명세 안내](https://www.7-zip.org/sdk.html)입니다.

- 시작 헤더/다음 헤더 CRC, packed/folder/substream CRC가 있을 때 해당 CRC와 크기, 스트림의 끝을 검증합니다. 경로, 충돌과 모든 선택 내용 검증이 끝난 뒤 기존 안전 게시 경로로 넘깁니다. 목록은 데이터 CRC 검증 성공을 뜻하지 않습니다.
- 기본 입력 한도 u64::MAX, 해제 총량 16 GiB와 기존 항목/이름 한도를 적용합니다. 압축 해제된 헤더도 최대 8 MiB이고 헤더 중첩은 한 단계입니다. 사전은 최대 64 MiB, LZMA의 lc+lp는 최대 4입니다. 최근 8 MiB 사전만 메모리에 두고 더 먼 참조는 임시 파일에서 읽습니다. 먼 거리 참조가 많은 파일은 추가 디스크 I/O가 발생합니다. 해독 중 checkpoint에서 취소와 시간 한도를 확인합니다.
- 암호화 7z, 다중 coder 필터 체인(BCJ/BCJ2/Delta 등), PPMd/BZip2, 분할/SFX, external metadata, additional streams와 anti 항목은 미지원입니다. 링크, reparse point와 특수 Unix 항목은 거부합니다. 지원하지 않는 구조를 성공으로 처리하지 않습니다.
- solid 그룹의 압축 크기는 첫 substream에만 기록하고 나머지는 0으로 표시합니다. 각 파일의 정확한 비압축 크기는 유지합니다.
- 합성 표본은 Python 표준 lzma로 생성합니다. 별도로 시스템 libarchive가 만든 LZMA1/LZMA2 표본을 해제하고, 합성 표본도 libarchive 읽기 API로 검사합니다. libarchive는 Linux 시험의 독립 참조 도구이며 제품 의존성이 아닙니다. 시험 도구와 표본은 tests/rust-archive-worker 아래에 있습니다.
- 형식 확장은 히나타 님과 합의한 Format::SevenZip=4입니다. 기존 값 0~3과 필드, 메시지 tag, 프레임 version은 유지합니다. 구버전 앱은 값 4를 거부하므로 앱 Format과 양방향 변환도 갱신해야 합니다. 이 백엔드 변경은 frontend 코드를 수정하지 않습니다.

## 압축 TAR 해제

TAR.BZ2와 TAR.XZ는 확장자가 아닌 BZh 및 XZ 매직으로 판별하고 비공개 임시 파일로 먼저 해제합니다. 연결된 BZip2/XZ 스트림은 하나의 TAR로 합치며 XZ의 4바이트 정렬 스트림 패딩도 허용합니다. 빈 TAR, USTAR 이름과 빈 폴더를 지원하지만 독립 BZ2/XZ 데이터 파일은 TAR가 아니면 거부합니다.

해제 총량은 TAR 헤더와 패딩을 포함해 기존 16 GiB 이하이며 요청으로 낮출 수 있습니다. 64 KiB씩 실제 해제량을 확인하고 기록 전에 임시 저장소의 공간을 검사합니다. XZ는 블록별 LZMA2 디코더 메모리 32 MiB, 인덱스 8 MiB, 스트림 수와 스트림당 블록 수 각각 32,768을 상한으로 둡니다. 이 상한을 넘는 정상 파일도 Limit로 거부합니다. 해제 전체와 TAR 경로, 항목 종류 및 이름 충돌을 검증하기 전에는 목적지에 게시하지 않습니다. 출력 공간 검사, 안전한 충돌 처리, 취소와 300초 시간 제한도 기존 흐름을 사용합니다.

XZ의 헤더, 인덱스, 푸터 CRC와 역방향 크기 및 전체 선언 해제량을 확인하고 실제 해제량과 비교합니다. 각 블록의 헤더와 LZMA2 청크 경계를 따라 압축 길이와 해제량을 인덱스 레코드 및 선택적 헤더 크기와 대조하므로 다른 블록이나 연결 스트림의 크기 변조가 상쇄되어도 Integrity로 거부합니다. 지원하는 BCJ와 Delta 필터는 길이를 보존합니다. 이 사전 검사는 payload를 해제하거나 할당하지 않으며 기존 디코더가 청크 내용과 데이터 블록의 CRC32/CRC64/SHA256 검사를 수행한 뒤에만 게시합니다. [XZ 명세 4.3절](https://tukaani.org/xz/xz-file-format.txt)의 블록별 Unpadded Size와 Uncompressed Size 검사를 적용합니다. XZ의 Check=None도 형식대로 허용하므로 데이터 체크섬이 없는 파일의 내용 무결성을 보장하지 않습니다. TAR의 PAX/GNU 확장, 링크와 특수 항목의 기존 제한도 유지합니다.

통신에는 기존 Format::Tar=1을 사용합니다. 압축 방식별 새 형식 값이나 메시지 변경은 없으며 목록 식별자는 원본 객체, 길이와 SHA256을 계속 포함합니다. 화면의 파일 선택 필터와 지원 안내, 새 형식의 UI 상황 추가는 frontend 연동 범위입니다.

의존성 선택 근거는 다음과 같습니다.

- [bzip2 0.6.1](https://docs.rs/crate/bzip2/0.6.1/source/Cargo.toml)은 MIT OR Apache-2.0이며 기본 백엔드가 Rust 구현인 libbz2-rs-sys입니다. 고정 lockfile은 libbz2-rs-sys 0.2.6(bzip2-1.0.6 자유 라이선스)을 선택하고 Rust 할당자를 사용합니다. C 구현인 bzip2-sys 기능과 stdio/C 할당자는 활성화하지 않습니다.
- [lzma-rust2 0.25.0](https://docs.rs/lzma-rust2/latest/lzma_rust2/struct.XzReader.html)은 Apache-2.0의 Rust 구현입니다. std와 xz만 활성화해 인코더와 다중 스레드 구현을 제외하고 사전 할당 전 메모리 제한을 적용합니다. CRC와 인덱스 레코드 개수를 검증하며 연결 스트림을 끝까지 읽습니다. 레코드별 크기는 작업자가 별도로 대조합니다.
- 두 라이브러리와 새 전이 의존성의 원문은 Cargo 체크섬을 대조하는 기존 packaging/licenses.py가 고지에 자동 포함합니다. tests/release-packages/test_compressed_tar_licenses.py는 실제 고지에 세 압축 라이브러리의 원문과 해시가 포함되고 C 압축 백엔드가 없는지 확인합니다.
- 2026-11-13 [RUSTSEC-2023-0004](https://rustsec.org/advisories/RUSTSEC-2023-0004.html)의 bzip2 DoS 권고와 수정 범위(>=0.4.4)를 확인했습니다. 고정 lockfile의 bzip2 0.6.1은 수정 범위에 해당하므로 이 권고의 영향을 받지 않습니다. 새 전이 sha2 0.11.1은 RUSTSEC-2021-0100의 수정 범위(>=0.9.8)에 해당합니다. 전체 최신 DB 자동 감사와 취약점 부재를 보장하는 확인은 수행하지 않았습니다.

Python 표준 bz2, lzma와 tarfile로 독립 생성 및 해제한 결과의 SHA256과 실제 작업자 결과를 비교합니다. tests/rust-archive-worker/compressed_tar.rs는 정상, 빈 TAR, 연결 스트림, XZ 필터와 체크 방식, 잘림과 손상, 푸터/인덱스 크기, 큰 사전, 작은 출력 한도의 압축 폭탄, 한도 경계, 경로 이탈, 링크, 목록 식별자와 선택, 취소 및 디코더 임시 공간 실패를 검증합니다.

## 암호 ZIP

- AES는 [WinZip AE-1/AE-2 명세](https://www.winzip.com/en/support/aes-encryption/)의 method 99, 0x9901 추가 필드, PBKDF2-HMAC-SHA1 1000회, 16바이트 salt, AES-256 CTR와 HMAC-SHA1-80 형식을 사용합니다. 새 파일은 CRC를 노출하지 않는 AE-2입니다. AE-1은 인증과 CRC를 모두 검증합니다. AES-128/192와 PKWARE Strong Encryption은 미지원입니다.
- AES 인증값을 상수 시간 비교로 확인한 뒤 복호화/압축 해제합니다. 모든 파일의 인증, CRC, 크기와 경로 검증이 끝나기 전에는 출력하지 않습니다. ZIP 파일명과 목록 메타데이터는 암호화되거나 인증되지 않으며, 목록 성공은 파일 내용 인증 성공을 의미하지 않습니다.
- [ZipCrypto](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT)는 기존 파일 해제만 지원합니다. 보안성이 약하며 암호학적 인증이 없습니다. 12바이트 헤더 검사 후 압축 스트림, 크기와 CRC를 확인합니다. 새 암호 ZIP은 AES-256만 생성합니다.
- 목록은 암호 없이 표시합니다. 해제 시 한 번 입력한 암호를 ZIP 전체에 적용하며 WrongPassword일 때 새 challenge_id로 최대 3회 요청합니다. 항목마다 다른 암호인 ZIP은 지원하지 않습니다. 헤더 확인값이 우연히 일치한 잘못된 암호는 Integrity로 끝날 수 있습니다. 인증 실패 후에는 다시 요청하거나 쓰지 않습니다.
- 생성은 PasswordPurpose::Encrypt로 한 번 요청하며 빈 암호는 거부합니다. 암호는 최대 1024 UTF-8 바이트입니다. 대기 중 Cancel도 처리하고, 답변 없이 입력 EOF가 오면 Protocol로 종료합니다. 기존 300초 작업 제한에 암호 입력 대기 시간도 포함됩니다.
- salt는 Linux /dev/urandom 또는 Windows BCryptGenRandom에서 얻으며 실패 시 생성을 중단합니다. 암호, 파생 키와 메모리 복호화 버퍼는 zeroize로 정리하고 AES 키 일정의 zeroize 기능을 켭니다. HMAC 내부 상태, OS 메모리/페이지 캐시 복사본과 임시 저장소의 평문에 대한 완전 소거를 보장하지 않습니다. 임시 평문 파일은 작업 종료 시 닫아 제거하며 보안 삭제를 뜻하지 않습니다.
- Python hashlib/hmac과 OpenSSL로 만든 AES 표본 4개, Info-ZIP으로 만든 ZipCrypto 표본 2개와 UTF-8 플래그 없이 한글 이름을 기록한 Info-ZIP 3.0 표본 4개(다중 항목 ZipCrypto 2개 포함)를 tests/ 아래에 보관합니다. 생성한 AES ZIP도 Linux에서 독립 Python/OpenSSL 검증기로 확인합니다. OpenSSL/Info-ZIP은 시험 도구이며 제품 의존성이 아닙니다.

대용량 7z 표본은 Python LZMA로 실행 시 생성하고 Linux 시스템 libarchive의 스트리밍 판독으로 내용과 CRC를 먼저 확인합니다. libarchive는 시험에만 사용합니다.

7z의 Copy/LZMA/LZMA2 입력과 solid 출력은 고정 버퍼와 임시 파일로 처리합니다. 모든 폴더와 하위 스트림의 CRC와 크기 검증이 끝난 뒤 기존 게시 경로를 사용합니다. ZIP 생성의 압축 및 AES 처리도 64 KiB 버퍼를 사용하며 여유 공간 사전 검사와 실제 쓰기의 DiskFull 구분을 적용합니다. 공간 예약을 보장하지 않으므로 다른 프로세스와의 경쟁으로 쓰기가 실패할 수 있습니다.

## ZIP 생성 입력

원본에 일반 파일과 폴더를 함께 지정할 수 있습니다. 각 archive_name을 최상위 이름으로 쓰고 폴더 안의 상대 경로와 빈 폴더를 보존합니다. 재귀 수집된 파일과 폴더 모두 항목 수 한도에 포함하며 파일 크기 합계에는 기존 16 GiB 기본 한도를 적용합니다. 이름 충돌과 한도는 암호 요청 전에 검사합니다.

심볼릭 링크, Windows reparse point와 특수 파일은 건너뛰거나 따라가지 않고 작업 전체를 거부합니다. 열린 디렉터리 기준으로 수집하고 파일은 검증한 핸들에서 캡처하며, 감지된 원본 교체나 변경은 게시 전 SourceChanged로 끝납니다. 전체 폴더 트리의 원자적 스냅샷을 보장하지는 않습니다. AES-256은 파일 내용에 적용하고 디렉터리 항목은 0바이트 비암호화 항목으로 기록합니다.

## 공유 codec

`protocol.rs`는 요청과 이벤트 타입, `wire.rs`는 LE codec이며 비밀 값이 담긴 프레임 버퍼는 zeroize로 정리합니다. 앱은 이 라이브러리의 타입과 Reader/Writer를 사용할 수 있습니다. `contract.json`은 합의한 revision 3 원문이며 `extract_conflicts.status`와 `guarded_resources.status`에서 구현 및 화면 연동 상태를 구분합니다.

헤더는 ZAW1, version u16=1, kind u16, body length u32, reserved u32=0, job ID u64, sequence u64의 32바이트입니다. 양방향 sequence는 각각 0부터 시작합니다. body는 최대 1 MiB이며 한 방향당 최대 1,000,000프레임입니다. 잘린 프레임, 알 수 없는 tag, 잔여 payload, 잘못된 ID/sequence를 거부합니다.

- kind: Start=1, Password=2, Cancel=3, StartInteractive=4, ConflictReply=5, StartGuarded=6, ResourceReply=7, Accepted=100, Entry=101, ListEnd=102, PasswordRequired=103, Progress=104, Output=105, Terminal=106, CancelAck=107, ConflictRequired=108, ConflictResolved=109, ResourceRequired=110입니다. 충돌 및 자원 승인의 지원 상태는 아래 계약 절을 따릅니다.
- body 필드는 `protocol.rs` 선언 순서입니다. enum은 선언 순서의 u8 tag이며 NativePath/SourceObject만 Unix=1, Windows=2입니다. bool/Option은 0 또는 1입니다. String/Vec는 u32 길이 뒤 원소를 저장하며 String은 UTF-8 바이트 길이입니다. `[u8; N]`은 길이 없이 N바이트입니다. action variant의 필드는 선언 순서를 사용합니다. Start는 action 뒤 limits(input u64, output u64, entries u32)입니다.
- Password의 비밀 값은 Debug에 노출되지 않습니다. 암호 작업은 PasswordRequired의 challenge_id와 일치하는 Password를 한 번 받습니다. 오래된 ID, 중복 또는 요청 전 암호는 Protocol 오류입니다. 비밀 값은 로그나 파일에 저장하지 않습니다.
- CancelAck.request_sequence는 요청 헤더 sequence입니다. Requested, AlreadyRequested, TooLate, Finished는 최종 결과와 별개입니다. 종료와 경합한 요청은 읽히지 않아 응답이 없을 수 있습니다. Terminal은 마지막 프레임이며 Failed/Cancelled도 정상 전달 뒤 프로세스 exit=0입니다. 앱은 Terminal과 stdout/stderr EOF, child wait를 모두 확인해야 합니다.

## revision 3 충돌 계약

작업자는 `StartInteractive`로 시작한 압축 해제에서 덮어쓰기, 건너뛰기와 이름 바꿔 저장을 처리합니다. 기존 `Start`는 이름 충돌 시 중단합니다. 앱의 활성화와 실제 UI 연동은 별도 PR에서 검증합니다.

- `StartInteractive(Start)`는 kind 4이며 기존 Start body를 그대로 사용합니다. Extract만 허용하고 List와 CreateZip은 codec에서 거부합니다. 기존 Start=1의 충돌 시 중단 동작과 모든 기존 필드 및 tag는 유지하며 구버전 작업자로 자동 재시도하지 않습니다.
- `ConflictChoice`는 Overwrite=0, Skip=1, Rename=2입니다. `ConflictReply`(5)는 challenge_id u64, choice u8, apply_to_remaining bool 순서입니다.
- `ConflictRequired`(108)는 challenge_id u64, entry_id u32, relative_name String, incoming_size u64, existing_size u64 순서입니다. 일반 파일끼리의 충돌에 세 선택을 허용하며 폴더끼리는 합치고 파일과 폴더 충돌 및 링크는 차단합니다.
- `ConflictResolved`(109)는 entry_id u32, relative_name String, choice u8 순서입니다. 일괄 적용에도 항목마다 보내며 저장 성공을 뜻하지 않습니다. 교체와 이름 변경의 게시 성공은 기존 `Output::Created`로 별도 확인하고 건너뛰기에는 Created를 보내지 않습니다.
- Rename 이름은 작업자가 같은 폴더 안에서 `파일명 (1).확장자` 형태의 미사용 번호 이름으로 정합니다. 기존 이름과 뒤에 처리할 항목의 경로도 피하며, 긴 이름은 UTF-8 경계를 보존해 255바이트 안으로 줄입니다. 일괄 적용은 현재 작업의 남은 일반 파일 충돌에만 적용합니다. 질문은 한 번에 하나이며 잘못되거나 중복된 응답은 Protocol, 답변 전 EOF도 Protocol입니다. 취소를 처리하고 기존 300초 제한에 대기 시간을 포함합니다.
- 전부 건너뛰고 새 출력도 없으면 Completed, Publication::None, created_count=0, Durability::NotApplicable입니다. 새 폴더가 만들어졌다면 해당 Output과 실제 집계를 유지합니다. 교체는 created_count에 포함하며 건너뛰기는 제외합니다.
- 런타임은 전체 무결성 검증 후 질문하고 목적지 임시 파일 기록, 검증, sync와 close를 마친 뒤 교체합니다. 교체 전 실패에는 원본을 보존하고 교체 후 오류에는 실제 게시, 내구성과 정리 상태를 보고합니다. 전체 작업의 원상 복구를 약속하지 않습니다.

덮어쓰기 직전에 기존 파일과 부모의 식별자, 크기와 수정 시각을 다시 비교하며 변경이 감지되면 Conflict로 중단합니다. Linux에서는 고정 부모의 renameat, Windows에서는 같은 폴더의 MoveFileExW(REPLACE_EXISTING)를 사용하며 원본을 먼저 지우지 않습니다. 기존 파일의 다른 hard link는 수정하지 않습니다. 같은 계정의 동시 수정과 검사 직후 경합을 원자적으로 비교하거나 전체 작업을 되돌리는 기능은 아닙니다. Windows의 폴더 내구성은 기존처럼 Unconfirmed이며 전원 차단 뒤 복구를 보장하지 않습니다.

교체 동작의 근거는 [Linux rename](https://man7.org/linux/man-pages/man2/rename.2.html)과 [Microsoft MoveFileExW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw)입니다.

## 큰 작업의 명시적 승인 계약

`StartGuarded`는 기존 요청과 분리된 선택적 확장입니다. 작업자 구현만 포함하며 앱에서 새 요청과 승인 화면을 사용하는 연동, `--ui-scenario` 및 가이드 갱신은 frontend 범위입니다. 기존 앱의 요청에는 새 질문을 보내지 않습니다. 구버전 작업자가 새 요청을 거부하면 자동 재시도하지 않습니다.

- kind 6의 본문은 `GuardedStart { start: Start, ceiling: Limits, interactive: bool }` 순서입니다. start.limits는 기존 16 GiB 및 10,000개 이하이고 ceiling은 이를 늘리되 64 GiB 및 200,000개 이하입니다. 입력 한도는 동일해야 합니다. interactive는 Extract에서만 허용하며 기존 암호와 충돌 처리를 함께 사용합니다.
- 작업자는 Accepted 뒤, 입력 캡처와 소스 탐색 전에 요청된 상한 확대 범위를 확인합니다. 입력 사본 작성부터 300초를 넘을 수 있으므로 승인을 먼저 받습니다. 이때 예상량과 실제량은 아직 미확인(None)이고 임시 저장 볼륨의 여유 공간을 알립니다. 허용 전에는 확대 한도가 적용되지 않으며 거부하면 Cancelled로 끝납니다. 작은 기존 작업은 Start를 계속 사용할 수 있습니다.
- kind 110의 `ResourceRequired`는 challenge_id, action, scope, usage, soft_limits, ceiling 순서입니다. `ResourceScope`는 output, entries, ratio의 bool 세 개입니다. `ResourceUsage`는 estimated_bytes, actual_bytes, estimated_entries, actual_entries, compressed_bytes, ratio_output_bytes, available_bytes 순서이며 모두 Option입니다. 바이트는 u64, 항목 수는 u32입니다. 미확인은 0으로 바꾸지 않습니다. 여유 공간은 관찰 중인 저장 볼륨의 값이며 공간 예약이나 모든 볼륨의 합계가 아닙니다.
- kind 7의 `ResourceReply`는 challenge_id u64, scope, allow bool입니다. 프레임의 작업 ID, 현재 질문 ID와 요청한 scope가 모두 일치하는 응답 한 개만 허용합니다. 중복, 오래된 응답과 다른 종류의 질문 응답은 Protocol입니다. 승인은 해당 작업의 해당 범위에만 적용하며 재사용하지 않습니다.
- 해제량이 256 MiB 이상이고 압축률이 1,000배를 초과하면 ratio 범위를 별도로 묻습니다. ZIP은 항목별, 7z는 solid 폴더와 전체, 압축 TAR와 GZip은 스트림 전체의 크기로 검사합니다. 예상량과 실제량을 구분하고 u128 곱셈으로 비교합니다. 승인 중에는 추가 해제와 쓰기를 멈추며 취소는 별도 제어 읽기에서 처리합니다.
- 승인 후에도 64 GiB와 200,000개 정책 상한, 메타데이터와 사전 예산, 경로 및 무결성, 임시 저장소와 목적지 공간 검사를 적용합니다. ZIP 생성은 원본 논리 크기 합계를 제한하며 ZIP 구조와 암호화로 늘어나는 저장 공간을 별도로 계산합니다. 이미 게시한 출력은 실패나 취소 시 유지하고 게시 및 정리 상태를 보고합니다.
- 승인된 작업은 전체 300초 대신 진행이 60초 동안 없을 때 Limit로 중단합니다. 실제 읽기, 해독, 쓰기와 메타데이터 수집만 진행으로 계산하며 반복 check 호출은 기한을 늘리지 않습니다. 암호, 충돌과 자원 질문은 한 번에 하나이며 각 응답 대기는 300초입니다. 유효한 응답 뒤에는 사용자 대기 시간을 제외하고 정체 감시를 재개합니다. 기존 요청의 전체 300초 제한은 유지합니다. OS의 블로킹 I/O를 강제로 중단하는 보장은 아닙니다.

현재 남은 제약: 기존 8 MiB 메타데이터 예산과 32,768개 노드 상한을 유지하므로 실제 항목 200,000개까지 처리할 수 있다는 뜻은 아닙니다. 소스 경로도 이 예산을 사용합니다. 메타데이터 저장 방식과 예산 결정, 정확한 codec 인계 검토 및 화면 연동 전까지 초안을 유지합니다.

시험은 실제 64 GiB를 쓰지 않고 수치 경계와 축소한 limits/ceiling을 사용합니다. `resource_budget.rs`는 16 GiB 아래/경계/초과, 64 GiB 초과, 10,000개 및 200,000개 경계, 압축률 경계와 가상 시계의 60초 정체를 검사합니다. `guarded.rs`는 실제 작업자 승인/거부, 오래된 응답, 암호/충돌 순서, 승인 뒤 공간 실패, 여러 형식과 작은 고압축 입력의 경고 및 취소를 검사합니다.

## 검증과 의존성

시험은 `tests/rust-archive-worker`에 있습니다. Python 표준 라이브러리로 만든 ZIP/TAR/GZ fixture와 실제 자식 프로세스를 사용합니다. 전체 검증 명령은 저장소 루트의 `python3 tests/ci/run_rust.py local`입니다. Windows 교차 빌드는 실제 Windows 실행 검증을 대신하지 않습니다.

직접 의존성은 기존 zlib-rs=0.6.8와 허용된 RustCrypto sha2=0.10.9입니다. 2026-11-11에 확인한 [zlib-rs RUSTSEC-2024-0401](https://rustsec.org/advisories/RUSTSEC-2024-0401.html)은 >=0.4.0, [sha2 RUSTSEC-2021-0100](https://rustsec.org/advisories/RUSTSEC-2021-0100.html)은 >=0.9.8을 수정 버전으로 명시합니다. 선택 버전은 이 수정 범위에 들어갑니다. 이는 전체 전이 의존성의 취약점 부재나 완전한 보안 감사를 의미하지 않습니다.

암호 구현에 RustCrypto aes=0.8.4(zeroize 기능), hmac=0.12.1, pbkdf2=0.12.2, sha1=0.10.7, zeroize=1.9.1과 상수 시간 비교용 subtle=2.6.1을 추가했습니다. SHA-1은 ZIP 호환용 HMAC/KDF에만 사용합니다. 고정 버전과 전이 의존성은 Cargo.lock에 있습니다. 로컬 RustSec DB(1645308, 2026-10-23)의 해당 패키지 기록과 공개 RustSec 검색을 확인했지만 cargo-audit 명령이 없어 최신 DB 전체 자동 감사는 수행하지 못했습니다. 이 확인은 취약점 부재나 전체 보안 감사의 증거가 아닙니다.

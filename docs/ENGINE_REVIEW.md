# 엔진 검토와 백엔드 기반 상태

## 적용 범위

Zipelo 자체 코드는 MIT 방침이다. 외부 엔진은 아직 채택하지 않았으며 엔진 파일이나 패키지를 프로젝트에 추가하지 않았다. 암호화 RAR와 구형 솔리드 RAR는 검증 중이며 제품 미지원으로 확정하지 않는다.

이번 기록은 SharpCompress 0.50.7의 소스, 패키징 설정과 공개 패키지 목록 및 고지를 대조하고 unar/XADMaster의 공식 소스와 Windows 배포 근거를 추가 확인한 결과다. 최신 버전이나 바이너리 실행 검증 완료를 의미하지 않는다.

## SharpCompress 0.50.7

### 공개 패키지 실물 확인

2026-11-09에 [공식 NuGet 패키지](https://api.nuget.org/v3-flatcontainer/sharpcompress/0.50.7/sharpcompress.0.50.7.nupkg)를 메모리로 내려받아 ZIP 항목 목록과 nuspec, README만 읽었다. 디스크에 저장하거나 압축을 풀지 않았고 DLL 로드, 설치와 코드 실행은 하지 않았다.

- 크기: 6,702,991바이트.
- SHA-256: `115f4157740c6690b0315d3dea7fa900e9eaccd395b4cee8019c1fbbf94923de`.
- 항목은 12개다. `_rels/.rels`, `SharpCompress.nuspec`, `README.md`, `[Content_Types].xml`, `package/services/metadata/core-properties/nuget.psmdcp`, `.signature.p7s`와 아래 DLL 6개다.
- DLL 경로는 `lib/{net10.0,net48,net6.0,net8.0,netstandard2.0,netstandard2.1}/SharpCompress.dll`이다.
- 별도 `LICENSE.txt`, NOTICE, C# 소스, `reference/unrar` 또는 네이티브 unrar DLL은 항목 목록에 없다. DLL 내부의 포함 디버그 정보와 소스는 조사하지 않았으므로 바이너리에 관련 구현이 없다는 의미는 아니다.
- 서명 파일은 존재하지만 인증서 신뢰 체인이나 패키지 서명을 검증하지 않았다. 위 해시는 이번에 읽은 바이트를 식별하며 진위 검증 완료를 뜻하지 않는다.

nuspec은 버전 0.50.7, MIT license expression, Adam Hathcock의 2025년 저작권, 저장소 커밋 `b4af8afe6683d019d504954a3b4e4f18b06e2cd6`을 기록한다. [0.50.7 태그의 커밋](https://github.com/adamhathcock/sharpcompress/commit/b4af8afe6683d019d504954a3b4e4f18b06e2cd6)과 이 값이 일치한다. 메타데이터의 소스 연결은 확인했지만 재현 빌드로 DLL 동일성을 검증하지 않았다. 최신 버전이라는 의미도 아니다.

### 패키징과 고지

[프로젝트 설정](https://github.com/adamhathcock/sharpcompress/blob/0.50.7/src/SharpCompress/SharpCompress.csproj)은 같은 대상 프레임워크 6개와 MIT를 선언하고 README를 패키지 루트에 추가한다. [Directory.Build.props](https://github.com/adamhathcock/sharpcompress/blob/0.50.7/Directory.Build.props), [배포 워크플로](https://github.com/adamhathcock/sharpcompress/blob/0.50.7/.github/workflows/nuget-release.yml), [빌드 프로그램](https://github.com/adamhathcock/sharpcompress/blob/0.50.7/build/Program.cs)의 pack 대상도 해당 C# 프로젝트다. 읽은 설정에 reference/unrar의 C++ 프로젝트를 빌드하거나 패키지에 넣는 경로는 없다. DLL의 embedded debug 설정이 있어 외부 파일 목록만으로 내부 정보를 모두 판단하면 안 된다.

실물 nuspec에서 net6.0, net8.0, net10.0은 외부 패키지 의존성이 없다. net48, netstandard2.0, netstandard2.1은 Microsoft.Bcl.AsyncInterfaces와 System.Text.Encoding.CodePages 8.0.0 이상을 요구한다. Zipelo의 현재 net10.0 가정에서는 이 구형 대상 의존성이 선택되지 않는다. 구형 대상으로 바꾸면 실제 해결된 버전과 전이 의존성의 라이선스를 별도 확인해야 한다. 이번에는 의존 패키지를 다운로드하거나 고지 전체를 감사하지 않았다.

실물 README에는 XZ.NET, managed-lzma, ZstdSharp의 출처와 Bouncy Castle 저작권 및 허가 고지가 들어 있다. [태그 README](https://github.com/adamhathcock/sharpcompress/blob/0.50.7/README.md)에서도 이 내용을 확인했다. [루트 LICENSE.txt](https://github.com/adamhathcock/sharpcompress/blob/0.50.7/LICENSE.txt)는 Adam Hathcock의 2014년 MIT 고지다. nuspec의 MIT 링크만으로 저작권자별 고지를 대체하지 말고, 배포 채택 시 적용되는 원문과 포함 코드별 고지를 함께 보존해야 한다. 이 README의 출처 목록을 완전한 구성요소 목록으로 간주하지 않는다.

### RAR 구현 출처에서 새로 확인한 근거

중요한 추가 발견은 제한 표기가 참고 자료 밖의 실제 C# 소스에도 있다는 점이다.

- [UnpackV1/Unpack15.cs 1~8행](https://github.com/adamhathcock/sharpcompress/blob/0.50.7/src/SharpCompress/Compressors/Rar/UnpackV1/Unpack15.cs#L1-L8)과 [Unpack20.cs 1~8행](https://github.com/adamhathcock/sharpcompress/blob/0.50.7/src/SharpCompress/Compressors/Rar/UnpackV1/Unpack20.cs#L1-L8)은 innoSysTec, Edmund Wagner의 출처를 적고 junrar 소스와 바이너리에 unrar 라이선스가 적용되며 RAR 압축 알고리즘 재구현에 사용하지 못한다고 명시한다.
- 해당 파일은 C# 프로젝트 디렉터리 아래의 partial Unpack 구현이다. 읽은 프로젝트 설정에는 이 파일의 컴파일 제외가 없으며, [RarReader.cs](https://github.com/adamhathcock/sharpcompress/blob/0.50.7/src/SharpCompress/Readers/Rar/RarReader.cs#L19-L21)는 UnpackV1과 UnpackV2017을 생성한다. 같은 파일 128~140행은 RAR 종류에 따라 두 구현으로 연결한다. 따라서 참고 파일이 우연히 저장소에 있다는 우려보다 구체적인 근거가 있다. 실제 컴파일과 역어셈블 검증은 하지 않았다.
- [UnpackV2017/notes.txt](https://github.com/adamhathcock/sharpcompress/blob/0.50.7/src/SharpCompress/Compressors/Rar/UnpackV2017/notes.txt)는 C에서 C#으로의 자료형 대응을 기록한다. [Unpack.unpack_cpp.cs](https://github.com/adamhathcock/sharpcompress/blob/0.50.7/src/SharpCompress/Compressors/Rar/UnpackV2017/Unpack.unpack_cpp.cs)와 [reference/unrar/unpack.cpp](https://github.com/adamhathcock/sharpcompress/blob/0.50.7/reference/unrar/unpack.cpp)의 생성자와 Init 부분에는 같은 초기화 순서, 분기와 주석이 확인된다. 이식 관계를 뒷받침하는 정황이지만 어느 커밋에서 어떤 허가로 이식했는지까지 입증하지는 않는다.
- [reference/unrar/license.txt](https://github.com/adamhathcock/sharpcompress/blob/0.50.7/reference/unrar/license.txt)는 RAR 압축 알고리즘 재구현 금지와 수정 소스 재배포 고지 조건을 포함한다. 상업적 사용 자체를 금지하는 조건과는 구분해야 한다.

판단: reference/unrar 원본 파일은 확인한 NuGet 패키지 목록에 없지만, 이를 근거로 실제 RAR 구현과 무관한 단순 참고 자료라고 단정할 수 없다. 루트 MIT 선언과 개별 구현의 추가 제한 표기 사이에 해결해야 할 불일치가 있다. 별도 재라이선스 허가, 권리자의 예외 허가 또는 제한 없는 재구현을 증명하는 근거는 이번 확인 범위에서 찾지 못했다. 반대로 이 표기만으로 SharpCompress 전체의 법적 조건이나 모든 파일의 권리를 단정하지 않는다.

### 기능과 채택 권고

[형식 문서](https://github.com/adamhathcock/sharpcompress/blob/0.50.7/docs/FORMATS.md)와 [RAR Reader 테스트](https://github.com/adamhathcock/sharpcompress/blob/0.50.7/tests/SharpCompress.Test/Rar/RarReaderTests.cs)에는 RAR/RAR5 암호화 데이터와 헤더, 분할, 솔리드 사례가 있다. 솔리드는 Reader 경로를 요구한다. 테스트 존재를 확인했으며 실행하지 않았다. 암호화, 분할, 솔리드를 동시에 사용하는 모든 조합의 지원 근거로 확대하지 않는다.

SharpCompress 0.50.7은 기능 검증 후보로 남기되, 사용 목적에 제한이 없는 배포물이라는 요구에 맞는 엔진으로 채택하지 않는 것이 현재 권고다. 제한 표기가 실제 구현에도 발견됐으므로 단순히 reference 디렉터리를 제외하거나 루트 MIT 고지만 넣는 방법으로 해결되었다고 판단하면 안 된다.

대안은 다음처럼 조건을 구분한다.

- RAR를 제외한 7-Zip과 libarchive 조합은 기존 후보를 유지한다. [7-Zip 제외 빌드 설명](https://github.com/ip7z/7zip/blob/main/DOC/readme.txt)과 libarchive 3.8.11의 [RAR](https://github.com/libarchive/libarchive/blob/v3.8.11/libarchive/archive_read_support_format_rar.c), [RAR5](https://github.com/libarchive/libarchive/blob/v3.8.11/libarchive/archive_read_support_format_rar5.c) 검토 기준으로 암호화 RAR 공백이 있어 전체 요구 충족안은 아니다. 이번에 이 엔진들을 실행하거나 재감사하지 않았다.
- 전체 7-Zip 구성은 암호화 RAR의 기술적 비교 대상이지만 unRAR 추가 제한이 있어 용도 제한 없는 배포 요구의 해결책으로 권고하지 않는다. 별도 프로세스나 선택 설치로 옮기는 것만으로 사용자 요구와 라이선스 문제가 해소되었다고 표시하지 않는다.
- 암호화 RAR의 요구를 유지하려면 출처와 권리 관계가 입증된 구현을 확보하거나 다른 구현을 검증해야 한다. 이번 조사로 기능과 라이선스를 모두 충족한다고 확정할 대안은 확보하지 못했다. 외부 저자에게 연락하지 않았다.

다음 판단에 필요한 것은 이식 코드의 권리와 고지 근거, 최종 선택 바이너리와 소스의 대응 검증, 암호화/분할/솔리드 조합별 해시 비교 및 실패 동작, 자원과 취소 검증이다. 암호화 RAR를 제품 미지원으로 확정하지 않는다. 사용자에게 MIT를 다시 선택받을 필요도 없다. 요구를 충족할 근거가 확보되지 않은 상태에서 출시 범위를 줄이는 결정은 별도로 다뤄야 한다.

## unar/XADMaster 추가 검토

### 확인 대상과 배포 상태

2026-11-09에 공식 자료를 한 차례 추가 확인했다. 아래 기능과 개별 파일 고지는 [XADMaster v1.10.8](https://github.com/MacPaw/XADMaster/releases/tag/v1.10.8), 커밋 `881e0ec25e249c9ad5bbc1b6782ae8dcdf48a6ed` 기준이다. 조회한 공식 릴리스 목록에서 가장 최근 공개 릴리스는 2023-10-12의 v1.10.8이며 이 릴리스에는 업로드된 바이너리 자산이 없다. 이것을 공식 사이트의 Windows ZIP과 같은 버전이라고 간주하지 않는다.

유지보수 상태는 별도로 현재 master 커밋 `7cb9ee0abbb163f261e4cb74501e15067032319c`와 워크플로를 읽었다. 이 커밋은 2026-09-21의 저작권 표기 변경 병합이다. 저장소 활동이 있다는 사실과 Windows 제품의 유지보수 및 호환성이 입증됐다는 주장은 구분한다. [현재 소스 기준점](https://github.com/MacPaw/XADMaster/commit/7cb9ee0abbb163f261e4cb74501e15067032319c)

### RAR 기능 근거와 조합의 한계

소스에 처리 경로가 있다는 판단이며 Zipelo나 Windows에서의 실행 통과를 뜻하지 않는다.

- 구형 RAR 데이터 암호: [XADRARParser.m 766~785행](https://github.com/MacPaw/XADMaster/blob/v1.10.8/XADRARParser.m#L766)은 버전별 CryptHandle과 AES 입력을 선택한다. 헤더 암호는 같은 파일 473~484행의 `MHD_PASSWORD` 분기에서 AES 핸들로 블록을 읽는다. 올바른 암호가 필요하다.
- 구형 RAR 분할과 솔리드: 같은 파일 130행 이후의 볼륨 이름 탐색, 753~769행의 조각 입력 결합, 723~741행의 솔리드 스트림 경로가 있다. 압축 버전 15, 20/26, 29/36을 각각 RAR15/20/30 핸들에 연결한다. 아주 오래된 별도 서명에는 미구현 TODO가 남아 있어 모든 역사적 RAR 형식을 보장하지 않는다.
- RAR5 헤더 암호: [XADRAR5Parser.m 241~258행](https://github.com/MacPaw/XADMaster/blob/v1.10.8/XADRAR5Parser.m#L241)은 암호화 헤더에서 암호와 salt로 키를 만들고, 669~673행은 그 키와 IV로 헤더를 복호화한다. 파일 데이터 암호는 430~460행의 추가 필드와 831~848행의 AES 입력 경로로 처리한다. 암호화 버전이 0이 아니면 명시적으로 거부한다.
- RAR5 분할과 솔리드: 같은 파일 101행 이후의 볼륨 탐색, 304~340행의 솔리드 스트림 축적, 819~834행의 RAR50 핸들 및 조각 입력 결합이 있다. v1.10.8 릴리스 설명에도 솔리드 RAR5 안의 디렉터리 처리 수정이 기록되어 있다.
- 조합 위험: RAR5 `volumesForHandle`은 먼저 `isPartOfMultiVolume`을 호출한다. 613~658행의 `readMasterHeaderFromHandle`은 첫 블록이 Main이 아니면 빈 결과를 반환하고, 911~917행의 호출자는 이를 비분할로 판정한다. [테스트 107~110행](https://github.com/MacPaw/XADMaster/blob/v1.10.8/XADMasterTests/XADRAR5ParserTests.m#L107)도 첫 블록이 Encryption이면 비분할로 보는 것을 기대한다. 따라서 헤더 암호화 RAR5의 자동 조각 탐색에 공백이 있을 수 있다는 소스 기반 추론이다. 실제 샘플이나 명시적 다중 입력 경로로 재현하지 않았으므로 모든 입력 방식이 실패한다고 단정하지 않는다.
- 확인한 [구형 RAR 테스트](https://github.com/MacPaw/XADMaster/blob/v1.10.8/XADMasterTests/RAR/XADRARParserTests.m)는 이름 파싱 중심이며 RAR5 테스트는 가변 정수, 분할 판별과 SFX 인식 중심이다. 이 두 파일은 데이터 암호, 헤더 암호, 분할, 솔리드의 전체 조합에 대한 추출 해시 검증 근거가 아니다. 테스트를 실행하지 않았다.

### 실제 빌드 대상의 고지 확인

루트 [LICENSE](https://github.com/MacPaw/XADMaster/blob/v1.10.8/LICENSE)는 LGPL 2.1 원문이다. 루트 표기만 보지 않고 v1.10.8의 `XADRAR*.m`, `XADRAR*.h`, `RAR*.c`, `RAR*.h`에 해당하는 32개 파일의 고지와 제한 관련 문구를 확인했다. 모두 LGPL 2.1 또는 이후 버전의 고지를 포함하며, 이 범위에서 SharpCompress의 Unpack15/20과 같은 unRAR의 RAR 압축 알고리즘 재구현 금지 문구는 발견하지 못했다. [RAR15 구현](https://github.com/MacPaw/XADMaster/blob/v1.10.8/XADRAR15Handle.m#L1), [RAR30 구현](https://github.com/MacPaw/XADMaster/blob/v1.10.8/XADRAR30Handle.m#L1), [RAR50 구현](https://github.com/MacPaw/XADMaster/blob/v1.10.8/XADRAR50Handle.m#L1), [RAR 가상 머신](https://github.com/MacPaw/XADMaster/blob/v1.10.8/RARVirtualMachine.c#L1)

[Makefile.common 105~117행과 181~183행](https://github.com/MacPaw/XADMaster/blob/v1.10.8/Makefile.common#L105)은 이 RAR 구현을 라이브러리 소스 목록에 넣는다. [Makefile.windows](https://github.com/MacPaw/XADMaster/blob/v1.10.8/Makefile.windows#L71)도 이 공통 목록을 포함한다. 따라서 확인한 고지는 참고 자료만의 고지가 아니라 빌드 대상 구현의 고지다. 다만 소스 목록과 실제 배포 바이너리의 동일성은 빌드나 역어셈블로 확인하지 않았다.

라이선스 감사가 완료됐다는 뜻은 아니다. 공통 목록에는 Crypto, PPMd, libxad 등도 있다. 예를 들어 [Crypto/aes.h](https://github.com/MacPaw/XADMaster/blob/v1.10.8/Crypto/aes.h#L1)는 Brian Gladman의 소스 및 바이너리 고지 유지 조건을, [Crypto/sha.c](https://github.com/MacPaw/XADMaster/blob/v1.10.8/Crypto/sha.c#L1)는 Aaron Gifford의 BSD 계열 고지와 보증 부인 및 홍보 시 이름 사용 제한을 포함한다. 이를 unRAR의 용도 제한과 혼동하지 않는다. 모든 포함 코드, UniversalDetector, GNUstep/Objective-C 런타임, zlib/bzip2와 배포 DLL의 버전별 고지를 전수 대조하지 않았다. 코드 이력 전체의 독립 구현 또는 권리 관계도 증명하지 않았다.

채택 시 Zipelo 자체 MIT와 외부 LGPL을 구분해야 한다. LGPL 적용 소스와 수정 내역, 원문 고지를 제공하고 링크 및 배포 방식에 맞는 교체나 재연결 조건을 충족해야 한다. 외부 코드를 MIT로 다시 표시하지 않는다. 고지 보존 의무가 있는 자유 라이선스와 사용 목적 자체를 제한하는 조항은 별개다.

### Windows 유지보수 판단

[공식 명령행 도구 안내](https://theunarchiver.com/command-line)는 Windows용 unar/lsar ZIP 링크를 제공하지만 Windows 버전이 오래되었다고 직접 적고 있다. 이 페이지는 macOS의 Cocotron 교차 컴파일을 설명한다. 반면 현재 저장소의 [Makefile.windows](https://github.com/MacPaw/XADMaster/blob/7cb9ee0abbb163f261e4cb74501e15067032319c/Makefile.windows)는 GCC, 고정 GNUstep 경로와 GNUstep 기반 링크 설정을 사용한다. 페이지의 바이너리 제작 방식과 저장소의 이 Makefile을 같은 배포 경로로 단정할 수 없다.

- 해당 Makefile의 경로별 마지막 커밋은 [2012-10-29](https://github.com/MacPaw/XADMaster/commit/a0f05d6161efce36f06b17badf500dd03a149f8a)이며 libcrypto 의존 제거를 기록한다. 공식 웹페이지의 OpenSSL 설치 설명을 현재 모든 빌드의 필수 의존성으로 옮기면 부정확하다.
- 현재 트리의 워크플로 네 개를 읽었다. [Linux 빌드](https://github.com/MacPaw/XADMaster/blob/7cb9ee0abbb163f261e4cb74501e15067032319c/.github/workflows/main.yml)와 [macOS 테스트](https://github.com/MacPaw/XADMaster/blob/7cb9ee0abbb163f261e4cb74501e15067032319c/.github/workflows/macos-tests.yml), Ubuntu 기반 CodeQL 및 big-endian 검사는 있으나 Windows 실행 작업은 확인하지 못했다. 워크플로 존재만 확인했으며 실행 결과를 검증하지 않았다.
- 공식 Windows ZIP은 다운로드하지 않았다. 버전, x64 여부, 서명, 포함 DLL 고지, 소스 대응과 Windows 11 호환성은 미확인이다. 오래된 바이너리 링크나 Makefile의 존재만으로 유지보수되는 Windows x64 구성이 확보됐다고 판단하지 않는다.

### SharpCompress 제한 범위 보충

앞에서 확인한 UnpackV1/Unpack15.cs와 Unpack20.cs는 구형 RAR 디코더의 partial 클래스다. [RarReader의 선택 경로](https://github.com/adamhathcock/sharpcompress/blob/0.50.7/src/SharpCompress/Readers/Rar/RarReader.cs#L128)는 `IsRarV3` 항목을 UnpackV1로, 나머지를 UnpackV2017로 보낸다. 일반 프로젝트에 두 구현이 포함되므로 RAR5만 사용하는 사용자 흐름으로 바꾸는 것과 제한 표기가 있는 소스를 바이너리에서 제거하는 것은 다르다.

직접 제한을 확인한 두 파일만으로 ZIP/7Z와 RAR5 전체의 법적 조건을 일괄 판정하지 않는다. 동시에 V2017에도 이식 정황이 있고 다른 공용 코드의 의존 관계와 권리 근거는 미확인이다. UnpackV1 또는 reference/unrar만 삭제한 빌드를 자유로운 구성이라고 추천할 근거는 없다. 제거 후 컴파일, 기능과 전체 포함 코드의 고지를 검증한 적도 없다.

### 조사 종료와 필요한 선택

이번 추가 후보 조사는 여기서 마친다. XADMaster는 확인한 RAR 파일의 LGPL 고지와 기능 코드 측면에서 가능성이 있는 후보지만, 유지보수되는 Windows 배포 구성, 전체 의존성 고지와 암호화 조합 동작까지 검증된 채택안은 확보하지 못했다. 암호화 RAR가 자유 라이선스로 원천적으로 불가능하다는 결론도 아니다.

사용자의 결정이 필요한 절충안은 다음 세 가지다. 이번 기록에서 어느 것도 확정하지 않았다.

1. 자유 라이선스와 암호화 RAR 요구를 모두 유지하고, Windows 이식 및 유지보수, 의존성 감사와 조합별 실증에 추가 일정과 개발 범위를 배정한다. XADMaster는 이 경우의 기술 검증 후보이며 성공이나 소요 기간은 보장하지 않는다.
2. 자유 라이선스 구성을 우선 출시하고, 아직 검증되지 않은 암호화 RAR 및 구형 솔리드 RAR의 첫 출시 지원을 보류한다. 기존 요구 축소이므로 명시적 선택이 필요하며 지금 미지원으로 바꾸지 않는다.
3. 암호화 RAR 지원을 우선하여 unRAR 용도 제한이 있는 외부 구성요소의 포함을 허용한다. Zipelo 자체 MIT는 유지하되 배포물 전체가 용도 제한 없는 오픈소스라는 조건을 변경하는 선택이다. 해당 엔진의 실행, 보안과 배포 조건 검증은 별도로 남는다.

실제 수행은 공식 소스, 라이선스, 릴리스와 빌드 설정 열람 및 이 문서 수정뿐이다. 설치, 엔진 실행, Windows 빌드, 샘플 추출, 성능 및 취약점 적용성 검증, 외부 연락과 업로드는 하지 않았다. 기존 src/와 tests/를 수정하지 않았고 브랜치 전환, 커밋과 푸시도 하지 않았다.

## 코어 구현 경계

`src/Zipelo.Core`는 UI와 일치하는 .NET 10 대상이며 외부 패키지 참조가 없다. UI 파일과 빌드 시작점은 수정하지 않았다.

| 구성 | 구현 내용 | 남은 제한 |
| --- | --- | --- |
| ArchiveJob | 단일 실행, 기능 조회, 읽기 전용 입력 요구, 오류 코드, 정리 후 종료 이벤트 | 실제 IPC와 별도 프로세스 실행 없음 |
| EngineContext | 항목 메타데이터, 목록과 출력 상태 구분, 복사와 검증 콜백, 암호 사용 범위와 폐기 | 실제 압축 엔진 없음. 어댑터는 시작한 비동기 작업을 모두 기다려야 함 |
| OutputPathPolicy | 절대 경로와 경로 이탈, ADS, 예약 이름, 끝의 점과 공백 차단 | 문자열 검사만 구현. 실제 링크와 junction 및 경로 교체 경쟁 방어가 아님 |
| JobBudget | 항목 수, 실제 처리 바이트, 출력 바이트, 경과 시간 누적 및 초과 상태 유지 | 메모리와 CPU 강제 제한 없음. 시간과 취소 검사는 협력 방식 |
| IOutputTransaction | 임시 파일 생성, 게시, 정리 계약 | 안전한 Windows 저장 구현 없음. UnavailableOutput은 명시적 실패 |

출력은 임시 상태인 Staged와 게시 완료인 Extracted를 구분한다. CRC 또는 인증의 Passed, Failed, NotChecked, Unavailable은 출력 성공과 별개다. 검증 실패 또는 작업 오류 시 게시하지 않으며, 정리가 끝난 뒤 종료를 보고한다. 저장 계층은 원본과 기존 파일을 보존하고 작업 소유 임시 출력만 정리해야 한다. 코어 인터페이스만으로 파일시스템 안전성이 입증되지는 않는다.

입력은 호출자가 읽기 전용으로 열고 수명을 관리한다. 코어는 쓰기 가능한 입력 스트림을 거부한다. 다른 프로세스가 원본을 수정하는 문제까지 차단하지 않으므로 Windows 호스트의 공유 모드와 원본 해시 검증이 필요하다. 외부 엔진은 신뢰된 어댑터로 취급하며 인터페이스 자체가 샌드박스는 아니다.

목록 작업은 ListEntryAsync, 추출 작업은 WriteEntryAsync에 각 항목을 한 번 전달한다. 해제 결과를 복사한 바이트는 코어가 차감한다. 중간 TAR, 솔리드 선행 해제와 복구 재시도처럼 출력되지 않는 처리량은 엔진이 같은 Budget.ChargeProcessed에 보고해야 한다. 별도 프로세스 감독자가 비협력 엔진의 시간 초과, 메모리 초과와 취소를 강제하는 구현은 후속 작업이다.

암호는 작업 ID와 요청 ID가 있는 콜백으로 요청하고 SecureString 수명을 코어가 관리한다. 콜백이 null을 반환하면 작업을 취소한다. 로그, 명령행, 환경변수와 영구 설정 경로는 만들지 않았다. 엔진이 암호를 복사하거나 메모리에 남기는 문제까지 해결되었다는 의미는 아니다.

## 검증과 연동

- `dotnet build src/Zipelo.Core/Zipelo.Core.csproj -c Release`는 dotnet 명령 부재로 종료 코드 127을 반환했다. 컴파일 성공으로 보고하지 않는다.
- 소스 검토와 프로젝트 XML 및 공백 검사를 수행했다. 7개 작성 파일에서 공백 및 지정 금지 문자 문제를 발견하지 않았다. 실제 C# 실행, 회귀 테스트, 엔진 샘플, Windows 파일시스템 검증은 미실행이다.
- 테스트 코드는 프로젝트 루트 tests/에만 추가한다. 회귀 담당자는 경로 거부, 대소문자 및 상하위 출력 충돌, 예산 경계와 초과 후 재시도, 암호 취소와 횟수 제한, 검증 실패 시 미게시, 정리 실패 결과를 검증할 수 있다.
- UI의 IArchiveWorkspace와 코어 계약은 아직 연결되지 않았다. UI 어댑터는 오류 코드와 항목별 검증 상태를 보존하고, 엔진 및 안전한 출력 저장 계층이 없으면 사용 가능으로 표시하지 않아야 한다.
- 공유 브랜치 docs/initial-project-plan은 보존했다. 구현 변경을 features/ 계열 브랜치로 분리하는 작업은 공유 작업자와 조율 후 수행해야 한다. 커밋이나 외부 게시를 하지 않았다.

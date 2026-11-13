# RAR 압축과 해제 라이선스 연구

조사일: 2026-11-13. 대상은 Zipelo의 RAR 압축과 해제 지원 방향이며 제품 구현이나 라이브러리 채택 승인이 아니다. 이 문서는 공개 원문 검토이며 법률 의견이나 특허 비침해 보증이 아니다.

## 2026-11-13 사용자 결정

RAR 압축과 해제는 v0.2에서도 제외한다. 라이선스 걱정 없이 쓸 방법이 확인되지 않으면 도입하지 않으며, 그 조건이 확인될 때만 다시 검토한다. 추가 후보 출처 감사는 진행하지 않고 RARLAB 문의도 하지 않는다.

아래 내용은 이 결정 전에 수행한 연구 기록이다. 당시의 대안과 권장 순서는 현재 진행할 작업이나 채택 승인이 아니다. rars와 rar 0.4.0의 추가 점검은 착수하지 않았으며, 커밋 이력과 연구 자료의 전체 출처, 버전별 라이선스 변경, cargo metadata를 통한 전이 의존성 전체, 실제 지원 범위와 호환성은 미확인이다. 기존에 확인한 후보의 공개 표기와 한계는 4절에 남긴다.

## 1. 판단 요약 (결정 전 연구)

- 해제 구현 후보는 존재하지만 라이선스 이름만으로 선택하면 안 된다. libarchive의 두 RAR 읽기 파일은 BSD-2-Clause 조건을 표시한다. 다만 RAR5 작성자는 libunrar 구조를 참고했다고 설명하므로, 설명서만 사용한 독립 구현이라고 단정할 수 없다. [RAR4 파일](https://github.com/libarchive/libarchive/blob/master/libarchive/archive_read_support_format_rar.c), [RAR5 파일](https://github.com/libarchive/libarchive/blob/master/libarchive/archive_read_support_format_rar5.c), [작성자 설명](https://github.com/libarchive/libarchive/pull/1061#issuecomment-419696568).
- 공식 UnRAR는 무료 사용을 허용하지만 압축기 개발 및 압축 알고리즘 재현에 제한을 둔다. Zipelo의 자유 라이선스 요구와 공식 unRAR 코드 사용 금지 방침에 따라 채택 후보에서 제외한다. Rust로 감싸거나 이식해도 원래 조건이 없어지는 것은 아니다. [동봉 라이선스](https://github.com/muja/unrar.rs/blob/master/unrar_sys/vendor/unrar/license.txt).
- 자유 라이선스 RAR 생성 구현이 없다고 단정할 근거는 부족하다. 조사 중 Apache-2.0으로 압축과 해제를 제공한다고 밝힌 rars를 확인했다. 이는 배포자 주장 확인이며 출처 적법성, 전체 의존성 및 성능 검증 완료를 뜻하지 않는다. [rars README](https://github.com/bitplane/rars), [COPYING](https://github.com/bitplane/rars/blob/master/COPYING).
- 권장안은 RAR 생성을 계속 보류하고, 해제용 순수 Rust 후보의 출처 감사를 먼저 하는 것이다. 후보가 부적합하면 libarchive 읽기 코드의 제한된 Rust 이식을 검토하되 RAR5 출처 쟁점을 먼저 해소한다. 아래 작업 크기와 위험 평가는 기획 판단이며 일정 약속이 아니다.

## 2. 공식 UnRAR 조건과 RARLAB 권리 주장

### UnRAR 소스 라이선스

[SPDX 원문](https://spdx.org/licenses/UnRAR.json)과 [unrar_sys에 동봉된 원문](https://github.com/muja/unrar.rs/blob/master/unrar_sys/vendor/unrar/license.txt)을 대조했다. 무료로 RAR을 처리하는 소프트웨어에 이용할 수 있지만, RAR 호환 압축기를 개발하거나 독점 압축 알고리즘을 재현하는 데 소스를 사용할 수 없다는 제한이 있다. 수정 소스를 배포할 때는 해당 조건 문단을 라이선스 또는 문서와 소스 주석에 포함하도록 요구한다.

따라서 무료 해제 허용과 용도 제한 없는 자유 라이선스는 구별해야 한다. 이 제한을 모든 독립 RAR 구현의 일괄 금지로 확대하지 않지만, Zipelo에는 이 소스나 파생 구현을 들이지 않는 것이 현재 방침에 맞는다.

### 압축 프로그램 사용권과 구현 권한의 차이

[RAR/WinRAR EULA](https://www.rarlab.com/license.htm)는 압축 알고리즘을 proprietary로 표현하며 RAR/WinRAR 바이너리 또는 UnRAR 소스/바이너리를 이용하거나 역공학해 재현하는 데 서면 허락을 요구한다. 일반 사용권은 프로그램 실행과 생성한 압축파일 배포에 관한 것이며, 압축 엔진의 소스 공개나 MIT 재배포 허락으로 읽어서는 안 된다.

공식 [판매 페이지](https://www.rarlab.com/shoprarlab.php)는 프로그램 사용권 구입 경로다. 임베딩, 재배포, 소스 제공 또는 자유 라이선스로의 재허락 조건은 미확인이다. 별도 협의를 원하면 [RARLAB 연락 페이지](https://www.rarlab.com/feedback.htm)의 판매/지원 경로를 이용할 수 있다. 실제 연락이나 구매는 하지 않았다. 문의를 승인받는 경우 원하는 플랫폼, 배포 방식, 소스 공개 범위, 재배포 권한과 비용을 서면으로 확인해야 한다.

### 특허와 공개 설명서

확인한 EULA에는 독점 주장과 코드 이용 제한이 있지만, 그것만으로 특정 특허가 존재하거나 존재하지 않는다고 결론 내릴 수 없다. RARLAB 대상 웹 검색과 위 공식 문서 검토에서 적용 특허번호, 국가별 유효 권리, 소멸 여부와 특허 실시 허락 범위는 미확인이다. 특허 데이터베이스의 국가별 청구항 조사와 법적 평가는 수행하지 않았다.

[RAR5 공식 설명서](https://www.rarlab.com/technote.htm)는 기본 데이터 구조를 설명하며 알고리즘 세부사항은 UnRAR 소스를 보도록 안내한다. 공개 열람 가능하다는 사실만으로 압축 알고리즘 구현에 필요한 명세가 완전하거나 모든 권리가 허락되었다고 판단하지 않는다. Zipelo 연구에서는 이 안내를 따라 공식 구현 소스를 제품에 복사하지 않는다.

## 3. libarchive RAR 읽기 코드의 출처

### RAR4 계열 읽기

[archive_read_support_format_rar.c](https://github.com/libarchive/libarchive/blob/master/libarchive/archive_read_support_format_rar.c) 머리말에는 Tim Kientzle(2003-2007), Andres Mejia(2011)의 저작권과 BSD 2개 조건이 있다. 소스 배포에는 저작권, 조건과 면책 문구를 보존하고 바이너리 배포에는 문서 등으로 재현해야 한다.

초기 [이슈 151의 유지관리자 설명](https://github.com/libarchive/libarchive/issues/151#issuecomment-91876434)은 UnRAR 코드를 사용할 수 없으며 헤더 설명서와 해제 알고리즘을 구분한다. 같은 이슈의 2011-07-15 댓글 37에서 구현자는 The Unarchiver를 참고했고 BSD 재허락을 받았다고 설명한다. 따라서 전체 해제가 technote만으로 구현됐다는 설명은 부정확하다. [개발 이력](https://github.com/libarchive/libarchive/issues/151).

현재 파일의 BSD 표기는 확인했지만, 이 문서가 모든 기여와 원저작자의 허락 증빙을 독립 감사한 것은 아니다. 실제 이식 시 가져오는 함수와 보조 파일의 출처를 특정해야 한다.

### RAR5 읽기

[archive_read_support_format_rar5.c](https://github.com/libarchive/libarchive/blob/master/libarchive/archive_read_support_format_rar5.c)에는 Grzegorz Antoniak(2018)의 저작권과 BSD 2개 조건이 있다. [도입 PR1061](https://github.com/libarchive/libarchive/pull/1061)은 기존 RAR4 처리와 별도로 작성한 스트리밍 해제기라고 설명한다.

그러나 [작성자 댓글](https://github.com/libarchive/libarchive/pull/1061#issuecomment-419696568)은 해제 블록을 libunrar 구조에 따라 작성했으며 일부는 RARLAB 문의와 생성 파일 분석으로 조사했다고 밝힌다. 이는 코드 복제를 입증하지도, 법적 문제 없음을 입증하지도 않는다. 특히 RAR 압축기를 만드는 데 이 코드를 재사용할 수 있는지는 미확인이다. BSD 표기만으로 공식 UnRAR 관련 출처 쟁점이 해소됐다고 결론 내리지 않는다.

[공식 릴리스 기록](https://github.com/libarchive/libarchive/wiki/ReleaseNotes)은 3.4.0의 RAR5 읽기 지원을 확인한다. 이를 RAR 생성 또는 모든 암호/분할 변형의 지원으로 확대하지 않는다.

### 전체 라이브러리와 보조 파일

[libarchive COPYING](https://github.com/libarchive/libarchive/blob/master/COPYING)은 각 파일의 조건이 우선한다고 명시하고 BLAKE2 파일의 복수 라이선스, 일부 BSD 3개 조건 및 빌드 파일의 다른 조건도 구분한다. Rust 이식은 원저작권을 없애는 작업이 아니므로 이식한 부분의 BSD 고지를 보존해야 한다. 전체 프로젝트 MIT 표기만으로 대체하지 않는다. 실제 채택 버전의 보조 파일과 활성화된 외부 라이브러리까지 확인하는 절차가 필요하다.

## 4. Rust 후보의 라이선스와 한계

아래는 조사 시 공개 문서 기준이다. master와 latest 링크는 변경될 수 있으므로 채택 전 실제 버전 및 소스 묶음 기준으로 재검토해야 한다. 실행, 압축률 측정, 취약점 감사는 하지 않았다.

- **unrar / unrar_sys:** 래퍼 Cargo.toml은 MIT OR Apache-2.0이나 unrar_sys는 FFI이며 cc 빌드 의존성과 별도 UnRAR 소스를 포함한다. 공식 RARLAB도 Rust wrapper로 소개한다. 순수 Rust 및 공식 UnRAR 미사용 조건을 충족하지 않으므로 제외한다. [래퍼 메타데이터](https://github.com/muja/unrar.rs/blob/master/Cargo.toml), [FFI 메타데이터](https://github.com/muja/unrar.rs/blob/master/unrar_sys/Cargo.toml), [실제 UnRAR 조건](https://github.com/muja/unrar.rs/blob/master/unrar_sys/vendor/unrar/license.txt), [RARLAB 소개](https://www.rarlab.com/rar_add.htm).
- **unrar-rs:** 게시 문서가 RARLAB reference 구현의 Rust 이식임을 밝히고 UnRAR 제한을 표시한다. Rust 언어라는 이유만으로 허용 후보로 삼지 않는다. [crate 문서](https://docs.rs/unrar-rs/latest/unrar_rs/).
- **compress-tools:** MIT OR Apache-2.0 Rust API이지만 libarchive에 의존한다. C 라이브러리 연결 방식이므로 현재 순수 Rust 정책의 예외 승인이 필요하다. [Cargo.toml](https://github.com/OSSystems/compress-tools-rs/blob/master/Cargo.toml), [설치 의존성](https://github.com/OSSystems/compress-tools-rs#dependencies).
- **rar 0.4.0:** 게시 README는 MIT와 Rust native 목록/해제를 표방하면서 기능이 제한됨을 밝힌다. 목록에 나열한 형식을 모두 검증된 지원으로 해석하지 않는다. 코드 출처와 실제 해제 범위는 미확인으로 남긴다. [게시 문서](https://docs.rs/crate/rar/latest).
- **rars:** 현재 저장소의 Cargo.toml과 COPYING은 Apache-2.0이고 README는 압축, 해제 및 복구를 제공한다고 설명한다. 조사된 crate 메타데이터에는 write, parallel, recovery, encryption 기능과 암호화/병렬 처리 의존성이 있다. 작성자는 더 많은 시험이 필요하고 WinRAR 대비 메모리와 속도에 한계가 있다고 밝힌다. 코드와 연결된 연구 자료의 전체 출처, 과거 버전별 라이선스, 전이 의존성 및 호환성을 감사하지 않았으므로 채택 승인은 보류한다. [README](https://github.com/bitplane/rars), [작업 공간 메타데이터](https://github.com/bitplane/rars/blob/master/Cargo.toml), [crate 메타데이터](https://github.com/bitplane/rars/blob/master/crates/rars/Cargo.toml), [라이선스](https://github.com/bitplane/rars/blob/master/COPYING), [연구 자료](https://github.com/bitplane/rar-research).

## 5. 네 가지 방안 비교

다음 크기는 작업량의 상대 평가다. 법적 안전이나 구현 완료를 보증하는 등급이 아니다.

### 방안 1: libarchive 읽기 코드를 Rust로 이식

- 라이선스 위험: 파일별 BSD 조건을 보존할 수 있지만 RAR5의 libunrar 참조 이력과 전이 코드 출처 확인이 선행되어야 한다. 압축기 재사용은 별도 판단 대상이다. 근거는 3절의 파일 원문과 작성자 설명이다.
- 작업 크기: 대~특대. 파서뿐 아니라 버퍼, 필터, 무결성 검사와 오류 처리를 이식하고 upstream 수정도 계속 추적해야 한다.
- 외부 의존성: C 런타임 연결은 줄일 수 있으나 체크섬과 암호 등의 보조 구현이 필요하다. 선택한 순수 Rust 의존성과 고지를 별도로 검토한다.
- 판단: 출처 문제가 해소되고 유지보수 부담을 수용할 때의 해제 대안이다. 읽기 코드 이식으로 RAR 생성까지 해결되지 않는다.

### 방안 2: libarchive를 연결해 사용

- 라이선스 위험: 파일별 조건과 실제 링크하는 라이브러리 고지가 필요하며 RAR5 출처 불확실성은 연결해도 남는다. 근거는 3절과 compress-tools 문서다.
- 작업 크기: 중~대. 이식보다 통합량은 줄지만 Windows/Ubuntu 배포, C 빌드와 보안 업데이트 관리가 추가된다.
- 외부 의존성: libarchive 및 빌드 옵션별 C 의존성. 현재 순수 Rust 정책 밖이므로 이준영 님의 별도 방향 결정이 필요하다.
- 판단: 빠른 해제 통합 대안이지만 현재 정책의 기본 권장안은 아니다.

### 방안 3: 형식 자료만 보고 신규 구현

- 라이선스 위험: 코드 차용을 줄일 수 있으나 명세의 이용 조건과 참고 자료 출처를 관리해야 한다. 설명서 공개가 특허 또는 제3자 권리 면책은 아니다. 공식 설명서의 알고리즘 설명 부족은 2절에서 확인했다.
- 작업 크기: 특대. 해제와 생성 각각 별도 명세, 표본 및 상호운용 시험이 필요하다.
- 외부 의존성: 순수 Rust로 구성할 수 있는 설계 방향이나 실제 보조 라이브러리 목록은 미정이다.
- 판단: 출처가 명확한 자료만으로 가능한 범위를 먼저 정해야 한다. rars 같은 후보를 평가할 때도 그 후보의 자료 출처를 검증하는 대안과 비교한다. 구현 가능한 범위는 미확인이다.

### 방안 4: RAR 생성 미지원 또는 RARLAB 정식 라이선스 협의

- 라이선스 위험: 생성 미지원은 관련 엔진 도입 위험을 줄인다. 일반 WinRAR 사용권 구매는 자유 라이선스 엔진 재배포 권한을 주는 것으로 확인되지 않았다. 근거는 2절 EULA와 판매 경로다.
- 작업 크기: 미지원 안내는 소. 별도 계약은 기간과 비용 미확인, 승인된 엔진 통합은 별도 산정이다.
- 외부 의존성: 미지원 유지에는 없음. 계약 경로는 제공되는 엔진/실행 파일과 조건에 따라 달라져 미정이다.
- 판단: 당장은 생성 보류가 현실적이다. 비용이나 계약을 약정하지 않으며 자유로운 수정 및 배포가 가능한지 서면 조건부터 확인해야 한다.

## 6. 당시 권장 순서와 미확인 증거

1. RAR 생성을 보류한 상태로 해제 후보의 소스 출처와 실제 버전별 라이선스를 우선 감사한다. 순수 Rust 후보가 적합하면 대규모 이식보다 먼저 평가하는 것을 권한다. 4절의 후보 소개는 조사 대상 제안이며 구현 지시가 아니다.
2. libarchive RAR5를 설명서만으로 만든 구현이라고 안내하지 않는다. 해당 코드와 이를 참고한 후보의 출처를 검토하고, 압축기 구현에 사용할 수 있는지 별도 판단한다.
3. 구현을 승인받는 경우에만 backend가 RAR 버전, 압축 방식, 암호, solid, 분할 범위를 정하고 정상/손상/경로 이탈/자원 초과 표본으로 검증한다. 라이선스 고지를 배포 산출물에 넣고 미지원 변형을 명확히 표시한다.
4. RAR 생성은 해제와 별도 결정한다. 후보의 자유 라이선스 표기 외에 출처, 권리와 상호운용 근거를 확보하거나 별도 계약 조건을 확인한 뒤 이준영 님이 방향을 정하도록 한다.

이번 조사에서 제품 코드, 외부 라이브러리와 라이선스 설정은 변경하지 않았다. 외부 관계자에게 문의하지 않았고 구매나 약정도 하지 않았다. 미확인 사항은 RAR5 참고 이력의 법적 영향, 후보 전체 출처와 전이 의존성, 국가별 특허 상태, 별도 재배포 계약의 가능 여부와 비용이다.

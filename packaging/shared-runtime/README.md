# 공유 런타임 패키징 검증 후보

기준 main: `9690067384af35b9084fc29e7a4d77a98e3b7bef`. 기본 self-contained 프로필은 유지함. 이번 후보는 배포 구조 실험이며 릴리스, 지원 완료, 절감률이나 실행 성능의 근거가 아님. installer PR #5 및 `packaging/prepare.py`와 연결하지 않음.

## 실행 계약

.NET 10 SDK의 `AppHostDotNetSearch=AppRelative`만 사용함. UI apphost는 실행파일 기준 `runtime/`, Worker apphost는 `../runtime/`를 검색함. EnvironmentVariable, Global, AppLocal 검색을 추가하지 않음. CWD, PATH, 사용자 DOTNET_ROOT에서 런타임이나 Worker를 찾지 않음. 기존 WorkerPlatform의 `worker/Zipelo.Worker[.exe]` 절대 경로 및 별도 프로세스 실행은 그대로임.

각 앱을 RID가 있는 framework-dependent 형식으로 게시함. `RuntimeFrameworkVersion=10.0.14`, `RollForward=Disable`로 두 프레임워크의 버전을 고정함. UI와 Worker 모두 동일 SHA-512로 검증한 공식 apphost pack의 apphost를 SDK `AppHostSourcePath`로 전달함. 게시된 apphost의 x64 PE/ELF, runtimeconfig의 정확한 버전/프레임워크/rollForward 및 deps의 RID를 확인함. 런타임 아카이브의 공식 해시는 버전과 RID를 함께 고정함.

이 구조는 런타임 검색 위치를 고정하는 계약임. 사용자 계정이나 배포 폴더가 장악된 경우의 무결성 보호 또는 OS 자원 격리를 새로 제공하지 않음. 다른 런타임 환경변수 전체를 차단한다고 주장하지 않음.

## 오프라인 입력과 필수 파일

`inputs.json`은 Microsoft 공식 release metadata의 10.0.14 파일명, URL, SHA-512를 고정함. 조립기는 네트워크 기능이나 자동 복원 기능이 없음. 담당 검증 환경에서 다음 공식 아카이브를 준비해야 함.

- Windows: `dotnet-runtime-10.0.14-win-x64.zip`, `windowsdesktop-runtime-10.0.14-win-x64.zip`, `dotnet-apphost-pack-10.0.14-win-x64.zip`.
- Ubuntu: `dotnet-runtime-10.0.14-linux-x64.tar.gz`, `dotnet-apphost-pack-10.0.14-linux-x64.tar.gz`.
- apphost pack ID는 `Microsoft.NETCore.App.Host.win-x64` 또는 `Microsoft.NETCore.App.Host.linux-x64`, 버전은 10.0.14임. SDK 복원에는 기존 캐시의 프로젝트 의존성과 SDK에 맞는 `Microsoft.NETCore.App.Ref`, Windows의 `Microsoft.WindowsDesktop.App.Ref`도 필요함. 캐시 부족은 오류로 남기며 새 다운로드로 우회하지 않음. 런타임 NuGet 팩의 일부 DLL만으로 설치 트리를 재구성하지 않음.

런타임 공식 아카이브의 전체 파일을 허용한 설치 트리 그대로 보존함. 필수 경로는 다음과 같음.

- `runtime/host/fxr/10.0.14/hostfxr.dll` 또는 `libhostfxr.so`.
- `runtime/shared/Microsoft.NETCore.App/10.0.14/` 전체. CoreLib, coreclr, clrjit, hostpolicy, 프레임워크 deps/runtimeconfig와 모든 동봉 파일 포함함.
- Windows의 `runtime/shared/Microsoft.WindowsDesktop.App/10.0.14/` 전체. PresentationFramework와 프레임워크 deps/runtimeconfig, 모든 문화권 리소스 포함함.
- 공식 아카이브의 `dotnet[.exe]`, LICENSE 및 ThirdPartyNotices 파일을 보존함. apphost pack은 빌드 입력으로만 사용하며 SDK나 packs 폴더를 최종 후보에 넣지 않음.
- 제품 루트의 검토된 `LICENSE`와 `THIRD_PARTY_NOTICES.txt`가 필요함. 이 실험은 Avalonia/Skia 등의 고지를 자동 생성하거나 라이선스 검토를 대신하지 않음. 공식 파일 간 충돌이 발생하면 덮어쓰지 않고 실패함.

## 조립 절차

별도 체크아웃에서 기존 SDK와 명시적으로 준비된 공개 패키지 캐시만 사용해 같은 RID/Release/framework-dependent 속성으로 먼저 오프라인 복원해야 함. `assemble.py`는 `dotnet publish --no-restore`만 호출함. SDK/런타임/도구를 설치하지 않음. SDK dotnet 실행파일의 검증된 절대 경로를 전달함. 공백은 셸의 따옴표로 처리하되 MSBuild 해석에 영향을 주는 세미콜론 등의 문자는 경로에서 사용하지 않음.

```sh
python3 packaging/shared-runtime/assemble.py --validation --rid linux-x64 --archives /approved/runtime-archives --dotnet /approved/sdk/dotnet --output /approved/output/zipelo-shared-runtime
```

Windows는 `--rid win-x64`, 검증된 SDK의 `dotnet.exe`와 Windows 경로를 사용함. 실제 해제 GUI 검증을 할 때만 `--extraction-validation`을 추가함. 기본 후보의 생성/해제 개발 검증 플래그는 false임. 일반 capability는 변경하지 않음.

UI의 별도 `SharedRuntimeValidation.pubxml`은 조립기 전용임. 실험 플래그가 켜진 경우에만 기존 UI targets의 Worker 자동 배치를 건너뛰며, 조립기가 UI와 Worker를 각각 새 SDK 출력 폴더에 게시함. Worker 게시가 실패하거나 필수 파일이 없으면 후보를 만들지 않음. 예전 bin 산출물 검색이나 수동 보정은 없음.

출력은 존재하지 않는 새 폴더만 허용함. 기존 폴더와 병합하거나 업데이트하지 않음. 기존 출력 부모와 아카이브/SDK 입력, 프로젝트 루트를 strict resolve로 해석한 뒤 실제 경로의 포함 관계를 검사함. 프로젝트 전체와 아카이브/SDK 디렉터리의 내부 또는 상위 출력은 거부함. 마지막 이름의 기존 파일, 폴더와 dangling symlink도 거부함. 외부의 정상 symlink 부모는 실제 경로로 정규화하며 소스/입력을 가리키면 차단함. 출력 예약 직전 다시 확인함. 조립 중 외부 프로세스가 부모 디렉터리를 교체하는 경합을 원자적으로 봉쇄하는 구현은 아니므로 검증 환경의 부모 경로가 안정적이어야 함. 임시 조립 후 필수 검증과 고지 복사가 끝나면 새 출력 폴더를 예약하고 후보를 이동함. 이동 실패는 이번에 예약한 출력만 정리함. 프로세스 강제 종료/전원 손실 때는 부분 출력이 남을 수 있으므로 후보 manifest와 전체 해시를 대조하고 불완전한 후보는 실행하지 않아야 함.

`shared-runtime-candidate.json`은 RID/버전/입력 해시/apphost 원본 해시/전체 파일 크기와 SHA-256을 기록함. 자체 manifest 파일은 순환 해시를 피하기 위해 목록에서 제외함. `releaseReady=false`, `actualExecutionValidated=false`는 실행 검증 전 상태이며 조립 성공을 기능 검증으로 표현하지 않음.

## 경로 회귀 검사

`python3 -B tests/shared-runtime/test_paths.py -v`로 19개 사례를 실행함. 테스트 전용 임시 디렉터리는 프로젝트 루트 tests/shared-runtime/ 아래에만 생성함. Ubuntu 작성 환경에서 19개 통과했으며 실제 소스/SDK/아카이브를 대상으로 출력하거나 SDK를 실행하지 않았음. Windows 경로와 junction 실환경 검사는 별도로 남음.

검사는 `..` 우회, symlink 부모와 symlink 뒤 `..`, 소스/프로젝트 내부, 아카이브/SDK 내부, 기존 출력/상위 경로, dangling leaf, 누락 부모, 정상 외부 경로와 상대 경로를 포함함. 기존 소스 및 입력 표식 바이트와 기존 출력 표식 보존을 확인함.

## 검증 상태와 수용 기준

작성 환경에서는 새 SDK 실행, 복원, 공식 바이너리 다운로드, 실제 조립 및 VM 실행을 하지 않음. XML/Python 구문, 기존 targets와의 최소 차이 및 공식 해시 연결만 정적으로 확인함. 아카이브 내부 경로와 실제 SDK가 생성한 apphost 옵션도 두 OS 실조립으로 확인해야 함. 공식 아카이브가 선언한 설치 트리와 다르면 오류로 중단하며 파일을 임의로 골라 통과시키지 않음.

검증 담당은 두 OS에서 전역 .NET/SDK가 없는 조건과 무관한 CWD, 거짓 DOTNET_ROOT/PATH를 사용해 앱과 Worker가 동봉 런타임만 선택하는지 확인해야 함. 런타임 폴더, hostfxr, coreclr, Desktop 프레임워크 각각의 누락/잘못된 버전/RID에서는 전역 설치가 있어도 실패해야 함. 정상 ZIP 목록/개발 검증 해제, 원본/충돌 보존, 진행 중 취소, 중복 요청과 자식 종료를 확인함.

현재 후보는 업데이트를 지원하지 않음. 실패 후보가 기존 검증 출력에 영향을 주지 않는지 확인하고, 향후 업데이트는 버전별 전체 후보 검증 후 교체 및 실패 복구를 별도 설계해야 함. 런타임 한 벌의 보안 업데이트가 UI와 Worker 모두에 영향을 주므로 두 프로세스를 함께 회귀 검증해야 함. 크기는 전체 파일 논리 크기/설치 디스크 점유/다운로드 압축 크기를 구분해 기존 self-contained 결과와 실제 비교함.

## 공식 근거

- https://learn.microsoft.com/en-us/dotnet/core/deploying/ 의 AppHostDotNetSearch/AppHostRelativeDotNet 계약.
- https://github.com/dotnet/designs/blob/main/proposed/apphost-embed-install-location.md 의 AppRelative 단독 검색과 실패 동작.
- https://github.com/dotnet/sdk/blob/main/src/Tasks/Microsoft.NET.Build.Tasks/ResolveAppHosts.cs 의 RuntimeFrameworkVersion/apphost pack 관계.
- https://builds.dotnet.microsoft.com/dotnet/release-metadata/10.0/releases.json 의 10.0.14 아카이브 및 SHA-512.

## Python 없는 Windows SDK와의 분리 실행

기존 단일 환경용 assemble.py도 유지함. 새 handoff.py의 prepare/finalize는 Python만 사용하며 SDK를 실행하지 않음. RID별로 별도의 계획을 생성함. 준비 단계에는 공식 아카이브와 검토된 제품 제3자 고지가 필요함. legal 담당의 검토가 끝나기 전 임의 요약을 완성 고지로 넣지 않음.

```sh
python3 packaging/shared-runtime/handoff.py --validation prepare --rid linux-x64 --archives /approved/archives --notices /approved/THIRD_PARTY_NOTICES.txt --output /approved/plan-linux
```

prepare는 기존 경로 보호를 적용하고 공식 SHA-512를 확인한 뒤 runtime/, apphost/, source/와 게시 도구를 새 번들에 준비함. 출력된 planSha256을 별도로 기록함. plan.json에는 무작위 planId, RID/10.0.14/Release/개발 검증 옵션, 공식 입력 및 모든 소스/런타임/apphost/도구의 크기와 SHA-256이 들어감. 복사 대상은 명시적 src 파일과 제품 LICENSE 및 검토된 고지임. SDK/개인 캐시/인증 자료는 포함하지 않음. 원본 소스에 bin/obj가 있어도 전달하지 않음.

Windows의 기존 PowerShell에서 준비 번들의 publish.ps1을 검토한 뒤 실행함. 실행 정책을 우회하거나 변경하지 않음. 아래 값은 검증 담당이 실제 승인 경로와 준비 시 기록한 해시로 대체해야 함.

```powershell
.\publish.ps1 -Validation -Bundle 'C:\approved\plan-linux' -PlanSha256 '<prepare에서 기록한 SHA-256>' -Dotnet 'C:\approved\sdk\dotnet.exe' -WorkParent 'C:\approved\builds' -PackageSource 'C:\approved\public-package-feed'
```

게시기는 준비된 전체 파일을 해시 대조하고 매번 새 작업 디렉터리에 source/를 명시적 목록으로 복사함. SDK 10 버전을 확인하고, 명시적 로컬 패키지 소스만 들어 있는 임시 NuGet.Config로 오프라인 복원함. 캐시/타기팅 팩/의존성 부족 시 실패하며 인터넷이나 새 SDK 설치로 우회하지 않음. publish는 --no-restore임. 경로의 reparse point와 입력 내부 작업 부모를 거부함. 새 작업 폴더가 입력의 상위에 생성되더라도 실제 새 하위 폴더만 사용하며 기존 입력을 복사하거나 삭제하지 않음.

Plan.targets를 SDK에 전달해 앱/Worker별 상대 런타임 경로와 AssemblyMetadata의 ZipeloPublishPlan 값을 준비 계획 SHA-256으로 고정함. Core/Runtime/Worker 제품 소스는 변경하지 않음. 게시 성공 및 소스 재검사 후에만 publish-receipt.json을 기록하며 계획 ID/해시/RID/버전/SDK 버전과 UI/Worker 전체 출력 해시를 연결함. 실패한 작업 폴더에는 완료 영수증이 없고 최종화할 수 없음. 기존 작업이나 결과를 재사용하지 않음.

Linux에서는 원래 준비 번들과 새 results/를 사용해 최종화함. 계획 해시는 준비 때 별도로 기록한 값이며 결과 폴더에서 자동 신뢰하지 않음.

```sh
python3 packaging/shared-runtime/handoff.py --validation finalize --bundle /approved/plan-linux --results /approved/returned-results --plan-sha256 '<prepare에서 기록한 SHA-256>' --output /approved/candidate-linux
```

최종화는 준비 파일 및 현재 소스/도구와 계획의 일치, 전체 결과와 영수증 해시, 실제 apphost의 x64 및 내장 DLL/검색 위치를 확인함. 공식 template의 placeholder 위치를 기준으로 AppRelative 단독 비트와 경로를 확인하며 Linux는 두 구성 버퍼 외 전체 바이트도 비교함. Windows는 SDK 리소스 갱신을 허용하되 공식 template의 .text 코드를 비교함. .NET HostWriter v10 계약이 달라지면 검사 우회 없이 실패해야 함.

각 제품 DLL에 해당 계획 해시와 ZipeloPublishPlan 메타데이터 문자열이 있어야 하므로 기존 계획의 낡은 결과를 영수증만 새로 써서 통과시킬 수 없음. 이 검사는 신뢰하는 SDK 게시기의 작업 연결 및 변조 탐지이며 악성 게시기에 대한 암호학적 빌드 증명은 아님. 게시기와 전달 매체는 검증 담당이 신뢰할 수 있어야 함. 최종화는 runtimeconfig/deps/RID/필수 파일 및 기존 고지 요구도 다시 확인함. Windows 교차 게시의 Linux 실행 권한은 입력을 바꾸지 않고 새 후보에서 계획에 기록한 공식 apphost 모드 0755로 배치함.

최종 후보는 준비/게시/계획 해시와 전체 파일 해시를 기록하며 actualExecutionValidated=false, releaseReady=false임. 기본 self-contained 배포와 installer PR #5는 전환하지 않음. 작성 시점에는 두 OS의 실제 준비/PowerShell/오프라인 복원/게시/최종화와 기능/크기가 미검증이었음. 이후 중앙 검증 결과와 남은 제한은 아래 고정 후보 기록을 적용함. 새 설치, 다운로드, VM 실행은 작성 범위에 포함하지 않음.

검사 명령은 `python3 -B -m unittest discover -s tests/shared-runtime -p 'test_*.py' -v`임. 기존 경로 19개와 분리 게시 대역 검사를 구분함. 대역 ELF/어셈블리는 tests/ 임시 루트의 작은 비실행 파일이며 실제 SDK 게시나 실제 바이너리 통과의 근거가 아님. PowerShell 구문/실행은 기존 Windows 환경의 별도 검증이 필요함.

HostWriter 근거: https://github.com/dotnet/runtime/blob/v10.0.0/src/installer/managed/Microsoft.NET.HostModel/AppHost/HostWriter.cs . Windows PE 리소스는 SDK가 변경할 수 있으므로 실행파일 전체를 원본 template의 해시와 같다고 요구하지 않음.

## 중앙 검증의 고정 후보 기록

2026-11-09 중앙 검증 담당의 보고 기준은 main `626483e8935aaa03d5a34feebc61be2f9180b80d` + PR #9 `de0d7ccd53f156bb857e4f4cc58d436b557330b9` + 고지 PR #11 `8e80057938f9bbc0f67d8cb5236043a8a021a350`임. 각 RID에서 Python prepare, 기존 Windows SDK의 오프라인 publish, Linux finalize가 성공한 것으로 보고됨. 아래 파일 수와 바이트 합은 담당자가 실제 실행한 결과와 구분하여 이번 문서 담당도 후보 manifest를 읽기 전용으로 대조함. 원본 후보의 파일 또는 manifest는 변경하지 않음.

- Linux 후보: `/home/user/zipelo-validation/shared-runtime-candidate-linux-8e800579`. 자체 manifest 제외 239파일, 103,348,032바이트. planId `20c758d58fae491eb03868fe82e004ee`, plan SHA-256 `4d467bfed03682a6f79a72ab71cabc8407ec02afcf8298f90581ea5f00662231`. 후보 manifest SHA-256 `42ae42bf60ad6c37163db0bc22afd61e30e4afb3f276f6338535641f90609d39`.
- Windows 후보: `/home/user/zipelo-validation/shared-runtime-candidate-win-8e800579`. 자체 manifest 제외 500파일, 181,041,888바이트. planId `44e7096ae03f454b82989a334b19e671`, plan SHA-256 `7a91b8e4fa797fc7e6d99fdf429bdcaa8aeccba1a1b672dd88ca764dad6ac124`. 후보 manifest SHA-256 `be8c23fd42080b2e3deafecc698220f4d6ce078142e232b260e30501dcfeb6cc`.

이 값은 파일의 논리적 길이 합계이며 다운로드 크기, 파일시스템의 실제 디스크 점유, 실행 속도나 메모리 수치가 아님. 기존 후보와의 기능/법적 조건이 같다고 가정해 절감률로 일반화하지 않음.

중앙 보고에서 거짓 DOTNET_ROOT/PATH에서도 동봉 runtime의 hostfxr/coreclr/hostpolicy를 사용함. Windows ZIP 목록/해제 결과 해시와 Ubuntu Worker 목록/메모리 해제가 맞았음. 별도 사본에서 runtime 전체를 제거한 검사는 유효한 DOTNET_ROOT를 제공해도 양 OS의 UI/Worker가 실패한 것으로 보고됨. 이 기록은 문서 담당의 재실행 결과가 아님.

Ubuntu GUI는 기존 0x0 화면으로 미검증임. Windows 검증 VM에는 SDK가 설치되어 있어 SDK가 전혀 없는 Windows 환경의 실제 실행 증거로 확대하지 않음. 개별 coreclr/Desktop 파일 누락, 버전 불일치 및 중단/불완전 결과의 후속 중앙 검증은 아래 실패 검사 결과에 기록함. 최신 고지 `c1fcde75d180de325981ee14916685e882d3148c`를 반영해 재조립한 후보도 아님.

Windows 런타임의 별도 라이선스 조건에 대한 사용자 확인과 배포 의무 검토가 남아 있음. 자체 Zipelo MIT와 제3자 조건을 분리하며 releaseReady=false, 기본 self-contained 배포 및 PR #9의 main 미병합 상태를 유지함. 조립 시 기록된 actualExecutionValidated=false를 이 문서 업데이트로 수정하지 않으며 중앙 실행 기록은 별도 근거로 구분함. 공개 업로드나 릴리스 승인을 의미하지 않음.

## 실패 검사 명세와 실행 결과

`tests/shared-runtime/failure-cases.json`은 작성 당시 미실행 상태인 12개 실패 사례의 명세임. 파일의 status=not_run은 당시 계획 상태로 보존하며 후속 실행 결과는 아래 중앙 보고와 구분함. 자동 실행기나 업데이트 구현을 추가한 것이 아님. 중앙 담당은 사례별 독립된 새 사본에서 실행함. 원본 후보/입력/사용자 파일은 읽기 전용이고 전역 PATH, DOTNET_ROOT, 레지스트리, 권한은 변경하지 않음. 환경 오염 값은 해당 자식 프로세스에만 전달함.

사본 준비 전 원본 manifest와 모든 파일 해시, 해당 RID/계획을 대조하고, symlink/reparse point/hardlink가 없는 별도 테스트 루트에 일반 파일 복사를 만듦. 사례 사이 사본을 재사용하지 않음. 제외/이름 변경은 사본을 만들 때 지정하며 원본에서는 삭제/이동하지 않음. 기존 데이터 보존 표식도 테스트 루트 안에만 둠. 부정 사본의 manifest는 성공처럼 다시 쓰지 않음.

각 결과에 고정 후보/manifest/계획 해시, 사본 전체 변경 목록, 명령/프로세스별 환경, 종료 코드/오류/host trace, 파일 해시와 보존 표식 전후를 남김. 원본 후보 전체가 변하지 않았는지도 재대조함. 제한 시간 초과나 오류 대화상자만으로 실패 폐쇄를 통과 처리하지 않으며 필요한 실제 경로가 실행되지 않았으면 inconclusive로 기록함. 검증 담당이 시작한 PID만 종료/회수할 수 있고 다른 프로세스는 건드리지 않음.

Desktop 전용 파일 누락은 Worker가 Core 런타임으로 계속 정상 동작하는 대조와 구분함. native 파일이 아직 로드되지 않은 화면을 표시했다는 이유로 검사를 통과시키지 않음. 업데이트는 현재 미구현이므로 중단 사례는 새 후보/게시 영수증의 검증 및 기존 정상본 보존을 검사하는 것이며 자동 업데이트/롤백 지원을 선언하지 않음. 최신 고지를 적용한 별도 재조립과 라이선스 확인, 미검증 Ubuntu 화면은 후속 검증 조건으로 유지함.

### 중앙 실패 검사 결과

중앙 최종 보고서 `shared-runtime-failures-742de2fd/final-report.json`을 읽기 전용으로 대조함. SHA-256은 `07540c1cd72614da36acd05acfb86c0ecb709f3968ac8983b71a6d642c097768`이며 전달된 값과 일치함. 명세 기준은 PR #9 `742de2fdf78e2a70fa5ffdf0f3e1a544f8593d9e`이고, 실제 기능 코드/소스/고지는 위 고정 후보의 de0d7ccd/main626483e8/8e800579 조합임. 최신 main 또는 최신 고지 조합의 재검증이 아님.

12개 정의의 19개 RID 인스턴스가 수용 기준을 통과함. Linux 8개, Windows 11개이며 실패 0개, 미확정 0개임. 보고서의 별도 Linux 정상 대조 1건은 이 19개에 더하지 않음. 중앙 명세 사본 SHA-256 `13cfaf234e1264a7310f9dce7cfa985c530458902066eb2de2c1bfd39bf1ded4`는 소스 명세와 직렬화 바이트가 다르지만 JSON 값 전체의 동일성을 대조함.

- CoreCLR 누락과 Core 버전 폴더 불일치는 UI/Worker가 외부 런타임으로 대체 실행되지 않고 실패함. 버전 검사는 10.0.14 폴더를 10.0.13 이름으로 바꾼 시뮬레이션이며 실제 구버전 바이너리 실행이 아님.
- Windows 정상 UI가 동봉 런타임과 wpfgfx_cor3.dll을 실제 로드한 대조 뒤 Desktop native/managed 파일 누락을 검사함. 처음 4초에 종료되지 않은 경우도 이후 필수 파일 로드 예외와 CLR 예외 종료 코드를 확인했으며 시간 초과를 통과 근거로 삼지 않음. Desktop만 손상된 사본의 Worker 목록은 정상임. Desktop 버전 폴더 불일치도 UI 실패와 Worker 정상 대조를 구분함.
- 영수증 누락, 부분/잘린 게시 결과, 잘못된 계획 해시와 기존 출력은 finalize에서 의도대로 거부함. 중단된 후보 전송은 전체 manifest 대조에서 누락을 검출해 실행하지 않음. 잘린 apphost도 실행하지 않음. 일부 래퍼의 예상 오류 문자열 불일치는 실제 거부 메시지와 보존 결과를 검토해 분류한 것으로, 재실행하거나 소스/기대값을 바꾼 결과가 아님.
- 원본 후보, 준비 계획과 게시 결과 6개 대상의 사후 대조 기록은 allUnchanged=true임. 중앙 담당은 프로세스 잔류 0을 보고함. 문서 담당이 기능을 재실행하거나 프로세스를 직접 조사한 결과가 아님.

Windows 오류 대화상자 억제는 자식 프로세스의 DOTNET_DISABLE_GUI_ERRORS=1에만 적용했고 실제 stderr/종료 코드/host trace로 판정함. 정상 Windows UI의 관찰 후 종료는 정상 종료 품질 검증이 아님. Ubuntu UI의 프로세스/동봉 런타임 확인과 0x0 화면으로 제외된 GUI 검증을 구분함. 전역 설정과 보안 조건은 변경하지 않음.

이 결과는 비공개 고정 후보의 실패 시험임. 설치, 실제 업데이트/롤백, SDK 없는 깨끗한 Windows, 실제 구버전 런타임, 최신 고지/라이선스 또는 출시 준비의 검증으로 확대하지 않음. releaseReady=false, 기본 self-contained 방식, 라이선스/배포 보류 및 main 미병합 상태를 유지함. 원문 증거, 바이너리와 메모리 덤프는 업로드하지 않으며 이 요약 문서만 갱신함.

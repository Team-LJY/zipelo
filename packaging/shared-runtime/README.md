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

출력은 존재하지 않는 새 폴더만 허용함. 기존 폴더와 병합하거나 업데이트하지 않음. 임시 조립 후 필수 검증과 고지 복사가 끝나면 새 출력 폴더를 예약하고 후보를 이동함. 이동 실패는 이번에 예약한 출력만 정리함. 프로세스 강제 종료/전원 손실 때는 부분 출력이 남을 수 있으므로 후보 manifest와 전체 해시를 대조하고 불완전한 후보는 실행하지 않아야 함.

`shared-runtime-candidate.json`은 RID/버전/입력 해시/apphost 원본 해시/전체 파일 크기와 SHA-256을 기록함. 자체 manifest 파일은 순환 해시를 피하기 위해 목록에서 제외함. `releaseReady=false`, `actualExecutionValidated=false`는 실행 검증 전 상태이며 조립 성공을 기능 검증으로 표현하지 않음.

## 검증 상태와 수용 기준

작성 환경에서는 새 SDK 실행, 복원, 공식 바이너리 다운로드, 실제 조립 및 VM 실행을 하지 않음. XML/Python 구문, 기존 targets와의 최소 차이 및 공식 해시 연결만 정적으로 확인함. 아카이브 내부 경로와 실제 SDK가 생성한 apphost 옵션도 두 OS 실조립으로 확인해야 함. 공식 아카이브가 선언한 설치 트리와 다르면 오류로 중단하며 파일을 임의로 골라 통과시키지 않음.

검증 담당은 두 OS에서 전역 .NET/SDK가 없는 조건과 무관한 CWD, 거짓 DOTNET_ROOT/PATH를 사용해 앱과 Worker가 동봉 런타임만 선택하는지 확인해야 함. 런타임 폴더, hostfxr, coreclr, Desktop 프레임워크 각각의 누락/잘못된 버전/RID에서는 전역 설치가 있어도 실패해야 함. 정상 ZIP 목록/개발 검증 해제, 원본/충돌 보존, 진행 중 취소, 중복 요청과 자식 종료를 확인함.

현재 후보는 업데이트를 지원하지 않음. 실패 후보가 기존 검증 출력에 영향을 주지 않는지 확인하고, 향후 업데이트는 버전별 전체 후보 검증 후 교체 및 실패 복구를 별도 설계해야 함. 런타임 한 벌의 보안 업데이트가 UI와 Worker 모두에 영향을 주므로 두 프로세스를 함께 회귀 검증해야 함. 크기는 전체 파일 논리 크기/설치 디스크 점유/다운로드 압축 크기를 구분해 기존 self-contained 결과와 실제 비교함.

## 공식 근거

- https://learn.microsoft.com/en-us/dotnet/core/deploying/ 의 AppHostDotNetSearch/AppHostRelativeDotNet 계약.
- https://github.com/dotnet/designs/blob/main/proposed/apphost-embed-install-location.md 의 AppRelative 단독 검색과 실패 동작.
- https://github.com/dotnet/sdk/blob/main/src/Tasks/Microsoft.NET.Build.Tasks/ResolveAppHosts.cs 의 RuntimeFrameworkVersion/apphost pack 관계.
- https://builds.dotnet.microsoft.com/dotnet/release-metadata/10.0/releases.json 의 10.0.14 아카이브 및 SHA-512.

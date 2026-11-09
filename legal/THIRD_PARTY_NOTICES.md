# 제3자 소프트웨어 고지 검토 초안

이 고지 묶음은 **검토 미완료이며 출시용 고지 완료본이 아닙니다.** `dependency-inventory.json`의 `release_ready`는 `false`입니다. 확인된 Windows 라이선스의 배포 의무 이행과 최종 배포물의 고지 포함 확인이 남아 있습니다.

## 단일 파일 배포 입력

패키징의 단일 --notices 입력에는 [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt)를 사용합니다. 구성요소 18개의 정확한 버전과 적용 파일 982개, 확보한 라이선스/고지/공식 안내 원문 22개 전체가 들어 있습니다. Windows 파일에는 공식 파일별 배포 라이선스도 함께 표시합니다. 다른 파일의 상대 링크를 열어 원문을 찾아야 하는 구조가 아닙니다.

[notice-bundle-manifest.json](notice-bundle-manifest.json)에 원문 구간의 바이트 위치와 길이, 원문 및 포함 구간 SHA-256과 바이트 일치 결과, 구성요소 절의 해시를 기록했습니다. 기존 원문 17개는 그대로 유지하며 공식 안내와 Windows SDK 자료 5개를 추가했습니다. 동일 본문의 별도 출처도 각 출처 및 동일 해시를 추적합니다.

입력 inventory의 정규화 SHA-256은 e09abdef1cc7e244986f1ffb131b8f07da55b016da93e1d6a901ade7ed4ab359입니다. 현재 inventory에서 distribution_notice_bundle 항목만 제외하고 ensure_ascii=False, indent=4, UTF-8, LF 및 마지막 LF로 직렬화하면 재현할 수 있습니다. 이 보완 전 기준은 커밋 8e80057938f9bbc0f67d8cb5236043a8a021a350입니다. 출력 해시와 입력 정규화 해시를 구분해 순환 참조를 피했습니다.

TXT 내용 및 공식 파일명 매핑은 대조했지만 실제 재배포 의무 이행과 두 OS 최종 조립물 포함은 미완료입니다. 공식 .NET archives 자체 LICENSE/ThirdPartyNotices도 최종 배포에 별도로 보존해야 합니다.

## 범위와 Zipelo 자체 라이선스

문서 검토의 기준 소스는 `626483e8935aaa03d5a34feebc61be2f9180b80d`입니다. 실제 산출물의 소스 조합은 각 round의 snapshot manifest에 기록된 별도 refs이며 inventory에 그대로 남겼습니다. 이 기준 main에서 산출물을 다시 빌드한 것은 아닙니다. 기존 self-contained 방식의 Windows round10과 Ubuntu round9 산출물만 대상으로 삼았습니다. 공유 런타임 패키징 실험, 후속 스트리밍 기능, 개발 SDK와 운영체제가 제공하는 라이브러리는 이 배포 구성에 포함했다고 간주하지 않았습니다.

Zipelo 자체 코드는 루트 [LICENSE](../LICENSE)의 MIT 라이선스와 `2026 Team-LJY contributors` 귀속을 따릅니다. 이 결정은 동봉되는 외부 코드의 저작권, 라이선스, 고지 의무를 대체하지 않습니다. 외부 구성요소에는 아래 원문과 각 원문의 별도 조건이 적용됩니다.

## 검증 근거와 한계

[dependency-inventory.json](dependency-inventory.json)에 입력 manifest와 deps의 SHA-256, 정확한 패키지 식별자, 공식 출처, 원문 고지 해시와 산출물별 파일 목록을 기록했습니다. 사용자 로컬 경로, 실행 로그, SDK나 바이너리는 이 묶음에 복사하지 않았습니다.

- Ubuntu round9는 418개 파일의 manifest와 앱 및 Worker의 deps/runtimeconfig 원문을 확인했습니다. 네 메타데이터 파일은 manifest 해시와 일치합니다.
- Ubuntu의 NuGet 런타임 자산 26개는 정확한 버전의 공식 NuGet 아카이브 내부 파일과 SHA-256이 일치합니다. 앱과 Worker의 .NET 런타임 파일 374개도 공식 Linux 런타임 아카이브 내부 파일과 일치합니다. 나머지 18개는 제품 파일과 생성된 앱 호스트 및 메타데이터로 구분했습니다. DLL이나 라이브러리를 실행하지 않았습니다.
- Windows round10은 600개 파일, 227,384,220바이트의 inventory와 소스 snapshot manifest를 확인했습니다. 추가 제공된 앱 및 Worker의 deps/runtimeconfig 네 원문은 export manifest와 기존 inventory 양쪽의 크기 및 SHA-256과 일치합니다. 앱의 `Microsoft.NETCore.App` 및 `Microsoft.WindowsDesktop.App`, Worker의 `Microsoft.NETCore.App`은 모두 `10.0.14`입니다. 외부 자산 582개는 정확한 공식 NuGet runtimepack 및 Windows 런타임 ZIP 내부 파일 양쪽과 SHA-256이 일치합니다. 나머지 18개는 제품 파일, 생성된 앱 호스트와 메타데이터입니다.
- 두 산출물 목록에는 이름에 license, licence, notice, copying이 들어가는 파일이 없습니다. 이는 해당 이름의 독립 파일 누락을 뜻하며, DLL 내부의 고지나 법적 준수 여부 전체를 판정한 결과는 아닙니다. 저장소에 고지를 추가하는 것만으로 설치물에 포함됐다고 볼 수 없습니다.
- deps에 기록된 NuGet 패키지 content hash와 현재 공식 아카이브 전체의 SHA-512는 서로 다릅니다. 그 원인을 추정하거나 두 해시를 같다고 취급하지 않았습니다. 실제 동봉 자산은 아카이브 내부 파일별 SHA-256으로 별도 대조했고, 양쪽 패키지 해시를 inventory에 보존했습니다.

## Ubuntu에서 확인한 구성요소와 원문

아래 패키지의 nuspec은 최상위 패키지 라이선스를 MIT로 선언합니다. 내부에 포함된 제3자 코드까지 모두 MIT라는 뜻은 아닙니다. 패키지 메타데이터의 저작권 문구는 inventory에 그대로 기록했고, 별도 고지 원문은 번역하거나 축약하지 않았습니다.

### .NET 10.0.14

앱과 Worker의 runtimeconfig는 `Microsoft.NETCore.App 10.0.14`, RID는 `linux-x64`를 지정합니다. [Microsoft 공식 releases.json](https://builds.dotnet.microsoft.com/dotnet/release-metadata/10.0/releases.json)의 SHA-512와 `dotnet-runtime-10.0.14-linux-x64.tar.gz` 전체 해시를 대조한 뒤 다음 텍스트만 추출했습니다.

- [원본 LICENSE.txt](notices/dotnet-runtime-10.0.14-linux-x64/LICENSE.txt)
- [원본 ThirdPartyNotices.txt](notices/dotnet-runtime-10.0.14-linux-x64/ThirdPartyNotices.txt)

최종 배포에서도 공식 런타임 아카이브의 해당 원본 파일과 이름을 유지해야 합니다. 이 문서나 자체 MIT 고지로 원문을 대체하지 않습니다. Windows 구성과 원문 출처는 아래 Windows 절에서 별도로 설명합니다.

### Avalonia 11.3.9

실제 자산이 포함된 패키지는 `Avalonia`, `Avalonia.Desktop`, `Avalonia.FreeDesktop`, `Avalonia.Native`, `Avalonia.Remote.Protocol`, `Avalonia.Skia`, `Avalonia.Themes.Fluent`, `Avalonia.Win32`, `Avalonia.X11`이며 모두 `11.3.9`입니다. Linux에서 사용되지 않는 플랫폼 코드라도 실제 DLL이 동봉된 경우 목록에서 제외하지 않았습니다.

각 nuspec이 지정한 공식 소스 커밋 `4e302560fc9f84de1fe64360e29165574cf2ea65`의 다음 원문을 보존했습니다. 원문 Git blob 해시도 확인했습니다.

- [Avalonia LICENSE](notices/avalonia-11.3.9/LICENSE.txt)
- [Avalonia 전체 NOTICE](notices/avalonia-11.3.9/NOTICE.txt)

NOTICE에는 MIT 외에 MS-PL, BSD와 Apache-2.0 등 포함 코드의 별도 고지가 있습니다. 목록을 임의로 필터링하지 않았으며, 여기에 열거된 모든 프로젝트가 독립 라이브러리로 동봉된다고 단정하지 않습니다.

### SkiaSharp와 HarfBuzzSharp

`SkiaSharp`와 `SkiaSharp.NativeAssets.Linux`는 `2.88.9`, `HarfBuzzSharp`와 `HarfBuzzSharp.NativeAssets.Linux`는 `8.3.1.1`입니다. 관리 DLL 두 개와 `libSkiaSharp.so`, `libHarfBuzzSharp.so`를 공식 패키지 자산과 대조했습니다.

공식 패키지에서 발견한 동일 바이트 원문은 한 번만 저장하고 모든 출처와 내부 경로를 inventory에 연결했습니다.

- [Microsoft 패키지 LICENSE 원문](notices/package-texts/89101e35a8c66fd4-LICENSE.txt)
- [SkiaSharp 및 HarfBuzzSharp 전체 THIRD-PARTY-NOTICES 원문](notices/package-texts/21504c46c4c58aa6-THIRD-PARTY-NOTICES.txt)

HarfBuzzSharp 관리 패키지에는 LICENSE만 있으며, Linux 네이티브 패키지의 제3자 고지도 함께 보존했습니다. 네이티브 라이브러리의 포함 코드에는 원문에 기재된 개별 라이선스가 적용됩니다.

### MicroCom.Runtime과 Tmds.DBus.Protocol

- `MicroCom.Runtime 0.11.0`: [MIT LICENSE 원문](notices/microcom.runtime-0.11.0/LICENSE.txt). nuspec에는 소스 커밋이 없어, 공식 저장소에서 `Directory.Build.props`가 `0.11.0`을 명시하는 커밋 `7e773759cfc1012a5d080d4129c4718a662884b0`의 원문을 사용했습니다. 이 커밋이 공개 패키지의 빌드 커밋이라고 단정하지 않습니다.
- `Tmds.DBus.Protocol 0.21.3`: [COPYING 원문](notices/tmds.dbus.protocol-0.21.3/COPYING.txt). nuspec에 지정된 공식 소스 커밋 `8cb04f66c330b64244e996ff68f685f27f7381a0`의 원문을 사용했습니다.

### 공통으로 참조되는 라이선스 본문

포함 코드 고지에서 참조하는 [Apache License 2.0 공식 전문](notices/apache-2.0/LICENSE-2.0.txt)도 보존했습니다. 이 일반 라이선스 본문은 각 저작권자의 개별 고지나 NOTICE를 대체하지 않습니다.

## Windows round10의 공식 배포 라이선스

앱의 Microsoft.NETCore.App.Runtime.win-x64 및 Microsoft.WindowsDesktop.App.Runtime.win-x64, Worker의 Microsoft.NETCore.App.Runtime.win-x64는 모두 10.0.14입니다. 앱/Worker deps 및 runtimeconfig 네 원문은 export manifest와 기존 600개 파일 inventory의 크기/SHA-256 양쪽과 일치했습니다. 외부 자산 582개(.NETCore 373개, WindowsDesktop 209개)는 정확한 공식 NuGet runtimepack 및 런타임 ZIP 내부 파일 양쪽과 일치합니다. WindowsDesktop 수치에는 실제 동봉 언어 리소스 156개가 포함됩니다.

### runtime packs도 제품 배포 조건의 대상

[공식 Product distributions 안내](https://github.com/dotnet/core/blob/8c8e5836c343f854b65437dfedb13598d3aa3707/license-information.md)는 Windows 다운로드와 runtime packs를 함께 제품 배포로 명시합니다. 소스 코드와 일반 라이브러리 패키지의 MIT 설명을 Windows 제품 배포 전체의 라이선스로 확장할 수 없습니다.

[안내 원문 사본](notices/dotnet-windows-policy/license-information.txt)과 [공식 Windows 파일 안내 사본](notices/dotnet-windows-policy/license-information-windows.txt)을 고정 커밋과 Git blob 해시로 보존했습니다. 두 문서는 스스로 라이선스가 아닌 정보 안내라고 명시합니다. 실제 .NET Library 및 SDK 라이선스 본문도 별도로 포함했습니다.

### 실제 파일별 적용

[공식 파일 안내](https://github.com/dotnet/core/blob/6c25b9da384e8d8a16fd12c79da8a8aa6bdfc6a1/license-information-windows.md)를 현재 inventory의 파일명에 대조했습니다.

- .NET Library 조건 7개: coreclr.dll, worker/coreclr.dll, Microsoft.DiaSymReader.Native.amd64.dll, worker/Microsoft.DiaSymReader.Native.amd64.dll, PresentationNative_cor3.dll, vcruntime140_cor3.dll, wpfgfx_cor3.dll.
- Windows SDK 조건 1개: D3DCompiler_47_cor3.dll.
- 나머지 외부 파일 574개: 공식 안내의 나머지 파일에 대한 MIT 규칙으로 연결했습니다. 포함 코드의 별도 고지는 계속 보존합니다.

전체 경로, SHA-256과 파일 버전은 inventory의 windows_licensing 및 파일별 windows_distribution_license에 연결했습니다. NuGet MIT 선언은 원래 메타데이터로 유지하며, 위 개별 파일 조건을 덮어쓰는 선택지로 취급하지 않습니다. Zipelo 자체 MIT는 별도로 유지합니다.

### .NET 원문

- [공식 Windows runtime ZIP의 LICENSE.txt](notices/dotnet-runtime-10.0.14-win-x64/LICENSE.txt): Microsoft .NET Library 조건 전문.
- [같은 ZIP의 ThirdPartyNotices.txt](notices/dotnet-runtime-10.0.14-win-x64/ThirdPartyNotices.txt).
- [정확한 .NETCore NuGet runtimepack의 LICENSE.TXT](notices/microsoft.netcore.app.runtime.win-x64-10.0.14/LICENSE.TXT) 및 [THIRD-PARTY-NOTICES.TXT](notices/microsoft.netcore.app.runtime.win-x64-10.0.14/THIRD-PARTY-NOTICES.TXT).
- [정확한 WindowsDesktop NuGet runtimepack의 LICENSE](notices/microsoft.windowsdesktop.app.runtime.win-x64-10.0.14/LICENSE).
- [공식 dotnet/core MIT 본문](notices/dotnet-windows-policy/LICENSE-MIT.txt). 같은 바이트의 기존 Linux 원문도 원래 출처대로 보존합니다.
- [고정 빌드 소스 커밋의 WPF LICENSE](notices/windowsdesktop-10.0.14-source/wpf-LICENSE.TXT), [WPF 고지](notices/windowsdesktop-10.0.14-source/wpf-THIRD-PARTY-NOTICES.TXT), [Windows Forms 고지](notices/windowsdesktop-10.0.14-source/winforms-THIRD-PARTY-NOTICES.TXT).

공식 WindowsDesktop ZIP에는 별도 고지 파일이 없고 NuGet에는 LICENSE만 있어, nuspec이 지정한 dotnet/dotnet 커밋 0427b9abbead3fa64c6655eb4290c537397131fa의 소스 고지도 유지합니다. SDK 문서를 런타임 아카이브 원문 대신 사용하지 않습니다.

### Windows SDK 원문과 D3D 적용

[Windows SDK license](https://learn.microsoft.com/en-us/legal/windows-sdk/license)와 [REDIST 목록](https://learn.microsoft.com/en-us/legal/windows-sdk/redist)의 공식 서버 Markdown 응답을 전체 보존했습니다. 응답 해시, 게시 소스 커밋 및 갱신 시각은 출처 자료에 기록했습니다. 원문의 표현과 서식을 임의로 고치지 않았습니다.

- [SDK 라이선스 전문 사본](notices/windows-sdk/LICENSE-MICROSOFT-LEARN.txt).
- [REDIST 목록 전문 사본](notices/windows-sdk/REDIST-MICROSOFT-LEARN.txt).

공식 .NET 파일 안내가 D3DCompiler_47_cor3.dll을 SDK 조건에 연결합니다. SDK REDIST에는 x64 d3dcompiler_47.dll이 Classic Windows 앱용으로 나오며 Universal Windows 앱 재배포는 허용 대상에서 제외됩니다. SDK 목록에 _cor3 이름 자체가 있다고 주장하지 않습니다. 현재 DLL은 공식 .NET 패키지 및 ZIP과 해시가 같고 파일 버전은 10.0.26100.7705입니다. 별도 SDK 원본 바이너리 대조나 SDK 취득 권리 확인까지 완료한 것은 아닙니다. 이 DLL은 Ubuntu manifest에는 없습니다.

## 남은 배포 의무 확인

공식 파일별 적용 구분은 확인했으며 다음 의무의 이행은 확인되지 않았습니다.

1. .NET Library 3.a의 조건에 따른 목적 코드 배포, 실질적인 주 기능, 배포자 및 외부 최종 사용자의 보호 약정 동의, 프로그램의 유효한 저작권 표시, 면책/방어 의무와 고지/상표/Excluded License 등 제한 준수.
2. SDK 2.a 및 REDIST의 적법한 취득/권리와 적용 버전, 명시된 변경 없는 목적 코드의 프로그램 동봉, 실질적인 주 기능, 보호 약정 동의, 저작권 표시와 면책 의무 및 배포 제한 준수. Microsoft 운영체제 플랫폼 제한과 D3D의 Classic Windows 전용 조건도 포함합니다.
3. SDK의 setup 동봉분은 해당 setup을 변경 없이 함께 배포하는 조건 등 해당되는 추가 조건을 확인해야 합니다. .lib 및 Windows Performance Toolkit 전용 조항은 조건부이며, 현재 산출물에 해당 파일들이 동봉됐다고 주장하지 않습니다.
4. 고지 파일을 복사하는 것만으로 보호 약정 동의나 다른 배포 의무의 이행이 증명되지는 않습니다. 이 변경은 약정 수락이나 제품 UI/패키징 변경을 수행하지 않습니다.
5. 포함 코드, 리소스 및 생성 apphost의 최종 출처/고지 적용과 실제 두 OS 최종 설치물/압축 배포물에 통합 TXT 및 별도 런타임 원문이 들어갔는지 확인해야 합니다.

Windows 메타데이터 미제공과 runtime packs의 정책 적용 여부라는 이전 불확실성은 해소했습니다. 공개 릴리스 승인이나 법적 보장은 없으며 실제 배포 의무 이행과 최종 조립 확인 전에는 완료로 표시하지 않습니다.

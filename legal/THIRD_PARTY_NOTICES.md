# 제3자 소프트웨어 고지 검토 초안

이 고지 묶음은 **검토 미완료이며 출시용 고지 완료본이 아닙니다.** `dependency-inventory.json`의 `release_ready`는 `false`입니다. Windows 배포 경로별 라이선스 적용 조건과 최종 배포물의 고지 포함 확인이 남아 있습니다.

## 단일 파일 배포 입력

패키징의 단일 `--notices` 입력에는 [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt)를 사용합니다. 이 TXT에는 구성요소 18개의 정확한 패키지와 버전, snapshot별 적용 파일 982개 및 확보한 원문 17개 전체가 들어 있습니다. 다른 파일의 상대 링크를 열어야 원문을 읽을 수 있는 구조가 아닙니다.

[notice-bundle-manifest.json](notice-bundle-manifest.json)에 각 원문의 SHA-256, TXT 안의 바이트 시작 위치와 길이, 원문과의 바이트 일치 결과를 기록했습니다. 구성요소별 적용 파일 절의 해시와 범위도 기록했습니다. 원문 구간의 BOM과 줄바꿈까지 그대로 보존했으며, 여러 패키지가 같은 원문을 참조하면 동일한 원문 번호로 연결합니다. 기존 원문 파일과 출처 inventory는 유지합니다.

번들 입력 inventory는 커밋 `6d9a02d13a443d245eb7a13d8f7a4c4f29b0e7f5`의 SHA-256 `79daa5d4e52b729a9d31a8fb975b3d363b3d41bfcfaa780290bb26ec033756bd`로 고정했습니다. 현재 inventory의 `distribution_notice_bundle`은 생성된 TXT와 대조 manifest의 출력 해시를 가리킵니다.

TXT의 내용 대조는 완료했지만, Windows 및 Ubuntu 최종 조립물 포함 확인과 라이선스 적용 검토는 미완료입니다. 공식 .NET 아카이브 자체의 LICENSE와 ThirdPartyNotices도 최종 배포에 별도로 보존해야 합니다. 이 통합 파일은 그 의무를 대체하지 않습니다.

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

## Windows round10에서 확인한 구성과 원문

앱은 `Microsoft.NETCore.App.Runtime.win-x64/10.0.14`와 `Microsoft.WindowsDesktop.App.Runtime.win-x64/10.0.14`, Worker는 전자만 사용합니다. 이 Windows deps에는 Avalonia, SkiaSharp 또는 HarfBuzzSharp 패키지가 없습니다. Ubuntu 구성요소 목록을 Windows의 포함 목록으로 복사하지 않았습니다.

- .NETCore: 앱 186개와 Worker 187개, 합계 373개 자산을 공식 패키지 및 아카이브와 대조했습니다. `hostfxr.dll`은 아카이브의 `host/fxr/10.0.14/` 원본에 연결했습니다.
- WindowsDesktop: deps에 나열된 53개와 실제 동봉된 언어 리소스 156개, 합계 209개 자산을 대조했습니다. 리소스가 deps에 개별 나열되지 않았다는 이유로 제외하지 않았습니다.
- 공개 메타데이터 export manifest의 SHA-256은 `c2964c5cd3e9cababb5191a181a2b75ca5b062de6b6e98c87a395e439b0ed2a4`입니다. 네 원문의 크기와 해시, 소스 snapshot 연결은 inventory에 기록했습니다. 실제 Windows 바이너리를 실행하거나 프로젝트를 재빌드하지 않았습니다.

### 공식 Windows 런타임 ZIP 원문

Microsoft releases.json에 게시된 `dotnet-runtime-10.0.14-win-x64.zip`의 전체 SHA-512를 확인한 뒤 다음 원문을 그대로 보존했습니다.

- [LICENSE.txt 원문](notices/dotnet-runtime-10.0.14-win-x64/LICENSE.txt)
- [ThirdPartyNotices.txt 원문](notices/dotnet-runtime-10.0.14-win-x64/ThirdPartyNotices.txt)

이 ZIP의 LICENSE는 `MICROSOFT SOFTWARE LICENSE TERMS / MICROSOFT .NET LIBRARY`입니다. Linux 아카이브의 MIT 본문과 다르므로 MIT로 다시 이름 붙이거나 자체 MIT LICENSE로 대체하지 않았습니다. 재배포에 관한 조건도 원문에 포함되어 있으며 최종 배포 경로에서의 적용 검토가 남아 있습니다.

### 정확한 NuGet runtimepack 원문

두 공식 NuGet runtimepack의 nuspec은 MIT를 선언합니다. 선택된 실제 자산은 각 패키지 내부 파일과도 일치합니다. 다음은 그 패키지에서 추출한 원문이며 ZIP의 별도 조건과 구별합니다.

- [Microsoft.NETCore.App.Runtime.win-x64 10.0.14 LICENSE.TXT](notices/microsoft.netcore.app.runtime.win-x64-10.0.14/LICENSE.TXT)
- [같은 패키지의 THIRD-PARTY-NOTICES.TXT](notices/microsoft.netcore.app.runtime.win-x64-10.0.14/THIRD-PARTY-NOTICES.TXT)
- [Microsoft.WindowsDesktop.App.Runtime.win-x64 10.0.14 LICENSE](notices/microsoft.windowsdesktop.app.runtime.win-x64-10.0.14/LICENSE)

출처별 원문의 바이트, 이름과 대소문자를 유지했습니다. NuGet의 MIT 선언이 ZIP의 조건이나 모든 동봉 네이티브 코드의 개별 조건을 대체한다고 단정하지 않습니다.

### WindowsDesktop 소스 고지

`windowsdesktop-runtime-10.0.14-win-x64.zip`도 Microsoft의 공식 SHA-512와 대조했지만, 해당 ZIP에는 license, licence, notice, copying 이름의 파일이 없었습니다. NuGet WindowsDesktop 패키지에는 위 LICENSE가 있으나 별도 THIRD-PARTY-NOTICES는 없습니다. 이를 SDK 문서로 대신하지 않았습니다.

두 nuspec이 가리키는 `dotnet/dotnet` 소스 커밋 `0427b9abbead3fa64c6655eb4290c537397131fa`에서 다음 원문을 확보하고 Git blob 해시를 확인했습니다.

- [WPF LICENSE.TXT](notices/windowsdesktop-10.0.14-source/wpf-LICENSE.TXT)
- [WPF THIRD-PARTY-NOTICES.TXT](notices/windowsdesktop-10.0.14-source/wpf-THIRD-PARTY-NOTICES.TXT)
- [Windows Forms THIRD-PARTY-NOTICES.TXT](notices/windowsdesktop-10.0.14-source/winforms-THIRD-PARTY-NOTICES.TXT)

이 집계 원문에 등장하는 모든 내부 프로젝트가 각각 별도 파일로 동봉됐다고 주장하지 않습니다. 실제 포함 DLL과의 연결은 inventory의 WindowsDesktop 자산 목록을 기준으로 하며, 내부 코드별 최종 고지 적용 검토는 계속 필요합니다.

## 남은 확인 항목

1. Windows ZIP의 Microsoft .NET Library 조건과 MIT를 선언하는 공개 runtimepack의 출처를 구분해 최종 배포 경로에 적용되는 조건을 검토해야 합니다. 특히 실제 동봉된 `D3DCompiler_47_cor3.dll`과 `vcruntime140_cor3.dll`은 각각 deps의 파일 버전 `10.0.26100.7705`, `14.44.35211.0`까지 기록했지만, 모든 네이티브 코드의 재배포 조건이 MIT 선언 하나로 해결됐다고 표시하지 않습니다.
2. 보존한 Avalonia, SkiaSharp/HarfBuzz, .NET 및 WindowsDesktop의 전체 고지에 있는 내부 코드, 리소스와 생성된 앱 호스트의 출처 및 조건을 최종 배포 구성에서 검토해야 합니다. 참조된 전문이나 소스 제공 의무가 있으면 해당 조건대로 처리해야 하며 완료로 표시하지 않습니다.
3. Windows와 Ubuntu의 기존 검증 산출물에는 독립 고지 파일이 목록상 누락되어 있습니다. 최종 설치물과 압축 배포물에 적용 원문, 패키지 저작권 정보와 이 inventory가 실제 포함되는지 새 manifest로 확인해야 합니다. 이번 변경은 `legal/` 자료만 준비하며 패키징 코드나 기존 바이너리를 수정하지 않습니다.

Windows 메타데이터 미제공 문제는 해소됐습니다. 최종 고지 포함과 위 적용 조건이 검증되기 전에는 이 초안을 출시용 고지 완료본으로 표시하지 않습니다.

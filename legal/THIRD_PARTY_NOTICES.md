# 제3자 소프트웨어 고지 검토 초안

이 고지 묶음은 **검토 미완료이며 출시용 고지 완료본이 아닙니다.** `dependency-inventory.json`의 `release_ready`는 `false`입니다. Windows 메타데이터와 최종 배포물의 고지 포함 확인이 남아 있습니다.

## 범위와 Zipelo 자체 라이선스

문서 검토의 기준 소스는 `626483e8935aaa03d5a34feebc61be2f9180b80d`입니다. 실제 산출물의 소스 조합은 각 round의 snapshot manifest에 기록된 별도 refs이며 inventory에 그대로 남겼습니다. 이 기준 main에서 산출물을 다시 빌드한 것은 아닙니다. 기존 self-contained 방식의 Windows round10과 Ubuntu round9 산출물만 대상으로 삼았습니다. 공유 런타임 패키징 실험, 후속 스트리밍 기능, 개발 SDK와 운영체제가 제공하는 라이브러리는 이 배포 구성에 포함했다고 간주하지 않았습니다.

Zipelo 자체 코드는 루트 [LICENSE](../LICENSE)의 MIT 라이선스와 `2026 Team-LJY contributors` 귀속을 따릅니다. 이 결정은 동봉되는 외부 코드의 저작권, 라이선스, 고지 의무를 대체하지 않습니다. 외부 구성요소에는 아래 원문과 각 원문의 별도 조건이 적용됩니다.

## 검증 근거와 한계

[dependency-inventory.json](dependency-inventory.json)에 입력 manifest와 deps의 SHA-256, 정확한 패키지 식별자, 공식 출처, 원문 고지 해시와 산출물별 파일 목록을 기록했습니다. 사용자 로컬 경로, 실행 로그, SDK나 바이너리는 이 묶음에 복사하지 않았습니다.

- Ubuntu round9는 418개 파일의 manifest와 앱 및 Worker의 deps/runtimeconfig 원문을 확인했습니다. 네 메타데이터 파일은 manifest 해시와 일치합니다.
- Ubuntu의 NuGet 런타임 자산 26개는 정확한 버전의 공식 NuGet 아카이브 내부 파일과 SHA-256이 일치합니다. 앱과 Worker의 .NET 런타임 파일 374개도 공식 Linux 런타임 아카이브 내부 파일과 일치합니다. 나머지 18개는 제품 파일과 생성된 앱 호스트 및 메타데이터로 구분했습니다. DLL이나 라이브러리를 실행하지 않았습니다.
- Windows round10은 600개 파일, 227,384,220바이트의 inventory와 소스 snapshot manifest 연결만 확인했습니다. inventory가 보고하는 런타임 버전은 `10.0.14`이지만, deps/runtimeconfig 원문이 없어 정확한 Windows 런타임 팩과 고지 적용 범위를 확정하지 않았습니다.
- 두 산출물 목록에는 이름에 license, licence, notice, copying이 들어가는 파일이 없습니다. 이는 해당 이름의 독립 파일 누락을 뜻하며, DLL 내부의 고지나 법적 준수 여부 전체를 판정한 결과는 아닙니다. 저장소에 고지를 추가하는 것만으로 설치물에 포함됐다고 볼 수 없습니다.
- deps에 기록된 NuGet 패키지 content hash와 현재 공식 아카이브 전체의 SHA-512는 서로 다릅니다. 그 원인을 추정하거나 두 해시를 같다고 취급하지 않았습니다. 실제 동봉 자산은 아카이브 내부 파일별 SHA-256으로 별도 대조했고, 양쪽 패키지 해시를 inventory에 보존했습니다.

## Ubuntu에서 확인한 구성요소와 원문

아래 패키지의 nuspec은 최상위 패키지 라이선스를 MIT로 선언합니다. 내부에 포함된 제3자 코드까지 모두 MIT라는 뜻은 아닙니다. 패키지 메타데이터의 저작권 문구는 inventory에 그대로 기록했고, 별도 고지 원문은 번역하거나 축약하지 않았습니다.

### .NET 10.0.14

앱과 Worker의 runtimeconfig는 `Microsoft.NETCore.App 10.0.14`, RID는 `linux-x64`를 지정합니다. [Microsoft 공식 releases.json](https://builds.dotnet.microsoft.com/dotnet/release-metadata/10.0/releases.json)의 SHA-512와 `dotnet-runtime-10.0.14-linux-x64.tar.gz` 전체 해시를 대조한 뒤 다음 텍스트만 추출했습니다.

- [원본 LICENSE.txt](notices/dotnet-runtime-10.0.14-linux-x64/LICENSE.txt)
- [원본 ThirdPartyNotices.txt](notices/dotnet-runtime-10.0.14-linux-x64/ThirdPartyNotices.txt)

최종 배포에서도 공식 런타임 아카이브의 해당 원본 파일과 이름을 유지해야 합니다. 이 문서나 자체 MIT 고지로 원문을 대체하지 않습니다. Windows 런타임과 Windows Desktop 구성의 적용 원문은 Windows 메타데이터 확인 뒤 별도로 확정해야 합니다.

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

## 남은 확인 항목

1. Windows round10의 `Zipelo.deps.json`, `Zipelo.runtimeconfig.json`, `worker/Zipelo.Worker.deps.json`, `worker/Zipelo.Worker.runtimeconfig.json` 원문이 필요합니다. 예상 크기와 SHA-256은 inventory의 `windows-metadata-missing` 항목에 기록했습니다. 다른 빌드의 deps로 대신하지 않습니다.
2. Windows의 정확한 런타임 팩과 포함 파일을 대조한 뒤 해당 공식 아카이브의 LICENSE 및 THIRD-PARTY-NOTICES 원문을 추가해야 합니다. Ubuntu 자료를 Windows 증거로 재사용하지 않습니다.
3. 보존한 Avalonia 및 SkiaSharp/HarfBuzz 전체 고지에 있는 내부 코드, 리소스와 생성된 앱 호스트의 출처 및 조건을 최종 배포 구성에서 검토해야 합니다. 참조된 전문이나 소스 제공 의무가 있으면 해당 조건대로 처리해야 하며 완료로 표시하지 않습니다.
4. 최종 설치물과 압축 배포물에 적용 원문, 패키지 저작권 정보와 이 inventory가 실제 포함되는지 새 manifest로 확인해야 합니다. 이번 변경은 `legal/` 자료만 준비하며 패키징 코드를 수정하거나 최종 배포를 검증하지 않습니다.

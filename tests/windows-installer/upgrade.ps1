param(
    [Parameter(Mandatory = $true)][string]$Installer,
    [ValidateSet("Snapshot", "Compare", "Rollback", "Upgrade")][string]$Mode,
    [Parameter(Mandatory = $true)][string]$Evidence,
    [string]$ExpectedVersion,
    [string]$Payload
)
$ErrorActionPreference = "Stop"
$root = Join-Path $env:LOCALAPPDATA "Programs\Zipelo"
$names = @("Zipelo.exe", "zipelo-archive-worker.exe", "LICENSE", "THIRD-PARTY-LICENSES.txt", "BUILD-INFO.json", "Uninstall.exe", "install.ini")

function Snapshot {
    $files = [ordered]@{}
    foreach ($name in $names) {
        $files[$name] = (Get-FileHash (Join-Path $root $name) -Algorithm SHA256).Hash
    }
    $keys = @("Software\Zipelo", "Software\Classes\Zipelo.Archive", "Software\Microsoft\Windows\CurrentVersion\Uninstall\Zipelo")
    foreach ($extension in @(".zip", ".7z", ".tar", ".gz", ".tgz")) {
        $keys += "Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\$extension\UserChoice"
    }
    $registry = [ordered]@{}
    foreach ($key in $keys) {
        $item = Get-Item "HKCU:\$key" -ErrorAction SilentlyContinue
        if ($null -eq $item) { continue }
        $items = @($item) + @(Get-ChildItem "HKCU:\$key" -Recurse)
        foreach ($child in $items) {
            foreach ($name in ($child.GetValueNames() | Sort-Object)) {
                $registry["$($child.Name)\$name"] = $child.GetValue($name)
            }
        }
    }
    $registry["RegisteredApplications\Zipelo"] = (Get-Item "HKCU:\Software\RegisteredApplications").GetValue("Zipelo")
    return [ordered]@{
        files = $files
        shortcut = (Get-FileHash (Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\Zipelo.lnk")).Hash
        registry = $registry
    }
}

function RunInstaller {
    $process = Start-Process $Installer -ArgumentList "/S" -PassThru
    if (-not $process.WaitForExit(60000)) {
        throw "설치가 제한 시간 안에 종료되지 않음. VM에서 상태 확인이 필요함."
    }
    return $process.ExitCode
}

function AssertEqual($expected, $actual, $message) {
    if (($expected | ConvertTo-Json -Depth 8 -Compress) -cne ($actual | ConvertTo-Json -Depth 8 -Compress)) {
        throw $message
    }
}

$before = Snapshot
switch ($Mode) {
    "Snapshot" {
        $before | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 $Evidence
        "snapshot_saved"
    }
    "Compare" {
        $saved = Get-Content $Evidence -Raw | ConvertFrom-Json
        AssertEqual $saved $before "기존 설치 또는 연결이 바뀜"
        "all_seven_files_shortcut_and_associations_unchanged"
    }
    "Rollback" {
        # 첫 파일 이동 뒤 작업자 파일 이동 실패도 복원되는지 확인함.
        foreach ($name in @("Zipelo.exe", "zipelo-archive-worker.exe", "install.ini")) {
            $stream = [IO.File]::Open((Join-Path $root $name), [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
            try { $code = RunInstaller } finally { $stream.Dispose() }
            if ($code -ne 1) { throw "잠긴 $name 업데이트의 종료 코드가 1이 아님: $code" }
            AssertEqual $before (Snapshot) "잠긴 $name 실패 뒤 기존 설치가 바뀜"
            "rollback_passed: $name"
        }
        $before | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 $Evidence
    }
    "Upgrade" {
        $code = RunInstaller
        if ($code -ne 0) { throw "업데이트 실패: $code" }
        $after = Snapshot
        # 정상 업데이트는 DisplayVersion만 바꾸므로 따로 확인하고 나머지 등록을 비교함.
        $versionKey = @($after.registry.Keys | Where-Object { $_ -match '\\Uninstall\\Zipelo\\DisplayVersion$' })
        if ($versionKey.Count -ne 1) { throw "DisplayVersion 등록을 찾지 못함" }
        $newVersion = $after.registry[$versionKey[0]]
        if ($ExpectedVersion -and $newVersion -cne $ExpectedVersion) {
            throw "업데이트 후 DisplayVersion이 $ExpectedVersion 이 아님: $newVersion"
        }
        if ($ExpectedVersion -and $before.registry[$versionKey[0]] -ceq $ExpectedVersion) {
            throw "업데이트 전부터 DisplayVersion이 $ExpectedVersion 이라 교체를 확인할 수 없음"
        }
        # 증거에는 버전을 남기고 비교용 복사본에서만 제외함.
        $beforeRegistry = [ordered]@{}
        foreach ($key in $before.registry.Keys) { if ($key -ne $versionKey[0]) { $beforeRegistry[$key] = $before.registry[$key] } }
        $afterRegistry = [ordered]@{}
        foreach ($key in $after.registry.Keys) { if ($key -ne $versionKey[0]) { $afterRegistry[$key] = $after.registry[$key] } }
        AssertEqual $beforeRegistry $afterRegistry "업데이트가 기존 연결 또는 등록을 바꿈"
        AssertEqual $before.shortcut $after.shortcut "업데이트가 기존 바로가기를 바꿈"
        if ($ExpectedVersion) {
            # 버전만 바뀌고 파일이 그대로인 경우를 막기 위해 설치된 파일을 빌드 payload와 대조함.
            if (-not $Payload) { throw "-ExpectedVersion에는 빌드 출력의 payload 폴더(-Payload)가 필요함" }
            foreach ($name in @("Zipelo.exe", "zipelo-archive-worker.exe", "LICENSE", "THIRD-PARTY-LICENSES.txt", "BUILD-INFO.json")) {
                $expected = (Get-FileHash (Join-Path $Payload $name) -Algorithm SHA256).Hash
                if ($after.files[$name] -ne $expected) { throw "설치된 $name 이 payload와 다름" }
            }
        } elseif ($before.files['Uninstall.exe'] -eq $after.files['Uninstall.exe']) {
            throw "새 제거 프로그램으로 교체되지 않음"
        }
        $after | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 $Evidence
        "upgrade_passed: version $($newVersion), files match payload or new uninstaller, preserved associations"
    }
}

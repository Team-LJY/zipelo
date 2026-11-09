param(
    [Parameter(Mandatory = $true)][string]$Bundle,
    [Parameter(Mandatory = $true)][ValidatePattern('^[a-f0-9]{64}$')][string]$PlanSha256,
    [Parameter(Mandatory = $true)][string]$Dotnet,
    [Parameter(Mandatory = $true)][string]$WorkParent,
    [Parameter(Mandatory = $true)][string]$PackageSource,
    [switch]$Validation
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (-not $Validation) { throw 'Explicit validation switch required.' }

function Require-PlainPath([string]$Path, [bool]$Directory) {
    if (-not [IO.Path]::IsPathRooted($Path) -or $Path -match '[;,%$\r\n]') { throw 'Absolute plain path required.' }
    $item = Get-Item -LiteralPath $Path
    if ($item.PSIsContainer -ne $Directory) { throw 'Path kind mismatch.' }
    while ($null -ne $item) {
        if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Reparse path rejected.' }
        $parent = Split-Path -Parent $item.FullName
        if (-not $parent -or $parent -eq $item.FullName) { break }
        $item = Get-Item -LiteralPath $parent
    }
    return [IO.Path]::GetFullPath($Path).TrimEnd('\')
}
function Hash([string]$Path) { return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
function Under([string]$Parent, [string]$Path) {
    return $Path.Equals($Parent, [StringComparison]::OrdinalIgnoreCase) -or $Path.StartsWith($Parent + '\', [StringComparison]::OrdinalIgnoreCase)
}
$Bundle = Require-PlainPath $Bundle $true
$Dotnet = Require-PlainPath $Dotnet $false
$WorkParent = Require-PlainPath $WorkParent $true
$PackageSource = Require-PlainPath $PackageSource $true
foreach ($protected in @($Bundle, (Split-Path -Parent $Dotnet), $PackageSource)) {
    if (Under $protected $WorkParent) { throw 'Build parent overlaps an input.' }
}
$planFile = Join-Path $Bundle 'plan.json'
if ((Hash $planFile) -ne $PlanSha256) { throw 'Plan SHA-256 mismatch.' }
$plan = Get-Content -LiteralPath $planFile -Raw -Encoding UTF8 | ConvertFrom-Json
if ($plan.schema -ne 2 -or $plan.version -ne '10.0.14' -or $plan.rid -notin @('win-x64', 'linux-x64') -or $plan.planId -notmatch '^[a-f0-9]{32}$') { throw 'Plan contract mismatch.' }
function Check-Inputs {
    if ((Hash $planFile) -ne $PlanSha256) { throw 'Plan changed.' }
    foreach ($entry in $plan.files.PSObject.Properties) {
        $name = $entry.Name
        if ($name -match '(^/|\\|:|(^|/)\.\.?(/|$))') { throw 'Unsafe manifest path.' }
        $file = Join-Path $Bundle $name.Replace('/', '\')
        $file = Require-PlainPath $file $false
        if (-not (Under $Bundle $file)) { throw 'Input escaped bundle.' }
        if ((Get-Item -LiteralPath $file).Length -ne $entry.Value.bytes -or (Hash $file) -ne $entry.Value.sha256) { throw "Input mismatch: $name" }
    }
}
Check-Inputs
$sdkVersion = (& $Dotnet --version).Trim()
if ($LASTEXITCODE -ne 0 -or $sdkVersion -notmatch '^10\.[0-9]+\.[0-9]+$') { throw '.NET 10 SDK required.' }
$work = Join-Path $WorkParent ('zipelo-plan-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work -ErrorAction Stop | Out-Null
$source = Join-Path $work 'source'
New-Item -ItemType Directory -Path $source | Out-Null
foreach ($entry in $plan.files.PSObject.Properties) {
    if ($entry.Name.StartsWith('source/')) {
        $target = Join-Path $work $entry.Name.Replace('/', '\')
        New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $Bundle $entry.Name.Replace('/', '\')) -Destination $target
        if ((Hash $target) -ne $entry.Value.sha256) { throw 'Source copy mismatch.' }
    }
}
$config = Join-Path $work 'NuGet.Config'
$escapedSource = [Security.SecurityElement]::Escape($PackageSource)
$configuration = '<configuration><packageSources><clear/><add key="offline" value="' + $escapedSource + '"/></packageSources><fallbackPackageFolders><clear/></fallbackPackageFolders><auditSources><clear/></auditSources></configuration>'
[IO.File]::WriteAllText($config, $configuration)
$results = Join-Path $work 'results'
New-Item -ItemType Directory -Path $results | Out-Null
$app = if ($plan.rid -eq 'win-x64') { 'Zipelo.Desktop' } else { 'Zipelo.Ubuntu' }
$apphost = Join-Path $Bundle ('apphost/apphost' + $(if ($plan.rid -eq 'win-x64') { '.exe' } else { '' }))
$properties = @('-p:ZipeloSharedRuntimeValidation=true', '-p:ZipeloSharedRuntimeAssembler=true',
    '-p:SelfContained=false', '-p:PublishSelfContained=false', '-p:UseAppHost=true',
    '-p:PublishSingleFile=false', '-p:PublishTrimmed=false', '-p:PublishAot=false',
    '-p:RuntimeFrameworkVersion=10.0.14', '-p:RollForward=Disable', '-p:TargetLatestRuntimePatch=false',
    '-p:EnableAppHostPackDownload=false', '-p:EnableTargetingPackDownload=false',
    '-p:AppHostDotNetSearch=AppRelative', '-p:ZipeloCreationValidation=false',
    ('-p:DirectoryBuildTargetsPath=' + (Join-Path $Bundle 'Plan.targets')),
    ('-p:AppHostSourcePath=' + $apphost), ('-p:ZipeloPublishPlanHash=' + $PlanSha256))
foreach ($kind in @('ui', 'worker')) {
    $projectName = if ($kind -eq 'ui') { $app } else { 'Zipelo.Worker' }
    $project = Join-Path $source ('src/' + $projectName + '/' + $projectName + '.csproj')
    $extraction = if ($kind -eq 'ui' -and $plan.extractionValidation) { 'true' } else { 'false' }
    $common = @('-r', $plan.rid, '-p:Configuration=Release', ('-p:ZipeloExtractionValidation=' + $extraction)) + $properties
    & $Dotnet restore $project --configfile $config @common
    if ($LASTEXITCODE -ne 0) { throw "Offline restore failed: $kind" }
    $arguments = @('publish', $project, '--no-restore', '-c', 'Release', '-o', (Join-Path $results $kind)) + $common
    if ($kind -eq 'ui') { $arguments += '-p:PublishProfile=SharedRuntimeValidation' }
    & $Dotnet @arguments
    if ($LASTEXITCODE -ne 0) { throw "Publish failed: $kind" }
}
Check-Inputs
foreach ($entry in $plan.files.PSObject.Properties) {
    if ($entry.Name.StartsWith('source/') -and (Hash (Join-Path $work $entry.Name.Replace('/', '\'))) -ne $entry.Value.sha256) { throw 'Build source changed.' }
}
$files = [ordered]@{}
foreach ($file in Get-ChildItem -LiteralPath $results -Recurse -File) {
    $plain = Require-PlainPath $file.FullName $false
    $relative = $plain.Substring($results.Length + 1).Replace('\', '/')
    $files[$relative] = @{ sha256 = (Hash $plain); bytes = $file.Length }
}
$receipt = @{ schema = 2; planSha256 = $PlanSha256; planId = $plan.planId; rid = $plan.rid;
    version = $plan.version; completed = $true; sdkVersion = $sdkVersion; files = $files }
$receiptFile = Join-Path $results 'publish-receipt.json'
[IO.File]::WriteAllText($receiptFile, ($receipt | ConvertTo-Json -Depth 12), (New-Object Text.UTF8Encoding($false)))
Write-Output ('Results: ' + $results)
Write-Output ('Plan SHA-256: ' + $PlanSha256)

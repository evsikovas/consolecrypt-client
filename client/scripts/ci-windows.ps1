$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
# A Windows service has a different PATH from an interactive desktop. Recover
# installed build tools without changing the service account or machine PATH.
function Add-BuildPath([string]$Directory) {
    if ($Directory -and (Test-Path $Directory)) { $env:Path = $Directory + ';' + $env:Path }
}
Add-BuildPath $env:CC_CI_FLUTTER_ROOT
if ($env:FLUTTER_ROOT) { Add-BuildPath (Join-Path $env:FLUTTER_ROOT 'bin') }
foreach ($path in @('C:\dev\flutter\bin', 'C:\src\flutter\bin', 'C:\tools\flutter\bin', 'C:\flutter\bin', 'C:\Strawberry\perl\bin', 'C:\Strawberry\c\bin')) { Add-BuildPath $path }
$Profiles = @($env:USERPROFILE)
$UsersRoot = Join-Path $env:SystemDrive 'Users'
if (Test-Path $UsersRoot) { $Profiles += @(Get-ChildItem $UsersRoot -Directory | ForEach-Object { $_.FullName }) }
foreach ($buildProfileRoot in ($Profiles | Select-Object -Unique)) {
    if (-not $buildProfileRoot) { continue }
    foreach ($relative in @('flutter\bin', 'dev\flutter\bin', 'tools\flutter\bin', 'AppData\Local\flutter\bin', 'scoop\apps\flutter\current\bin')) { Add-BuildPath (Join-Path $buildProfileRoot $relative) }
    $cargo = Join-Path $buildProfileRoot '.cargo'
    $rustup = Join-Path $buildProfileRoot '.rustup'
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue) -and (Test-Path (Join-Path $cargo 'bin\cargo.exe')) -and (Test-Path $rustup)) {
        $env:CARGO_HOME = $cargo; $env:RUSTUP_HOME = $rustup
        Add-BuildPath (Join-Path $cargo 'bin')
    }
    $pythonBase = Join-Path $buildProfileRoot 'AppData\Local\Programs\Python'
    if (Test-Path $pythonBase) { foreach ($python in (Get-ChildItem $pythonBase -Directory -Filter 'Python3*' | Sort-Object Name -Descending)) { Add-BuildPath $python.FullName } }
    Add-BuildPath (Join-Path $buildProfileRoot 'AppData\Local\Programs\Inno Setup 6')
}
# Bootstrap only the missing Flutter SDK from the official, verified tag.
# This dedicated runner cache is outside the checkout and never enters Git.
if (-not (Get-Command flutter -ErrorAction SilentlyContinue)) {
    $runnerTools = if ($env:CC_CI_TOOLS_DIR) { $env:CC_CI_TOOLS_DIR } else { Join-Path $env:SystemDrive 'GitLab-Runner\tools' }
    $flutterSdk = Join-Path $runnerTools 'flutter-3.47.5'
    if (-not (Test-Path (Join-Path $flutterSdk 'bin\flutter.bat'))) {
        New-Item -ItemType Directory -Force $runnerTools | Out-Null
        & git clone --depth 1 --branch 3.47.5 https://github.com/flutter/flutter.git $flutterSdk
        if ($LASTEXITCODE -ne 0) { throw 'Official Flutter SDK checkout failed' }
    }
    $flutterRevision = & git -C $flutterSdk rev-parse HEAD
    if ($LASTEXITCODE -ne 0 -or "$flutterRevision".Trim() -ne '6a19cca56475dbfba1478ee68d7bd0c2ef891da1') { throw 'Flutter SDK revision does not match the verified release' }
    Add-BuildPath (Join-Path $flutterSdk 'bin')
}
$missing = @()
foreach ($tool in @('flutter', 'cargo', 'rustup', 'perl', 'python')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { $missing += $tool }
}
if ($missing.Count) { throw "Missing runner build tools: $($missing -join ', '). See docs/public/BUILD_WINDOWS.md." }
function Invoke-Checked {
    param([string]$Command, [string[]]$Arguments)
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Command failed (exit $LASTEXITCODE)" }
}
Push-Location (Join-Path $Repo 'client\flutter')
try {
    Invoke-Checked flutter @('pub', 'get')
    Invoke-Checked flutter @('analyze')
    Invoke-Checked flutter @('test')
} finally { Pop-Location }
Push-Location $Repo
try {
    Invoke-Checked python @('-m', 'unittest', 'discover', '-s', 'client/scripts', '-p', 'test_bump_version.py', '-v')
    if ($env:CI_JOB_ID) { $env:CC_BUILD_NUMBER = $env:CI_JOB_ID }
    & (Join-Path $PSScriptRoot 'build-windows.ps1') -Installer -NoCli
    if ($LASTEXITCODE -ne 0) { throw 'Windows packaging failed' }
} finally { Pop-Location }

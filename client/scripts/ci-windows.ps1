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
foreach ($profile in ($Profiles | Select-Object -Unique)) {
    if (-not $profile) { continue }
    foreach ($relative in @('flutter\bin', 'dev\flutter\bin', 'tools\flutter\bin', 'AppData\Local\flutter\bin')) { Add-BuildPath (Join-Path $profile $relative) }
    $cargo = Join-Path $profile '.cargo'
    $rustup = Join-Path $profile '.rustup'
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue) -and (Test-Path (Join-Path $cargo 'bin\cargo.exe')) -and (Test-Path $rustup)) {
        $env:CARGO_HOME = $cargo; $env:RUSTUP_HOME = $rustup
        Add-BuildPath (Join-Path $cargo 'bin')
    }
    $pythonBase = Join-Path $profile 'AppData\Local\Programs\Python'
    if (Test-Path $pythonBase) { foreach ($python in (Get-ChildItem $pythonBase -Directory -Filter 'Python3*' | Sort-Object Name -Descending)) { Add-BuildPath $python.FullName } }
    Add-BuildPath (Join-Path $profile 'AppData\Local\Programs\Inno Setup 6')
}
foreach ($tool in @('flutter', 'cargo', 'rustup', 'perl', 'python')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "Build tool '$tool' is missing in the runner service account. Install prerequisites from docs/public/BUILD_WINDOWS.md or set CC_CI_FLUTTER_ROOT to Flutter's bin directory." }
}
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

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
# A Windows service has a different PATH from an interactive desktop. Recover
# installed build tools without changing the service account or machine PATH.
function Add-BuildPath([string]$Directory) {
    if ($Directory -and (Test-Path $Directory)) { $env:Path = $Directory + ';' + $env:Path }
}
if ($env:CC_CI_FLUTTER_ROOT) { Add-BuildPath (Join-Path $env:CC_CI_FLUTTER_ROOT 'bin') }
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
# Bootstrap missing SDKs from official releases with pinned verification.
# This dedicated runner cache is outside the checkout and never enters Git.
$runnerTools = if ($env:CC_CI_TOOLS_DIR) { $env:CC_CI_TOOLS_DIR } else { Join-Path $env:SystemDrive 'GitLab-Runner\tools' }
if (-not (Get-Command flutter -ErrorAction SilentlyContinue)) {
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
if (-not (Get-Command perl -ErrorAction SilentlyContinue)) {
    $perlSdk = Join-Path $runnerTools 'perl-5.40.5.1'
    if (-not (Test-Path (Join-Path $perlSdk 'perl\bin\perl.exe'))) {
        New-Item -ItemType Directory -Force $runnerTools | Out-Null
        $perlArchive = Join-Path $runnerTools 'strawberry-perl-5.40.5.1-64bit-portable.zip'
        Invoke-WebRequest -UseBasicParsing -Uri 'https://github.com/StrawberryPerl/Perl-Dist-Strawberry/releases/download/SP_54051_64bit/strawberry-perl-5.40.5.1-64bit-portable.zip' -OutFile $perlArchive
        if ((Get-FileHash $perlArchive -Algorithm SHA256).Hash.ToLowerInvariant() -ne '6619fe7eeef921ccddb4aac3972fb602a0c690a3074205b863ade998d7bc79a6') { throw 'Strawberry Perl archive checksum mismatch' }
        Expand-Archive -Path $perlArchive -DestinationPath $perlSdk -Force
        Remove-Item $perlArchive
    }
    Add-BuildPath (Join-Path $perlSdk 'c\bin')
    Add-BuildPath (Join-Path $perlSdk 'perl\bin')
}
foreach ($innoBase in @(${env:ProgramFiles(x86)}, $env:ProgramFiles)) {
    if ($innoBase) { Add-BuildPath (Join-Path $innoBase 'Inno Setup 6') }
}
if (-not (Get-Command ISCC.exe -ErrorAction SilentlyContinue)) {
    $innoSdk = Join-Path $runnerTools 'inno-6.7.3'
    if (-not (Test-Path (Join-Path $innoSdk 'ISCC.exe'))) {
        New-Item -ItemType Directory -Force $runnerTools | Out-Null
        $innoInstaller = Join-Path $runnerTools 'innosetup-6.7.3.exe'
        Invoke-WebRequest -UseBasicParsing -Uri 'https://github.com/jrsoftware/issrc/releases/download/is-6_7_3/innosetup-6.7.3.exe' -OutFile $innoInstaller
        if ((Get-FileHash $innoInstaller -Algorithm SHA256).Hash.ToLowerInvariant() -ne '9c73c3bae7ed48d44112a0f48e66742c00090bdb5bef71d9d3c056c66e97b732') { throw 'Inno Setup installer checksum mismatch' }
        $innoInstall = Start-Process -FilePath $innoInstaller -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CURRENTUSER', ('/DIR="' + $innoSdk + '"')) -Wait -PassThru
        if ($innoInstall.ExitCode -ne 0 -or -not (Test-Path (Join-Path $innoSdk 'ISCC.exe'))) { throw 'Inno Setup installation failed' }
        Remove-Item $innoInstaller
    }
    Add-BuildPath $innoSdk
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
# Exercise the Windows filesystem and native RDP input/clipboard code on the
# actual runner OS before packaging. Live server tests remain opt-in and use no
# runner credentials during this unit/integration pass.
$OriginalNativeTestTarget = $env:CARGO_TARGET_DIR
try {
    $env:CARGO_TARGET_DIR = Join-Path $Repo 'target\windows-rdp-tests'
    Invoke-Checked cargo @('test', '--manifest-path', (Join-Path $Repo 'client\rust\Cargo.toml'), '--locked', '-p', 'cc-rdp-core', '--all-targets')
} finally { $env:CARGO_TARGET_DIR = $OriginalNativeTestTarget }
Push-Location $Repo
try {
    Invoke-Checked python @('-m', 'unittest', 'discover', '-s', 'client/scripts', '-p', 'test_bump_version.py', '-v')
    Invoke-Checked python @('-m', 'unittest', 'discover', '-s', 'client/scripts', '-p', 'test_release_identity.py', '-v')
    Invoke-Checked python @('-m', 'unittest', 'discover', '-s', 'client/scripts', '-p', 'test_release_ci.py', '-v')
    Invoke-Checked python @('client/scripts/verify-release-identity.py')
    if ($env:CI_JOB_ID) { $env:CC_BUILD_NUMBER = $env:CI_JOB_ID }
    & (Join-Path $PSScriptRoot 'build-windows.ps1') -Installer -NoCli
    if ($LASTEXITCODE -ne 0) { throw 'Windows packaging failed' }
} finally { Pop-Location }

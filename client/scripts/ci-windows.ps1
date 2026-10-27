param([switch]$NativeOnly)
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
if (-not $NativeOnly -and -not (Get-Command flutter -ErrorAction SilentlyContinue)) {
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
if (-not $NativeOnly -and -not (Get-Command ISCC.exe -ErrorAction SilentlyContinue)) {
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
$RequiredTools = @('cargo', 'rustup', 'perl', 'python')
if (-not $NativeOnly) { $RequiredTools += 'flutter' }
foreach ($tool in $RequiredTools) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { $missing += $tool }
}
if ($missing.Count) { throw "Missing runner build tools: $($missing -join ', '). See docs/public/BUILD_WINDOWS.md." }
function Invoke-Checked {
    param([string]$Command, [string[]]$Arguments)
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Command failed (exit $LASTEXITCODE)" }
}
Invoke-Checked python @('-m', 'unittest', 'discover', '-s', (Join-Path $Repo 'client\scripts'), '-p', 'test_ci_cleanup.py', '-v')
# rustc and the MSVC linker need ~3 GB each for the larger test binaries;
# one job per logical CPU exhausted the commit limit (page file) on the
# 24-thread runner. Bound the parallelism by memory unless set explicitly.
# Since 0.9.3 the app-core tests link against 100+ MB rlibs (scylla,
# kube); at 3 GB per job rustc could not map some of them ("required to be
# available in rlib format"), so each job now gets 5 GB.
if (-not $env:CARGO_BUILD_JOBS) {
    $memoryGb = (Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB
    $env:CARGO_BUILD_JOBS = [string][math]::Max(2, [math]::Min([Environment]::ProcessorCount, [math]::Floor($memoryGb / 5)))
    Write-Host "Cargo jobs: $env:CARGO_BUILD_JOBS"
}
# Resolve Python/toolchain paths before reserving a GitHub build number.
if ($env:GITHUB_RUN_NUMBER -and -not $NativeOnly) {
    $githubBuild = & python (Join-Path $PSScriptRoot 'github-build-number.py') windows --print-only
    if ($LASTEXITCODE -ne 0 -or "$githubBuild" -notmatch '^\d+$') { throw 'GitHub build identity reservation failed' }
    $env:CC_BUILD_NUMBER = "$githubBuild".Trim()
    Write-Host "Reserved Windows build $env:CC_BUILD_NUMBER"
}
# Exercise native RDP, PTY and Kubernetes before the optional model downloads.
# Live server tests remain opt-in and use no runner credentials in this pass.
$OriginalNativeTestTarget = $env:CARGO_TARGET_DIR
try {
    # Tests and `cargo check` use separate target folders, so metadata-only
    # check artifacts never stand next to the rlibs the tests link against.
    $env:CARGO_TARGET_DIR = Join-Path $Repo 'target\windows-native-tests'
    $NativeArguments = @('test', '--manifest-path', (Join-Path $Repo 'client\rust\Cargo.toml'), '--locked', '-p', 'cc-terminal-core', '-p', 'cc-k8s-core', '-p', 'cc-app-core', '--all-targets')
    if (-not $NativeOnly) { $NativeArguments += @('-p', 'cc-rdp-core') }
    # Build the test binaries first; one retry reuses every finished artifact
    # if memory pressure still made rustc miss an rlib.
    try { Invoke-Checked cargo ($NativeArguments + @('--no-run')) }
    catch {
        Write-Host 'Native test build failed once; retrying with the finished artifacts'
        Invoke-Checked cargo ($NativeArguments + @('--no-run'))
    }
    if ($env:CC_KUBE_WSL_LIVE -eq '1') { $NativeArguments += @('--', '--nocapture') }
    Invoke-Checked cargo $NativeArguments
    # Compile the production Flutter boundary, including Windows-only cfg paths.
    $env:CARGO_TARGET_DIR = Join-Path $Repo 'target\windows-bridge-check'
    Invoke-Checked cargo @('check', '--manifest-path', (Join-Path $Repo 'client\rust\Cargo.toml'), '--locked', '-p', 'cc-bridge', '--all-targets')
} finally { $env:CARGO_TARGET_DIR = $OriginalNativeTestTarget }
if ($NativeOnly) {
    Write-Host 'WINDOWS_NATIVE_CHECKS_PASS: no application packaging or publication'
    return
}
Push-Location (Join-Path $Repo 'client\flutter')
try {
    Invoke-Checked flutter @('pub', 'get')
    Invoke-Checked flutter @('analyze')
    Invoke-Checked flutter @('test')
    # Reuse owned model downloads across failed attempts. Only a published
    # release cleans this checkout cache; SDK/toolchain caches stay separate.
    if ($env:GITHUB_ACTIONS -eq 'true') {
        $LocalAiSmokeCache = & python (Join-Path $PSScriptRoot 'cleanup-ci-workspace.py') --workspace $Repo --prepare-ai-cache
        if ($LASTEXITCODE -ne 0) { throw 'Could not prepare an owned AI smoke cache' }
        $LocalAiSmokeCache = "$LocalAiSmokeCache".Trim()
    } else {
        $LocalAiSmokeCache = Join-Path $runnerTools 'consolecrypt-local-ai-smoke'
    }
    foreach ($model in @('qwen2.5-coder-1.5b-instruct-q4_k_m', 'qwen3.5-4b-q4_k_m')) {
        Invoke-Checked dart @('run', 'test/ai/local/local_ai_smoke.dart', '--run', "--directory=$LocalAiSmokeCache", "--model=$model", '--request-timeout-seconds=900')
    }
    $LocalAiSmokeMemory = [UInt64](Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory
    if ($LocalAiSmokeMemory -ge 16GB) {
        Invoke-Checked dart @('run', 'test/ai/local/local_ai_smoke.dart', '--run', "--directory=$LocalAiSmokeCache", '--model=qwen3.5-9b-q4_k_m', '--request-timeout-seconds=900')
    } else {
        Write-Host '9B CPU generation not checked: this runner has less than 16 GiB RAM.'
    }
} finally { Pop-Location }
Push-Location $Repo
try {
    Invoke-Checked python @('-m', 'unittest', 'discover', '-s', 'client/scripts', '-p', 'test_bump_version.py', '-v')
    Invoke-Checked python @('-m', 'unittest', 'discover', '-s', 'client/scripts', '-p', 'test_release_identity.py', '-v')
    Invoke-Checked python @('-m', 'unittest', 'discover', '-s', 'client/scripts', '-p', 'test_release_ci.py', '-v')
    Invoke-Checked python @('client/scripts/verify-release-identity.py')
    if ($env:CI_JOB_ID) { $env:CC_BUILD_NUMBER = $env:CI_JOB_ID }
    & (Join-Path $PSScriptRoot 'build-windows.ps1') -Installer
    if ($LASTEXITCODE -ne 0) { throw 'Windows packaging failed' }
} finally { Pop-Location }

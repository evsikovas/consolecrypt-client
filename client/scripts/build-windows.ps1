# Windows: .\client\scripts\build-windows.ps1 -Installer -NoCli
# Prerequisites: docs/public/BUILD_WINDOWS.md
param([switch]$Debug, [string]$Commit = '', [switch]$Mock, [switch]$NoCli, [switch]$Installer)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Invoke-Checked {
    param([string]$Command, [string[]]$Arguments)
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Command failed (exit $LASTEXITCODE)" }
}
function Find-InnoSetup {
    $command = Get-Command ISCC.exe -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }
    foreach ($base in @(${env:ProgramFiles(x86)}, $env:ProgramFiles, $env:LOCALAPPDATA)) {
        if (-not $base) { continue }
        foreach ($relative in @('Inno Setup 6\ISCC.exe', 'Programs\Inno Setup 6\ISCC.exe')) {
            $candidate = Join-Path $base $relative
            if (Test-Path $candidate) { return $candidate }
        }
    }
    throw 'Inno Setup 6 is required for -Installer: https://jrsoftware.org/isdl.php'
}
function Find-VcRuntime {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) { throw 'Visual Studio C++ tools are missing; see docs/public/BUILD_WINDOWS.md' }
    $vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($LASTEXITCODE -ne 0 -or -not $vs) { throw 'Visual Studio Desktop development with C++ is required' }
    $redist = Join-Path $vs 'VC\Redist\MSVC'
    if (Test-Path $redist) {
        foreach ($version in (Get-ChildItem $redist -Directory | Where-Object { $_.Name -match '^\d+\.\d+\.\d+$' } | Sort-Object { [version]$_.Name } -Descending)) {
            $x64 = Join-Path $version.FullName 'x64'
            if (-not (Test-Path $x64)) { continue }
            foreach ($crt in (Get-ChildItem $x64 -Directory -Filter 'Microsoft.VC*.CRT')) {
                $required = @('msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll')
                if (@($required | Where-Object { -not (Test-Path (Join-Path $crt.FullName $_)) }).Count -eq 0) { return $crt.FullName }
            }
        }
    }
    throw 'Visual C++ x64 runtime DLLs not found. Install current MSVC C++ tools in Visual Studio Installer.'
}
$Repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$Src = $Repo
$TmpWt = $null
$OriginalLocation = Get-Location
$OriginalCargoTarget = $env:CARGO_TARGET_DIR
$OriginalBridgeTarget = $env:CONSOLECRYPT_CARGO_TARGET_DIR
try {
    if ($Debug -and $Installer) { throw 'Installers require a release build; remove -Debug' }
    foreach ($tool in @('flutter', 'cargo', 'rustup', 'perl', 'git', 'python')) {
        if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "Missing $tool; see docs/public/BUILD_WINDOWS.md" }
    }
    $Iscc = if ($Installer) { Find-InnoSetup } else { $null }
    $Runtime = if (-not $Debug) { Find-VcRuntime } else { $null }
    if ($Commit -ne '') {
        $TmpWt = Join-Path $env:TEMP ('consolecrypt-build-' + [guid]::NewGuid().ToString('N'))
        Invoke-Checked git @('-C', $Repo, 'worktree', 'add', '--detach', $TmpWt, $Commit)
        $Src = $TmpWt
    }
    if (Test-Path (Join-Path $Src 'client\rust\rdp-core')) {
        Invoke-Checked python @((Join-Path $Src 'client\scripts\verify-release-identity.py'), '--root', $Src)
    }
    $Mode = if ($Debug) { 'debug' } else { 'release' }
    # OpenSSL adds long source paths below Cargo's target directory. Keep it
    # out of Flutter's deeply nested build/windows/.../plugins directory.
    $env:CONSOLECRYPT_CARGO_TARGET_DIR = Join-Path $Src 'target\windows-rust'
    $Out = Join-Path $Repo 'dist\windows'
    New-Item -ItemType Directory -Force -Path $Out | Out-Null
    $BuildVersion = & python (Join-Path $Repo 'client\scripts\bump-version.py') --root $Repo --source-root $Src
    if ($LASTEXITCODE -ne 0) { throw 'Version bump failed' }
    $BuildVersion = "$BuildVersion".Trim()
    if ($BuildVersion -notmatch '^(\d+\.\d+\.\d+)\+(\d+)$') { throw 'Invalid build version' }
    $FileVersion = "$($Matches[1]).$($Matches[2])"
    Write-Host "==> ConsoleCrypt $BuildVersion ($Mode, Windows x64)"
    Set-Location (Join-Path $Src 'client\flutter')
    Invoke-Checked flutter @('pub', 'get')
    $BuildArgs = @('build', 'windows', "--$Mode")
    if ($Mock) { $BuildArgs += '--dart-define=CC_MOCK=true' }
    Invoke-Checked flutter $BuildArgs
    $Configuration = if ($Debug) { 'Debug' } else { 'Release' }
    $Built = Join-Path (Get-Location) "build\windows\x64\runner\$Configuration"
    $AppOut = Join-Path $Out 'ConsoleCrypt'
    if (Test-Path $AppOut) { Remove-Item -Recurse -Force $AppOut }
    Copy-Item -Recurse $Built $AppOut
    if ($Runtime) { Copy-Item (Join-Path $Runtime '*.dll') $AppOut -Force }
    foreach ($required in @('ConsoleCrypt.exe', 'flutter_windows.dll', 'cc_bridge.dll', 'data\icudtl.dat')) {
        if (-not (Test-Path (Join-Path $AppOut $required))) { throw "Incomplete Flutter bundle: $required" }
    }
    if (Test-Path (Join-Path $Src 'LICENSE')) {
        Copy-Item (Join-Path $Src 'LICENSE') $AppOut -Force
    } else {
        # Historical commits retain the licenses originally published with them.
        Copy-Item (Join-Path $Src 'LICENSE-MIT'), (Join-Path $Src 'LICENSE-APACHE') $AppOut -Force
    }
    if (Test-Path (Join-Path $Src 'client\rust\rdp-core')) {
        Copy-Item (Join-Path $Src 'client\rust\rdp-core\THIRD_PARTY_NOTICES.txt') (Join-Path $AppOut 'RDP-THIRD-PARTY-NOTICES.txt')
    }
    if (-not $NoCli) {
        Set-Location (Join-Path $Src 'client\rust')
        $env:CARGO_TARGET_DIR = Join-Path $Src 'client\rust\target\dist'
        Invoke-Checked cargo @('build', '--release', '--locked', '-p', 'cc-cli')
        Copy-Item (Join-Path $env:CARGO_TARGET_DIR 'release\consolecrypt.exe') (Join-Path $Out 'consolecrypt-cli.exe') -Force
    }
    $Zip = Join-Path $Out 'ConsoleCrypt-windows.zip'
    if (Test-Path $Zip) { Remove-Item -Force $Zip }
    Compress-Archive -Path (Join-Path $AppOut '*') -DestinationPath $Zip
    $Artifacts = @($Zip)
    if ($Installer) {
        $SetupName = "ConsoleCrypt-$BuildVersion-windows-x64-setup"
        Invoke-Checked $Iscc @("/DAppVersion=$BuildVersion", "/DFileVersion=$FileVersion", "/DAppSource=$AppOut", "/DRepoRoot=$Src", "/DOutputDir=$Out", "/DOutputName=$SetupName", (Join-Path $Src 'client\packaging\windows\ConsoleCrypt.iss'))
        $Setup = Join-Path $Out "$SetupName.exe"
        if (-not (Test-Path $Setup)) { throw 'Inno Setup produced no installer' }
        $Artifacts += $Setup
    }
    foreach ($file in $Artifacts) {
        $hash = (Get-FileHash -Algorithm SHA256 $file).Hash.ToLowerInvariant()
        Set-Content -Path "$file.sha256" -Value "$hash  $([IO.Path]::GetFileName($file))" -Encoding ascii
    }
    Set-Content -Path (Join-Path $Out 'ConsoleCrypt.version') -Value $BuildVersion -Encoding ascii
    Write-Host "Done -> $Out"
    Write-Host 'Keep the complete app folder together. Public installers need Windows code signing.'
}
finally {
    Set-Location $OriginalLocation
    $env:CARGO_TARGET_DIR = $OriginalCargoTarget
    $env:CONSOLECRYPT_CARGO_TARGET_DIR = $OriginalBridgeTarget
    if ($TmpWt -and (Test-Path $TmpWt)) { & git -C $Repo worktree remove --force $TmpWt }
}

# Compile the production installer with a disposable placeholder, never the app.
# Run before native compilation to catch PE version failures within seconds.
param([Parameter(Mandatory = $true)][string]$Iscc)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$TempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$Scratch = Join-Path $TempRoot ('consolecrypt-installer-smoke-' + [guid]::NewGuid().ToString('N'))
try {
    $Payload = Join-Path $Scratch 'placeholder'
    $Output = Join-Path $Scratch 'output'
    New-Item -ItemType Directory -Force $Payload, $Output | Out-Null
    [IO.File]::WriteAllText((Join-Path $Payload 'ConsoleCrypt.exe'), 'Installer metadata test fixture; not an application.')
    $Versions = @('1.2.3+65535', '1.2.3+65536', '1.2.3+66012')
    if ($env:CC_BUILD_NUMBER) {
        $Pubspec = Get-Content (Join-Path $Repo 'client\flutter\pubspec.yaml') -Raw
        if ($Pubspec -notmatch '(?m)^version:\s*(\d+\.\d+\.\d+)\+') { throw 'Missing source release version' }
        $Versions += "$($Matches[1])+$env:CC_BUILD_NUMBER"
    }
    foreach ($Version in $Versions) {
        $Numeric = & python (Join-Path $PSScriptRoot 'windows-file-version.py') $Version
        if ($LASTEXITCODE -ne 0) { throw 'Windows version mapping failed' }
        $Numeric = "$Numeric".Trim()
        & $Iscc /Qp "/DAppVersion=$Version" "/DFileVersion=$Numeric" "/DAppSource=$Payload" "/DRepoRoot=$Repo" "/DOutputDir=$Output" /DOutputName=metadata-smoke (Join-Path $Repo 'client\packaging\windows\ConsoleCrypt.iss')
        if ($LASTEXITCODE -ne 0) { throw "Installer metadata smoke failed: $Version" }
        $Info = [Diagnostics.FileVersionInfo]::GetVersionInfo((Join-Path $Output 'metadata-smoke.exe'))
        $Actual = "$($Info.FileMajorPart).$($Info.FileMinorPart).$($Info.FileBuildPart).$($Info.FilePrivatePart)"
        # Inno pads the textual resource values with spaces.
        if ($Actual -ne $Numeric -or $Info.FileVersion.Trim() -ne $Version -or $Info.ProductVersion.Trim() -ne $Version) {
            throw "Installer lost its numeric or full text identity: $Version"
        }
        Write-Host "WINDOWS_INSTALLER_METADATA_PASS: $Version ($Numeric)"
    }
} finally {
    $ResolvedScratch = [IO.Path]::GetFullPath($Scratch)
    if ([IO.Path]::GetDirectoryName($ResolvedScratch) -ne $TempRoot.TrimEnd('\') -or
        [IO.Path]::GetFileName($ResolvedScratch) -notmatch '^consolecrypt-installer-smoke-[a-f0-9]{32}$') {
        throw 'Unexpected installer smoke directory'
    }
    if (Test-Path -LiteralPath $ResolvedScratch) { Remove-Item -LiteralPath $ResolvedScratch -Recurse -Force }
}

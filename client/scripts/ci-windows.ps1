$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
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

param([switch]$Maintenance, [switch]$LegacyWindowsAiCache)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
# Discover an existing Python; do not provision tools or modify machine PATH.
$Python = Get-Command python.exe -ErrorAction SilentlyContinue |
    Where-Object { $_.Source -notmatch '\\Microsoft\\WindowsApps\\' } |
    Select-Object -First 1 -ExpandProperty Source
if (-not $Python) {
    $Users = Join-Path $env:SystemDrive 'Users'
    if (Test-Path $Users) {
        foreach ($UserDirectory in (Get-ChildItem $Users -Directory)) {
            $Base = Join-Path $UserDirectory.FullName 'AppData\Local\Programs\Python'
            if (Test-Path $Base) {
                foreach ($Install in (Get-ChildItem $Base -Directory -Filter 'Python3*' | Sort-Object Name -Descending)) {
                    $Candidate = Join-Path $Install.FullName 'python.exe'
                    if (Test-Path $Candidate) { $Python = $Candidate; break }
                }
            }
            if ($Python) { break }
        }
    }
}
if (-not $Python) { throw 'Existing Python 3 is required for guarded CI cleanup' }
$Arguments = @((Join-Path $PSScriptRoot 'cleanup-ci-workspace.py'), '--workspace', $env:GITHUB_WORKSPACE)
if ($Maintenance) { $Arguments += '--maintenance' }
if ($LegacyWindowsAiCache) { $Arguments += '--legacy-windows-ai-cache' }
& $Python @Arguments
if ($LASTEXITCODE -ne 0) { throw 'Guarded CI cleanup failed' }

# Fixed read-only query. Native chooses the OS System32 PowerShell executable,
# supplies the ACTUAL TokenElevation observation and clears inherited env.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)

function UnknownFeature([string]$reason) {
    return @{ state = 'unknown'; reason = $reason }
}
function ReadFeature([string]$name) {
    if ($env:CC_PREREQUISITE_ELEVATION -eq 'notElevated') {
        return (UnknownFeature 'requiresElevation')
    }
    if ($env:CC_PREREQUISITE_ELEVATION -ne 'elevated') {
        return (UnknownFeature 'unavailable')
    }
    try {
        if (-not $script:dismLoaded) { return (UnknownFeature 'unavailable') }
        $feature = Dism\Get-WindowsOptionalFeature -Online -FeatureName $name -ErrorAction Stop
        switch ([string]$feature.State) {
            'Enabled' { return @{ state = 'enabled'; reason = $null } }
            'Disabled' { return @{ state = 'disabled'; reason = $null } }
            'DisabledWithPayloadRemoved' { return @{ state = 'disabled'; reason = $null } }
            'EnablePending' { return @{ state = 'enablePending'; reason = $null } }
            'DisablePending' { return @{ state = 'disablePending'; reason = $null } }
            default { return (UnknownFeature 'unavailable') }
        }
    } catch { return (UnknownFeature 'unavailable') }
}
$script:dismLoaded = $false
if ($env:CC_PREREQUISITE_ELEVATION -eq 'elevated') {
    try {
        # Absolute module from the SAME OS system directory selected by Native.
        Import-Module -Name ($PSHOME + '\Modules\Dism\Dism.psd1') -ErrorAction Stop
        $script:dismLoaded = $true
    } catch { }
}
$features = @{
    subsystemLinux = (ReadFeature 'Microsoft-Windows-Subsystem-Linux')
    virtualMachinePlatform = (ReadFeature 'VirtualMachinePlatform')
}
$indicators = [System.Collections.Generic.List[string]]::new()
$allRead = $true
$base = $null
try {
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey(
        [Microsoft.Win32.RegistryHive]::LocalMachine,
        [Microsoft.Win32.RegistryView]::Registry64)
    foreach ($entry in @(
        @('componentServicing', 'SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending'),
        @('windowsUpdate', 'SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired')
    )) {
        $key = $null
        try {
            $key = $base.OpenSubKey($entry[1], $false)
            if ($null -ne $key) { $indicators.Add($entry[0]) }
        } catch { $allRead = $false }
        finally { if ($null -ne $key) { $key.Dispose() } }
    }
    $key = $null
    try {
        $key = $base.OpenSubKey('SYSTEM\CurrentControlSet\Control\Session Manager', $false)
        if ($null -eq $key) { $allRead = $false }
        else {
            $value = $key.GetValue('PendingFileRenameOperations', $null,
                [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
            if ($null -ne $value) {
                if ($value -isnot [string[]]) { $allRead = $false }
                elseif (@($value | Where-Object { -not [string]::IsNullOrEmpty($_) }).Count -gt 0) {
                    $indicators.Add('pendingFileRename')
                }
            }
        }
    } catch { $allRead = $false }
    finally { if ($null -ne $key) { $key.Dispose() } }
} catch { $allRead = $false }
finally { if ($null -ne $base) { $base.Dispose() } }
if ($indicators.Count -gt 0) {
    $reboot = @{ state = 'indicatorsPresent'; reason = $null; indicators = @($indicators.ToArray()) }
} elseif ($allRead) {
    $reboot = @{ state = 'notObserved'; reason = $null; indicators = @() }
} else {
    $reboot = @{ state = 'unknown'; reason = 'unavailable'; indicators = @() }
}
# Fixed enum projections only. No registry values, stderr or feature names.
@{ version = 1; features = $features; reboot = $reboot } |
    ConvertTo-Json -Depth 6 -Compress | Microsoft.PowerShell.Utility\Write-Output

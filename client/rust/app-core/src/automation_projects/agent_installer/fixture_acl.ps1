$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSHOME 'Modules\Microsoft.PowerShell.Security\Microsoft.PowerShell.Security.psd1') -ErrorAction Stop
$temp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
$path = (Resolve-Path -LiteralPath $env:CC_AGENT_INSTALLER_TEST_ROOT).ProviderPath
$item = Get-Item -LiteralPath $path -Force
if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
    -not [String]::Equals($item.Parent.FullName, $temp, [StringComparison]::OrdinalIgnoreCase) -or
    -not $item.Name.StartsWith('owned-publication-agent-installer-', [StringComparison]::Ordinal) -or
    @(Get-ChildItem -LiteralPath $path -Force).Count -ne 0) { throw 'Only fresh empty owned temporary fixture root is allowed' }
$current = [Security.Principal.WindowsIdentity]::GetCurrent().User
$fixed = [Security.AccessControl.DirectorySecurity]::new()
$fixed.SetOwner($current)
$fixed.SetAccessRuleProtection($true, $false)
foreach ($principal in @($current, [Security.Principal.SecurityIdentifier]::new('S-1-5-18'), [Security.Principal.SecurityIdentifier]::new('S-1-5-32-544'))) {
    $rule = [Security.AccessControl.FileSystemAccessRule]::new($principal, [Security.AccessControl.FileSystemRights]::FullControl,
        ([Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [Security.AccessControl.InheritanceFlags]::ObjectInherit),
        [Security.AccessControl.PropagationFlags]::None, [Security.AccessControl.AccessControlType]::Allow)
    $fixed.AddAccessRule($rule)
}
# Only this new EMPTY test root is provisioned before AppCore/profile creation.
# Production still observes existing profile ACLs and never repairs them.
[IO.Directory]::SetAccessControl($path, $fixed)
$after = Microsoft.PowerShell.Security\Get-Acl -LiteralPath $path
$raw = [Security.AccessControl.RawSecurityDescriptor]::new($after.GetSecurityDescriptorBinaryForm(), 0)
if (-not $after.AreAccessRulesProtected -or $raw.Owner.Value -ne $current.Value -or $raw.DiscretionaryAcl.Count -ne 3) { throw 'Owned fixture descriptor mismatch' }
$seen = @{}
foreach ($ace in $raw.DiscretionaryAcl) {
    if (-not ($ace -is [Security.AccessControl.CommonAce]) -or [int]$ace.AceType -ne 0 -or [int]$ace.AceFlags -ne 3 -or $ace.AccessMask -ne 2032127) { throw 'Owned fixture ACE mismatch' }
    $sid = $ace.SecurityIdentifier.Value
    if (($sid -ne $current.Value -and $sid -ne 'S-1-5-18' -and $sid -ne 'S-1-5-32-544') -or $seen.ContainsKey($sid)) { throw 'Owned fixture principal mismatch' }
    $seen[$sid] = $true
}

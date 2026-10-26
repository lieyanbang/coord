param([Parameter(Mandatory)][string]$BackupRoot)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = [IO.Path]::GetFullPath($BackupRoot)
$parent = [IO.Path]::GetFullPath('C:\ProgramData\coord\backup')
if ([IO.Path]::GetDirectoryName($root) -ne $parent -or [IO.Path]::GetFileName($root) -notmatch '^g7-[0-9]{8}T[0-9]{6}Z-[a-f0-9]{8}$') { throw 'Unexpected exact G7 backup location.' }
if (-not (Test-Path -LiteralPath $root -PathType Container)) { throw 'Precreated administrator-only staging directory required.' }
$acl = Get-Acl -LiteralPath $root
if (-not $acl.AreAccessRulesProtected) { throw 'Backup directory must not inherit broad grants.' }
foreach ($rule in $acl.Access) {
    if ($rule.AccessControlType -eq 'Allow' -and $rule.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value -notin @('S-1-5-18', 'S-1-5-32-544')) { throw 'Backup directory grants an unapproved principal.' }
}
if (Test-Path -LiteralPath (Join-Path $root 'backup-manifest.json')) { throw 'Never overwrite a completed backup.' }
$servy = 'C:\Program Files\Servy\servy-cli.exe'
$python = (Get-Command python.exe -ErrorAction Stop).Source
$helper = Join-Path $root 'sqlite-online-backup.py'
$checker = Join-Path $root 'servy-snapshot-check.exe'
foreach ($file in @($helper, $checker, $servy)) { if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw 'Backup tool missing.' } }
foreach ($directory in @('binary', 'runner', 'relay', 'monitor', 'services', 'config', 'security')) { New-Item -ItemType Directory -Path (Join-Path $root $directory) | Out-Null }
foreach ($name in @('coord', 'CoordRelayAgent', 'CoordActionRunnerG7')) {
    & $servy export --name $name --config json --path (Join-Path $root ('services/' + $name + '.json')) --quiet *> (Join-Path $root ('services/' + $name + '.export.log'))
    if ($LASTEXITCODE -ne 0) { throw ('Servy export failed: ' + $name) }
}
Copy-Item -LiteralPath 'C:\ProgramData\coord\bin\coord.exe' -Destination (Join-Path $root 'binary/coord.exe')
foreach ($name in @('coord-action-runner.exe', 'coord-emote-action.exe')) {
    $source = Join-Path 'C:\ProgramData\coord\g7\bin' $name
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw ('Installed G7 binary missing: ' + $name) }
    Copy-Item -LiteralPath $source -Destination (Join-Path $root ('runner/' + $name))
}
$registry = 'C:\ProgramData\coord\g7\config\registry-production.json'
if (-not (Test-Path -LiteralPath $registry -PathType Leaf)) { throw 'Current production registry missing.' }
Copy-Item -LiteralPath $registry -Destination (Join-Path $root 'config/registry-production.json')
Get-ChildItem -LiteralPath 'C:\ProgramData\coord-relay-agent' -File | Copy-Item -Destination (Join-Path $root 'relay')
Get-ChildItem -LiteralPath 'C:\ProgramData\coord\health' -File | Copy-Item -Destination (Join-Path $root 'monitor')
Export-ScheduledTask -TaskName 'coord-health-monitor' | Set-Content -LiteralPath (Join-Path $root 'monitor/coord-health-monitor.xml') -Encoding utf8
Get-CimInstance Win32_Service | Select-Object Name,State,StartMode,StartName,ProcessId | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $root 'services/baseline.json') -Encoding utf8
$aclPaths = @(
    'C:\ProgramData\coord\g7\config',
    'C:\ProgramData\coord\g7\config\broker.key',
    'C:\ProgramData\coord\g7\daemon-private',
    'C:\ProgramData\coord\g7\daemon-private\management.key',
    'C:\ProgramData\coord\g7\monitor-private',
    'C:\ProgramData\coord\g7\monitor-private\management.key',
    'C:\ProgramData\coord\g7\private',
    'C:\ProgramData\coord\g7\runner-private',
    'C:\ProgramData\coord\g7\broker-public'
)
$aclManifest = foreach ($path in $aclPaths) {
    if (Test-Path -LiteralPath $path) {
        $acl = Get-Acl -LiteralPath $path
        [ordered]@{
            path = $path
            owner = $acl.Owner
            sddl = $acl.Sddl
            access = @($acl.Access | ForEach-Object {
                [ordered]@{ identity = [string]$_.IdentityReference; rights = [string]$_.FileSystemRights; type = [string]$_.AccessControlType; inherited = $_.IsInherited }
            })
        }
    }
}
$aclManifest | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $root 'security/acl-manifest.json') -Encoding utf8

& $python $helper --source 'C:\ProgramData\coord\data\coord.db' --destination (Join-Path $root 'coord.db') *> (Join-Path $root 'coord-backup.json')
if ($LASTEXITCODE -ne 0) { throw 'coord online backup failed.' }
$coordBackup = Get-Content -Raw -LiteralPath (Join-Path $root 'coord-backup.json') | ConvertFrom-Json
if ($coordBackup.quick_check -ne 'ok' -or -not $coordBackup.row_counts_match) { throw 'coord snapshot integrity failed.' }
& $checker --source 'C:\ProgramData\Servy\db\Servy.db' --snapshot (Join-Path $root 'Servy.db') --current-services *> (Join-Path $root 'servy-backup.json')
if ($LASTEXITCODE -ne 0) { throw 'Servy collation-aware online backup failed.' }
& $checker --verify-snapshot (Join-Path $root 'Servy.db') --inventory (Join-Path $root 'servy-backup.json') *> (Join-Path $root 'servy-verify.json')
if ($LASTEXITCODE -ne 0) { throw 'Servy snapshot revalidation failed.' }
$servyBackup = Get-Content -Raw -LiteralPath (Join-Path $root 'servy-verify.json') | ConvertFrom-Json
if ($servyBackup.result -ne 'PASS') { throw 'Servy snapshot was not accepted.' }
$manifest = [ordered]@{
    state = 'PASS'; observed_at = [DateTime]::UtcNow.ToString('o'); backup_root = $root
    access = 'live sources read-only; SQLite Online Backup API includes committed WAL'
    coord_snapshot = @{ sha256 = (Get-FileHash -LiteralPath (Join-Path $root 'coord.db')).Hash.ToLowerInvariant(); quick_check = $coordBackup.quick_check; row_counts_match = $coordBackup.row_counts_match }
    servy_snapshot = @{ sha256 = (Get-FileHash -LiteralPath (Join-Path $root 'Servy.db')).Hash.ToLowerInvariant(); result = $servyBackup.result }
    installed_binary_sha256 = (Get-FileHash -LiteralPath (Join-Path $root 'binary/coord.exe')).Hash.ToLowerInvariant()
    monitor_task_saved = (Test-Path -LiteralPath (Join-Path $root 'monitor/coord-health-monitor.xml'))
    service_exports = @('coord', 'CoordRelayAgent', 'CoordActionRunnerG7')
    registry_sha256 = (Get-FileHash -LiteralPath (Join-Path $root 'config/registry-production.json')).Hash.ToLowerInvariant()
    binary_sha256 = @{
        coord = (Get-FileHash -LiteralPath (Join-Path $root 'binary/coord.exe')).Hash.ToLowerInvariant()
        runner = (Get-FileHash -LiteralPath (Join-Path $root 'runner/coord-action-runner.exe')).Hash.ToLowerInvariant()
        publisher = (Get-FileHash -LiteralPath (Join-Path $root 'runner/coord-emote-action.exe')).Hash.ToLowerInvariant()
    }
    acl_manifest_saved = (Test-Path -LiteralPath (Join-Path $root 'security/acl-manifest.json'))
    relay_files = @(Get-ChildItem -LiteralPath (Join-Path $root 'relay') -File | Select-Object -ExpandProperty Name)
    secrets = 'configuration and secrets remain only inside administrator-only remote backup'
}
$manifest | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $root 'backup-manifest.json') -Encoding utf8
$manifest | ConvertTo-Json -Depth 8

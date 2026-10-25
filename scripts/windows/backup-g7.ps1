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
foreach ($directory in @('binary', 'relay', 'monitor', 'services')) { New-Item -ItemType Directory -Path (Join-Path $root $directory) | Out-Null }
foreach ($name in @('coord', 'CoordRelayAgent')) {
    & $servy export --name $name --config json --path (Join-Path $root ('services/' + $name + '.json')) --quiet *> (Join-Path $root ('services/' + $name + '.export.log'))
    if ($LASTEXITCODE -ne 0) { throw ('Servy export failed: ' + $name) }
}
Copy-Item -LiteralPath 'C:\ProgramData\coord\bin\coord.exe' -Destination (Join-Path $root 'binary/coord.exe')
Get-ChildItem -LiteralPath 'C:\ProgramData\coord-relay-agent' -File | Copy-Item -Destination (Join-Path $root 'relay')
Get-ChildItem -LiteralPath 'C:\ProgramData\coord\health' -File | Copy-Item -Destination (Join-Path $root 'monitor')
Export-ScheduledTask -TaskName 'coord-health-monitor' | Set-Content -LiteralPath (Join-Path $root 'monitor/coord-health-monitor.xml') -Encoding utf8
Get-CimInstance Win32_Service | Select-Object Name,State,StartMode,StartName,ProcessId | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $root 'services/baseline.json') -Encoding utf8

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
    service_exports = @('coord', 'CoordRelayAgent'); relay_files = @(Get-ChildItem -LiteralPath (Join-Path $root 'relay') -File | Select-Object -ExpandProperty Name)
    secrets = 'configuration and secrets remain only inside administrator-only remote backup'
}
$manifest | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $root 'backup-manifest.json') -Encoding utf8
$manifest | ConvertTo-Json -Depth 8

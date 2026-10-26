$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$backupParent = 'C:\ProgramData\coord\backup'
$requiredBackupPaths = @(
    'coord.db',
    'binary\coord.exe',
    'runner\coord-action-runner.exe',
    'runner\coord-emote-action.exe',
    'services\coord.json',
    'services\CoordActionRunnerG7.json',
    'config\registry-production.json',
    'monitor\coord-health-monitor.ps1',
    'monitor\coord-health-monitor.xml',
    'security\acl-manifest.json'
)

$backups = @()
$backupParentAcl = $null
if (Test-Path -LiteralPath $backupParent -PathType Container) {
    $acl = Get-Acl -LiteralPath $backupParent
    $backupParentAcl = [ordered]@{
        owner = $acl.Owner
        access_rules_protected = $acl.AreAccessRulesProtected
        allow_principals = @($acl.Access | Where-Object AccessControlType -eq 'Allow' |
            ForEach-Object { [pscustomobject]@{ identity = [string]$_.IdentityReference; rights = [string]$_.FileSystemRights; inherited = $_.IsInherited } })
    }
    $backups = @(Get-ChildItem -LiteralPath $backupParent -Directory |
        Sort-Object LastWriteTimeUtc -Descending |
        Select-Object -First 12 |
        ForEach-Object {
            $root = $_.FullName
            $manifestPath = Join-Path $root 'backup-manifest.json'
            $manifestState = $null
            if (Test-Path -LiteralPath $manifestPath -PathType Leaf) {
                try { $manifestState = (Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json).state }
                catch { $manifestState = 'UNREADABLE' }
            }
            [ordered]@{
                name = $_.Name
                last_write_utc = $_.LastWriteTimeUtc.ToString('o')
                manifest_exists = Test-Path -LiteralPath $manifestPath -PathType Leaf
                manifest_state = $manifestState
                required_files = @($requiredBackupPaths | Where-Object {
                    Test-Path -LiteralPath (Join-Path $root $_) -PathType Leaf
                })
            }
        })
}

$serviceNames = @('Servy', 'coord', 'CoordRelayAgent', 'CoordActionRunnerG7')
$services = @(Get-CimInstance Win32_Service | Where-Object Name -in $serviceNames | ForEach-Object {
    [ordered]@{
        name = $_.Name
        state = $_.State
        start_name = $_.StartName
        start_mode = $_.StartMode
        pid = $_.ProcessId
    }
})

$healthTask = $null
try {
    $task = Get-ScheduledTask -TaskName 'coord-health-monitor' -ErrorAction Stop
    $info = Get-ScheduledTaskInfo -TaskName 'coord-health-monitor' -ErrorAction Stop
    $healthTask = [ordered]@{
        principal = $task.Principal.UserId
        state = [string]$task.State
        last_result = $info.LastTaskResult
        last_run_utc = $info.LastRunTime.ToUniversalTime().ToString('o')
        next_run_utc = $info.NextRunTime.ToUniversalTime().ToString('o')
    }
} catch {
    $healthTask = [ordered]@{ error = $_.Exception.GetType().Name }
}

$result = [ordered]@{
    state = 'PASS'
    observed_at_utc = [DateTime]::UtcNow.ToString('o')
    backup_parent_acl = $backupParentAcl
    backup_roots = $backups
    backup_tools = [ordered]@{
        python = Test-Path -LiteralPath 'C:\Users\lieyanbang\AppData\Local\Programs\Python\Python314\python.exe' -PathType Leaf
        sqlite_online_backup_helper = Test-Path -LiteralPath (Join-Path $backupParent 'sqlite-online-backup.py') -PathType Leaf
        servy_snapshot_checker = Test-Path -LiteralPath (Join-Path $backupParent 'servy-snapshot-check.exe') -PathType Leaf
        servy_cli = Test-Path -LiteralPath 'C:\Program Files\Servy\servy-cli.exe' -PathType Leaf
    }
    services = $services
    system_health_task = $healthTask
}
$result | ConvertTo-Json -Depth 12 -Compress

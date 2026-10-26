$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$root = 'C:\ProgramData\coord'
$g7 = Join-Path $root 'g7'
$serviceNames = @('Servy', 'coord', 'CoordRelayAgent', 'CoordActionRunnerG7')
$services = @(Get-CimInstance Win32_Service | Where-Object Name -in $serviceNames | ForEach-Object {
    [ordered]@{
        name = $_.Name
        state = $_.State
        start_name = $_.StartName
        start_mode = $_.StartMode
        pid = $_.ProcessId
        executable = if ($_.PathName -match '^"([^"]+\.exe)') { $Matches[1] } elseif ($_.PathName -match '^([^ ]+\.exe)') { $Matches[1] } else { $null }
    }
})

$listeners = @(Get-NetTCPConnection -State Listen -LocalPort 7777, 7779 -ErrorAction SilentlyContinue | ForEach-Object {
    [ordered]@{ address = $_.LocalAddress; port = $_.LocalPort; pid = $_.OwningProcess }
})

$healthTask = $null
try {
    $task = Get-ScheduledTask -TaskName 'coord-health-monitor' -ErrorAction Stop
    $info = Get-ScheduledTaskInfo -TaskName 'coord-health-monitor' -ErrorAction Stop
    $healthTask = [ordered]@{
        principal = $task.Principal.UserId
        state = [string]$task.State
        last_result = $info.LastTaskResult
        last_run = $info.LastRunTime.ToUniversalTime().ToString('o')
        next_run = $info.NextRunTime.ToUniversalTime().ToString('o')
    }
} catch {
    $healthTask = [ordered]@{ error = $_.Exception.GetType().Name }
}

$healthPath = Join-Path $root 'health\status.json'
$health = $null
if (Test-Path -LiteralPath $healthPath) {
    try {
        $rawHealth = Get-Content -LiteralPath $healthPath -Raw | ConvertFrom-Json
        $health = [ordered]@{
            status = $rawHealth.status
            checked_at_utc = $rawHealth.checked_at_utc
            checks = $rawHealth.checks
            components = $rawHealth.components
            failed_checks = $rawHealth.failed_checks
            consecutive_failures = $rawHealth.consecutive_failures
            runner_consecutive_failures = $rawHealth.runner_consecutive_failures
        }
    } catch {
        $health = [ordered]@{ error = 'status_parse_failed' }
    }
}

$registryPath = Join-Path $g7 'config\registry-production.json'
$registry = [ordered]@{ exists = Test-Path -LiteralPath $registryPath }
if ($registry.exists) {
    try {
        $config = Get-Content -LiteralPath $registryPath -Raw | ConvertFrom-Json
        $runner = $config.external_runner
        $registry.broker_addr = $runner.broker_addr
        $registry.runner_sid = $runner.runner_sid
        $registry.daemon_sid = $runner.daemon_sid
        $registry.broker_key_file_present = [bool]$runner.key_file
        $registry.management_key_file_present = [bool]($config.management_rpc_key_file -or $runner.daemon_rpc_key_file)
        $registry.profile_names = @($config.profiles.PSObject.Properties.Name | Sort-Object)
    } catch {
        $registry.parse = 'failed'
    }
}

$brokerKeyPath = Join-Path $g7 'config\broker.key'
$brokerAcl = $null
if (Test-Path -LiteralPath $brokerKeyPath) {
    try {
        $acl = Get-Acl -LiteralPath $brokerKeyPath
        $brokerAcl = [ordered]@{
            owner = $acl.Owner
            access = @($acl.Access | ForEach-Object {
                [ordered]@{
                    identity = [string]$_.IdentityReference
                    rights = [string]$_.FileSystemRights
                    type = [string]$_.AccessControlType
                    inherited = $_.IsInherited
                }
            })
        }
    } catch {
        $brokerAcl = [ordered]@{ error = $_.Exception.GetType().Name }
    }
}

$coreBinaries = @(
    'C:\ProgramData\coord\bin\coord.exe',
    'C:\ProgramData\coord\g7\bin\coord.exe'
) | Where-Object { Test-Path -LiteralPath $_ } | ForEach-Object {
    $item = Get-Item -LiteralPath $_
    [ordered]@{
        path = $_
        version = $item.VersionInfo.FileVersion
        sha256 = (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}
$coreProcesses = @(Get-CimInstance Win32_Process -Filter "Name='coord.exe'" -ErrorAction SilentlyContinue | ForEach-Object {
    [ordered]@{ pid = $_.ProcessId; executable = $_.ExecutablePath }
})
$logDirectory = Join-Path $g7 'runtime\logs'
$coreLogInventory = @()
$coreLogCategories = @()
if (Test-Path -LiteralPath $logDirectory) {
    $candidateLogs = @(Get-ChildItem -LiteralPath $logDirectory -File -ErrorAction SilentlyContinue | Where-Object {
        $_.Name -match '(?i)coord|servy|runner|stderr|stdout'
    } | Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 12)
    $coreLogInventory = @($candidateLogs | ForEach-Object {
        [ordered]@{ name = $_.Name; last_write_utc = $_.LastWriteTimeUtc.ToString('o'); bytes = $_.Length }
    })
    foreach ($log in $candidateLogs | Select-Object -First 5) {
        foreach ($line in (Get-Content -LiteralPath $log.FullName -Tail 120 -ErrorAction SilentlyContinue)) {
            if ($line -match '(?i)inspect trusted path component') { $coreLogCategories += 'trusted_path_check_error' }
            elseif ($line -match '(?i)access is denied|access denied') { $coreLogCategories += 'access_denied' }
            elseif ($line -match '(?i)address already in use|only one usage|broker.*bind') { $coreLogCategories += 'broker_bind_error' }
            elseif ($line -match '(?i)Action Broker daemon identity mismatch') { $coreLogCategories += 'daemon_identity_mismatch' }
            elseif ($line -match '(?i)broker.*(invalid|config|key|signature)') { $coreLogCategories += 'broker_configuration_or_auth_error' }
        }
    }
    $coreLogCategories = @($coreLogCategories | Sort-Object -Unique)
}

$rpcProbe = 'not_attempted'
$rpcListeners = @($listeners | Where-Object { $_.port -eq 7777 })
if ($rpcListeners.Count -eq 0) {
    $rpcProbe = 'connection_refused'
} else {
try {
    $body = [Text.Encoding]::UTF8.GetBytes('{"jsonrpc":"2.0","id":1,"method":"tasks/list","params":{"limit":1,"skip_reclaim":true}}')
    $request = [Net.WebRequest]::Create('http://127.0.0.1:7777/')
    $request.Method = 'POST'
    $request.ContentType = 'application/json'
    $request.Timeout = 3000
    $request.ReadWriteTimeout = 3000
    $request.ContentLength = $body.Length
    $stream = $request.GetRequestStream()
    $stream.Write($body, 0, $body.Length)
    $stream.Dispose()
    try {
        $response = $request.GetResponse()
        $rpcProbe = [string][int]$response.StatusCode
        $response.Dispose()
    } catch [Net.WebException] {
        if ($_.Exception.Response) {
            $rpcProbe = [string][int]$_.Exception.Response.StatusCode
            $_.Exception.Response.Dispose()
        } elseif ($_.Exception.Message -match 'refused|actively refused|unable to connect|No connection could be made' -or ($_.Exception.InnerException -and $_.Exception.InnerException.Message -match 'refused|actively refused|unable to connect|No connection could be made')) {
            $rpcProbe = 'connection_refused'
        } else {
            $rpcProbe = $_.Exception.GetType().Name
        }
    }
} catch {
    $rpcProbe = $_.Exception.GetType().Name
}
}

$pythonPath = 'C:\Users\lieyanbang\AppData\Local\Programs\Python\Python314\python.exe'
$dbPath = Join-Path $root 'data\coord.db'
$dbAudit = $null
if (Test-Path -LiteralPath $dbPath) {
    $python = @'
import hashlib, json, sqlite3, sys
from urllib.parse import quote

path = sys.argv[1]
uri = 'file:' + quote(path.replace('\\', '/'), safe='/:') + '?mode=ro'
db = sqlite3.connect(uri, uri=True, timeout=3)
db.execute('PRAGMA query_only=ON')
db.execute('BEGIN')
tables = {r[0] for r in db.execute("SELECT name FROM sqlite_master WHERE type='table'")}
integrity = db.execute('PRAGMA integrity_check').fetchone()[0]
user_version = db.execute('PRAGMA user_version').fetchone()[0]
schema_version = db.execute('PRAGMA schema_version').fetchone()[0]
journal_mode = db.execute('PRAGMA journal_mode').fetchone()[0]
formal = '90ce2ca7-184e-4c3d-9d87-56386db25419'
canaries = ['ddb65e62-2847-4dbb-a188-40f2c20c9c8e', 'd2313c78-22da-4cb8-bcd7-ece9897e0db1']

def safe_rows(table, where_sql, args):
    if table not in tables:
        return []
    columns = [r[1] for r in db.execute('PRAGMA table_info("' + table.replace('"', '""') + '")')]
    safe = [c for c in columns if not any(x in c.lower() for x in ('token', 'secret', 'credential', 'private_key'))]
    if not safe:
        return []
    projection = ','.join('"' + c.replace('"', '""') + '"' for c in safe)
    rows = [dict(zip(safe, row)) for row in db.execute('SELECT ' + projection + ' FROM "' + table + '" ' + where_sql, args)]
    return sorted(rows, key=lambda row: json.dumps(row, sort_keys=True, separators=(',', ':'), default=str))

def batch_report(batch_id):
    batch = safe_rows('batches', 'WHERE id=?', [batch_id])
    task_rows = safe_rows('tasks', 'WHERE batch_id=?', [batch_id])
    ids = [r['id'] for r in task_rows if 'id' in r]
    states = {}
    for row in task_rows:
        states[row['state']] = states.get(row['state'], 0) + 1
    related = {'batches': batch, 'tasks': task_rows}
    task_tables = ['task_attempts', 'task_progress', 'task_progress_receipts', 'task_progress_extend_receipts', 'task_receipts']
    for table in task_tables:
        if table in tables and ids:
            cols = [r[1] for r in db.execute('PRAGMA table_info("' + table + '")')]
            if 'task_id' in cols:
                marks = ','.join('?' for _ in ids)
                related[table] = safe_rows(table, 'WHERE task_id IN (' + marks + ')', ids)
            elif 'batch_id' in cols:
                related[table] = safe_rows(table, 'WHERE batch_id=?', [batch_id])
    related['completion_actions'] = safe_rows('completion_actions', 'WHERE batch_id=?', [batch_id]) if 'completion_actions' in tables else []
    # task_attempts is a derived table created additively by the new schema.
    # Exclude it from the legacy-row fingerprint while reporting its own
    # count/fingerprint so a migration does not masquerade as a task rewrite.
    stable_related = {key: value for key, value in related.items() if key != 'task_attempts'}
    canonical = json.dumps(stable_related, sort_keys=True, separators=(',', ':'), default=str).encode()
    attempt_canonical = json.dumps(related.get('task_attempts', []), sort_keys=True, separators=(',', ':'), default=str).encode()
    return {
        'batch_id': batch_id,
        'batch_found': bool(batch),
        'task_count': len(task_rows),
        'task_states': states,
        'task_attempt_sum': sum(int(row.get('attempt_count', 0) or 0) for row in task_rows),
        'task_attempt_max': max((int(row.get('attempt_count', 0) or 0) for row in task_rows), default=0),
        'attempt_table_rows': len(related.get('task_attempts', [])),
        'derived_attempts_fingerprint': hashlib.sha256(attempt_canonical).hexdigest(),
        'progress_count': len(related.get('task_progress', [])),
        'progress_receipt_count': len(related.get('task_progress_receipts', [])),
        'extension_receipt_count': len(related.get('task_progress_extend_receipts', [])),
        'task_receipt_count': len(related.get('task_receipts', [])),
        'action_count': len(related.get('completion_actions', [])),
        'protected_fingerprint': hashlib.sha256(canonical).hexdigest(),
    }

global_action_states = {}
if 'completion_actions' in tables:
    for state, count in db.execute('SELECT state,COUNT(*) FROM completion_actions GROUP BY state'):
        global_action_states[state] = count

print(json.dumps({
    'integrity_check': integrity,
    'user_version': user_version,
    'schema_version': schema_version,
    'journal_mode': journal_mode,
    'task_attempts_table': 'task_attempts' in tables,
    'runner_nonces_table': 'runner_nonces' in tables,
    'runner_nonce_rows': db.execute('SELECT COUNT(*) FROM runner_nonces').fetchone()[0] if 'runner_nonces' in tables else 0,
    'completion_action_states': global_action_states,
    'protected': [batch_report(batch_id) for batch_id in [formal] + canaries],
}, sort_keys=True))
'@
    $tempPython = Join-Path $env:TEMP ('coord-r5-b0-' + [guid]::NewGuid().ToString('N') + '.py')
    try {
        [IO.File]::WriteAllText($tempPython, $python, [Text.UTF8Encoding]::new($false))
        $rawAudit = & $pythonPath $tempPython $dbPath 2>$null
        if ($LASTEXITCODE -ne 0) { throw 'read_only_sqlite_audit_failed' }
        $dbAudit = ($rawAudit -join "`n") | ConvertFrom-Json
    } catch {
        $dbAudit = [ordered]@{ error = 'read_only_audit_failed' }
    } finally {
        Remove-Item -LiteralPath $tempPython -Force -ErrorAction SilentlyContinue
    }
}

$result = [ordered]@{
    state = 'PASS'
    observed_at_utc = [DateTime]::UtcNow.ToString('o')
    host = $env:COMPUTERNAME
    services = $services
    listeners = $listeners
    unauthenticated_rpc_probe = $rpcProbe
    system_health_task = $healthTask
    health_status = $health
    registry = $registry
    broker_key_acl = $brokerAcl
    core_binaries = @($coreBinaries)
    core_processes = $coreProcesses
    core_log_inventory = $coreLogInventory
    core_log_categories = $coreLogCategories
    database = $dbAudit
}
$result | ConvertTo-Json -Depth 16 -Compress

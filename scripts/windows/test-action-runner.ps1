param(
    [Parameter(Mandatory)][string]$BinaryDirectory,
    [Parameter(Mandatory)][string]$EvidenceDirectory
)
$ErrorActionPreference = 'Stop'
# This account/ACL rehearsal is deliberately confined to disposable hosted CI.
# It must never provision identities or touch ACLs on an operator/production host.
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'Run this account-creating rehearsal only on a disposable GitHub Actions Windows runner.' }
if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Disposable runner administrator token required.' }
$binaryRoot = (Resolve-Path -LiteralPath $BinaryDirectory).Path
$evidenceRoot = [IO.Path]::GetFullPath($EvidenceDirectory)
New-Item -ItemType Directory -Path $evidenceRoot -Force | Out-Null
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('coord-g6-' + [guid]::NewGuid().ToString('N'))
$testRoot = [IO.Path]::GetFullPath($testRoot)
$userName = 'coordT' + [guid]::NewGuid().ToString('N').Substring(0, 10)
$createdSid = $null
$daemonProcess = $null
$runnerProcess = $null
$summary = [ordered]@{ state = 'FAIL'; disposable_ci = $true; production_gate = 'NOT_RUN'; checks = @() }
function Assert-LocalTarget([string]$Target) {
    $resolved = [IO.Path]::GetFullPath($Target)
    if (-not $resolved.StartsWith($testRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Fixture path escapes exact disposable test root.' }
    return $resolved
}
function Set-FixtureAcl([string]$Target, [string[]]$Grants) {
    $resolved = Assert-LocalTarget $Target
    & icacls.exe $resolved /inheritance:r /grant:r '*S-1-5-32-544:(OI)(CI)F' '*S-1-5-18:(OI)(CI)F' @Grants | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Fixture ACL application failed.' }
}
function Free-Port {
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
    $listener.Start(); $portNumber = $listener.LocalEndpoint.Port; $listener.Stop(); return $portNumber
}
function Rpc([string]$Method, $Params) {
    $body = @{ jsonrpc = '2.0'; id = 1; method = $Method; params = $Params } | ConvertTo-Json -Depth 40 -Compress
    $response = Invoke-RestMethod -Uri $script:daemonUrl -Method Post -ContentType 'application/json' -Body $body -TimeoutSec 10 -Headers @{ Authorization = ('Bearer ' + $script:managementKey) }
    if ($response.error) { throw ('Isolated RPC failed: ' + $response.error.message) }
    return $response.result
}
function Run-AsRunner([string]$Label, [string]$ExtraArguments, [int]$ExpectedExit = 0) {
    $stdoutPath = Assert-LocalTarget (Join-Path $testRoot ('logs/' + $Label + '.stdout.log'))
    $stderrPath = Assert-LocalTarget (Join-Path $testRoot ('logs/' + $Label + '.stderr.log'))
    $arguments = '--registry "' + $registryPath + '" ' + $ExtraArguments
    $process = Start-Process -FilePath $runnerExe -ArgumentList $arguments -WorkingDirectory $testRoot -Credential $credential -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
    if (-not $process.WaitForExit(45000)) { Stop-Process -Id $process.Id; throw 'Disposable runner process timeout.' }
    $process.Refresh()
    Copy-Item -LiteralPath $stderrPath -Destination (Join-Path $evidenceRoot ($Label + '.stderr.log'))
    if ($process.ExitCode -ne $ExpectedExit) { throw ($Label + ' exit ' + $process.ExitCode) }
    $script:summary.checks += @{ name = $Label; exit_code = $process.ExitCode }
}
try {
    New-Item -ItemType Directory -Path $testRoot | Out-Null
    $passwordText = [Convert]::ToBase64String([Security.Cryptography.RandomNumberGenerator]::GetBytes(24)) + 'aA1!'
    $securePassword = ConvertTo-SecureString -String $passwordText -AsPlainText -Force
    $localUser = New-LocalUser -Name $userName -Password $securePassword -AccountNeverExpires -PasswordNeverExpires -Description 'Disposable coord G6 runner acceptance fixture'
    $createdSid = $localUser.SID.Value
    Add-LocalGroupMember -SID 'S-1-5-32-545' -Member $userName
    $credential = [Management.Automation.PSCredential]::new(($env:COMPUTERNAME + '\' + $userName), $securePassword)
    $passwordText = $null
    $summary.runner_sid = $createdSid
    $daemonSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $summary.daemon_sid = $daemonSid
    foreach ($directory in @('bin', 'config', 'protected', 'logs', 'scratch', 'business/refs/arknights/icons', 'business/task/00', 'business/A1/private-attempt', 'business/results/.receipts', 'business/results/review_queue')) {
        New-Item -ItemType Directory -Path (Join-Path $testRoot $directory) -Force | Out-Null
    }
    foreach ($binaryName in @('coord.exe', 'coord-action-runner.exe', 'coord-emote-action.exe')) {
        Copy-Item -LiteralPath (Join-Path $binaryRoot $binaryName) -Destination (Join-Path $testRoot ('bin/' + $binaryName))
    }
    $runnerExe = Join-Path $testRoot 'bin/coord-action-runner.exe'
    $publisherExe = Join-Path $testRoot 'bin/coord-emote-action.exe'
    $pingExe = Join-Path $testRoot 'bin/ping-fixture.exe'
    Copy-Item -LiteralPath (Join-Path $env:windir 'System32/ping.exe') -Destination $pingExe
    $registryPath = Join-Path $testRoot 'config/registry.json'
    $keyPath = Join-Path $testRoot 'config/broker.key'
    $managementKeyPath = Join-Path $testRoot 'protected/management.key'
    $managementKey = [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32))
    [IO.File]::WriteAllText($managementKeyPath, $managementKey)
    [IO.File]::WriteAllText($keyPath, [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32)))
    [IO.File]::WriteAllText((Join-Path $testRoot 'business/refs/arknights/icons/one.png'), 'disposable reference')
    [IO.File]::WriteAllText((Join-Path $testRoot 'business/task/00/1.png'), 'disposable image')
    [IO.File]::WriteAllText((Join-Path $testRoot 'protected/state.db'), '')
    [IO.File]::WriteAllText((Join-Path $testRoot 'business/A1/private-attempt/secret.json'), '{}')
    $daemonPort = Free-Port; $brokerPort = Free-Port
    $daemonUrl = 'http://127.0.0.1:' + $daemonPort
    $command = @{ executable = $publisherExe; sha256 = (Get-FileHash -LiteralPath $publisherExe -Algorithm SHA256).Hash.ToLowerInvariant(); working_directory = (Join-Path $testRoot 'scratch'); args = @('--root', (Join-Path $testRoot 'business')); include_result = $true; timeout_ms = 10000; max_output_bytes = 65536; max_attempts = 2; idempotent = $true }
    $crashCommand = @{ executable = $pingExe; sha256 = (Get-FileHash -LiteralPath $pingExe -Algorithm SHA256).Hash.ToLowerInvariant(); working_directory = (Join-Path $testRoot 'scratch'); args = @('-n', '30', '127.0.0.1'); include_result = $false; timeout_ms = 30000; max_output_bytes = 65536; max_attempts = 1; idempotent = $false }
    @{ version = 1; allowed_root = $testRoot; external_runner = @{ broker_addr = ('127.0.0.1:' + $brokerPort); key_file = $keyPath; daemon_rpc_key_file = $managementKeyPath; runner_sid = $createdSid; daemon_sid = $daemonSid }; profiles = @{ emote = @{ version = 1; on_task_completed = $command; on_batch_completed = $command }; crash = @{ version = 1; on_task_completed = $crashCommand } } } | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $registryPath -Encoding utf8NoBOM
    # Administrator owns every directory. Runner gets read/execute on trusted
    # code/config/input, modify only on exact output/scratch/log subtrees.
    & icacls.exe $testRoot /inheritance:r /grant:r '*S-1-5-32-544:(OI)(CI)F' '*S-1-5-18:(OI)(CI)F' ('*' + $createdSid + ':(OI)(CI)RX') | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Test root ACL failed.' }
    Set-FixtureAcl (Join-Path $testRoot 'protected') @()
    Set-FixtureAcl (Join-Path $testRoot 'business/A1') @()
    foreach ($directory in @('logs', 'scratch', 'business/results')) { Set-FixtureAcl (Join-Path $testRoot $directory) @('*' + $createdSid + ':(OI)(CI)M') }
    $probePath = Join-Path $testRoot 'config/acl-probe.json'
    @{ read_files = @($registryPath, $keyPath, $publisherExe, (Join-Path $testRoot 'business/task/00/1.png'), (Join-Path $testRoot 'business/refs/arknights/icons/one.png')); denied_read_files = @((Join-Path $testRoot 'protected/state.db'), $managementKeyPath, (Join-Path $testRoot 'business/A1/private-attempt/secret.json')); denied_write_files = @($registryPath, $publisherExe, (Join-Path $testRoot 'bin/coord.exe')); write_directories = @((Join-Path $testRoot 'scratch'), (Join-Path $testRoot 'logs'), (Join-Path $testRoot 'business/results')); denied_write_directories = @((Join-Path $testRoot 'protected'), (Join-Path $testRoot 'config'), (Join-Path $testRoot 'bin'), $env:windir); denied_rpc_url = $daemonUrl; broker_key_file = $keyPath } | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $probePath -Encoding utf8NoBOM
    Run-AsRunner 'identity' '--check-identity'
    & icacls.exe $testRoot /T | Set-Content -LiteralPath (Join-Path $evidenceRoot 'fixture-acls.log')
    if ($LASTEXITCODE -ne 0) { throw 'ACL evidence capture failed.' }
    $daemonProcess = Start-Process -FilePath (Join-Path $testRoot 'bin/coord.exe') -ArgumentList @('serve', '--addr', ('127.0.0.1:' + $daemonPort), '--db', ('"' + (Join-Path $testRoot 'protected/state.db') + '"'), '--actions-config', ('"' + $registryPath + '"')) -WorkingDirectory $testRoot -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $testRoot 'logs/daemon.stderr.log') -RedirectStandardOutput (Join-Path $testRoot 'logs/daemon.stdout.log')
    $ready = $false
    foreach ($attempt in 1..100) { try { Invoke-RestMethod ($daemonUrl + '/.well-known/agent.json') -TimeoutSec 1 | Out-Null; $ready = $true; break } catch { Start-Sleep -Milliseconds 100 } }
    if (-not $ready) { throw 'Isolated daemon readiness failed.' }
    Run-AsRunner 'acl-policy' ('--probe-acl "' + $probePath + '"')
    $batch = Rpc 'batches/create' @{ name = 'G6 disposable low privilege runner'; config = @{ action_profile = 'emote' } }
    Rpc 'batches/add_tasks' @{ batch_id = $batch.id; tasks = @(@{ key = '00'; name = 'disposable test'; payload = @{ group_id = '00'; source_image_ids = @('1') } }) } | Out-Null
    $task = (Rpc 'scheduler/claim' @{ batch_id = $batch.id; agent_id = 'A0'; lease_seconds = 30 }).task
    $result = @{ schema_version = 1; group_id = '00'; agent_id = 'A0'; attempt_no = $task.attempt_no; source_image_ids = @('1'); characters = @(); uncertain = @(@{ image_id = '1'; visible_features = 'Visible disposable character evidence'; reference_candidates = @(); checked_paths = @('refs/arknights/icons/one.png'); check_conclusion = 'Checked disposable reference without match'; reason = 'Insufficient visual identity information'; review_priority = 'high' }); general_notes = '' }
    Rpc 'scheduler/submit' @{ id = $task.id; agent_id = 'A0'; lease_token = $task.lease_token; result = $result } | Out-Null
    Run-AsRunner 'publish-task' '--once'
    Run-AsRunner 'publish-index' '--once'
    $actions = Rpc 'actions/status' @{ batch_id = $batch.id }
    if (@($actions.actions).Count -ne 2 -or @($actions.actions | Where-Object state -ne 'succeeded').Count -ne 0) { throw 'Low privilege actions did not both succeed.' }
    foreach ($relative in @('business/results/00.json', 'business/results/.receipts/00.json', 'business/results/index.json', 'business/results/review_queue/candidates.json')) {
        $source = Assert-LocalTarget (Join-Path $testRoot $relative)
        if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw 'Missing publication output.' }
        Copy-Item -LiteralPath $source -Destination (Join-Path $evidenceRoot ($relative.Replace('/', '-')))
    }
    $summary.actions = $actions
    # Kill exactly our independently authenticated runner while its pinned native
    # child is active. No production process or live lease is involved.
    $crashBatch = Rpc 'batches/create' @{ name = 'G6 disposable non-idempotent crash'; config = @{ action_profile = 'crash' } }
    Rpc 'batches/add_tasks' @{ batch_id = $crashBatch.id; tasks = @(@{ key = 'one'; name = 'crash fixture'; payload = @{} }) } | Out-Null
    $crashTask = (Rpc 'scheduler/claim' @{ batch_id = $crashBatch.id; agent_id = 'A0'; lease_seconds = 30 }).task
    Rpc 'scheduler/submit' @{ id = $crashTask.id; agent_id = 'A0'; lease_token = $crashTask.lease_token; result = @{ ok = $true } } | Out-Null
    $runnerProcess = Start-Process -FilePath $runnerExe -ArgumentList ('--registry "' + $registryPath + '" --once') -WorkingDirectory $testRoot -Credential $credential -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $testRoot 'logs/crash-runner.stderr.log') -RedirectStandardOutput (Join-Path $testRoot 'logs/crash-runner.stdout.log')
    $childProcess = $null
    foreach ($attempt in 1..100) {
        $childProcess = Get-CimInstance Win32_Process -Filter ('ParentProcessId=' + $runnerProcess.Id) | Where-Object ExecutablePath -eq $pingExe | Select-Object -First 1
        if ($childProcess) { break }; Start-Sleep -Milliseconds 100
    }
    if (-not $childProcess) { throw 'Pinned non-idempotent child never started.' }
    $crashAction = (Rpc 'actions/status' @{ batch_id = $crashBatch.id }).actions | Select-Object -First 1
    if ($crashAction.state -ne 'running') { throw 'Crash fixture action is not running.' }
    Stop-Process -Id $runnerProcess.Id
    $runnerProcess.WaitForExit()
    foreach ($attempt in 1..50) { if (-not (Get-Process -Id $childProcess.ProcessId -ErrorAction SilentlyContinue)) { break }; Start-Sleep -Milliseconds 100 }
    if (Get-Process -Id $childProcess.ProcessId -ErrorAction SilentlyContinue) { throw 'Runner crash failed to kill its Job Object child.' }
    $expirePath = Join-Path $testRoot 'config/expire-fixture-lease.py'
    @'
import sqlite3, sys
with sqlite3.connect(sys.argv[1]) as db:
    changed = db.execute("UPDATE completion_actions SET lease_until='2000-01-01T00:00:00Z' WHERE id=? AND state='running'", (sys.argv[2],)).rowcount
    assert changed == 1
'@ | Set-Content -LiteralPath $expirePath -Encoding utf8NoBOM
    & python $expirePath (Join-Path $testRoot 'protected/state.db') $crashAction.id
    if ($LASTEXITCODE -ne 0) { throw 'Disposable lease expiry injection failed.' }
    Run-AsRunner 'crash-recovery' '--once'
    $recovered = (Rpc 'actions/status' @{ batch_id = $crashBatch.id }).actions | Select-Object -First 1
    if ($recovered.state -ne 'uncertain' -or $recovered.attempt_count -ne 1) { throw 'Non-idempotent crash was incorrectly replayed.' }
    if ((Rpc 'tasks/get' @{ id = $crashTask.id }).state -ne 'completed') { throw 'Action crash reopened task state.' }
    $summary.crash_recovery = @{ state = $recovered.state; attempt_count = $recovered.attempt_count; child_killed_on_runner_exit = $true; isolated_forced_lease_expiry = $true }
    $summary.state = 'PASS'
} finally {
    if ($runnerProcess -and -not $runnerProcess.HasExited) { Stop-Process -Id $runnerProcess.Id -ErrorAction SilentlyContinue }
    if ($daemonProcess -and -not $daemonProcess.HasExited) { Stop-Process -Id $daemonProcess.Id -ErrorAction SilentlyContinue; $daemonProcess.WaitForExit() }
    if ($createdSid) {
        $currentUser = Get-LocalUser -Name $userName -ErrorAction SilentlyContinue
        if ($currentUser -and $currentUser.SID.Value -eq $createdSid -and $userName.StartsWith('coordT')) { Remove-LocalUser -Name $userName; $summary.disposable_account_removed = -not (Get-LocalUser -Name $userName -ErrorAction SilentlyContinue) }
    }
    # Preserve evidence outside fixture before removing only this exact random
    # directory. Never enumerate and pass paths to another shell for deletion.
    $summary | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath (Join-Path $evidenceRoot 'summary.json') -Encoding utf8NoBOM
    if (Test-Path -LiteralPath (Join-Path $testRoot 'logs/daemon.stderr.log')) { Copy-Item -LiteralPath (Join-Path $testRoot 'logs/daemon.stderr.log') -Destination (Join-Path $evidenceRoot 'daemon.stderr.log') }
    $resolvedRoot = [IO.Path]::GetFullPath($testRoot)
    $expectedParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if ($resolvedRoot.StartsWith($expectedParent, [StringComparison]::OrdinalIgnoreCase) -and [IO.Path]::GetFileName($resolvedRoot) -match '^coord-g6-[0-9a-f]{32}$') { Remove-Item -LiteralPath $resolvedRoot -Recurse -Force }
}

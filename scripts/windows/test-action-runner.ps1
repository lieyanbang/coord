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
# A standard user must be able to inspect every trusted path ancestor. Hosted
# administrator-profile TEMP ancestors deny that metadata access, so use an
# exact random child of the system volume root (never a shared writable parent).
$systemVolumeRoot = [IO.Path]::GetPathRoot([IO.Path]::GetFullPath($env:SystemRoot))
if ($systemVolumeRoot -notmatch '^[A-Za-z]:\\$') { throw 'Expected a local system-volume root for the disposable fixture.' }
$testRoot = Join-Path $systemVolumeRoot ('coord-r5-broker-' + [guid]::NewGuid().ToString('N'))
$testRoot = [IO.Path]::GetFullPath($testRoot)
$userName = 'coordT' + [guid]::NewGuid().ToString('N').Substring(0, 10)
$createdSid = $null
$daemonTaskName = 'coord-r5-broker-' + [guid]::NewGuid().ToString('N')
$daemonCoreProcessId = $null
$runnerProcess = $null
$summary = [ordered]@{ state = 'FAIL'; phase = 'initialize'; failed_phase = $null; disposable_ci = $true; fixture_layout = 'system-volume-root'; production_gate = 'NOT_RUN'; checks = @() }
function Assert-LocalTarget([string]$Target) {
    $resolved = [IO.Path]::GetFullPath($Target)
    if (-not $resolved.Equals($testRoot, [StringComparison]::OrdinalIgnoreCase) -and -not $resolved.StartsWith($testRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Fixture path escapes exact disposable test root.' }
    return $resolved
}
function Set-FixtureAcl([string]$Target) {
    $resolved = Assert-LocalTarget $Target
    & icacls.exe $resolved /inheritance:r | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Fixture ACL application failed.' }
    Add-FixtureGrant $resolved '*S-1-5-32-544:(OI)(CI)F'
    Add-FixtureGrant $resolved '*S-1-5-18:(OI)(CI)F'
}
function Add-FixtureGrant([string]$Target, [string]$Grant) {
    $resolved = Assert-LocalTarget $Target
    & icacls.exe $resolved /grant:r $Grant | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Fixture ACL application failed.' }
}
function Free-PortPair {
    $listeners = [Collections.Generic.List[Net.Sockets.TcpListener]]::new()
    try {
        foreach ($index in 1..2) {
            $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
            $listener.Start()
            $listeners.Add($listener)
        }
        return @($listeners | ForEach-Object { [int]$_.LocalEndpoint.Port })
    } finally {
        foreach ($listener in $listeners) { $listener.Stop() }
    }
}
function Rpc([string]$Method, $Params) {
    $body = @{ jsonrpc = '2.0'; id = 1; method = $Method; params = $Params } | ConvertTo-Json -Depth 40 -Compress
    $response = Invoke-RestMethod -Uri $script:daemonUrl -Method Post -ContentType 'application/json' -Body $body -TimeoutSec 10 -Headers @{ Authorization = ('Bearer ' + $script:managementKey) }
    if ($response.error) { throw ('Isolated RPC failed: ' + $response.error.message) }
    return $response.result
}
function Run-AsRunner([string]$Label, [string]$ExtraArguments, [int]$ExpectedExit = 0) {
    $script:summary.phase = $Label
    $stdoutPath = Assert-LocalTarget (Join-Path $testRoot ('logs/' + $Label + '.stdout.log'))
    $stderrPath = Assert-LocalTarget (Join-Path $testRoot ('logs/' + $Label + '.stderr.log'))
    $arguments = '--registry "' + $registryPath + '" --private-key-file "' + $privateKeyPath + '" ' + $ExtraArguments
    $process = Start-Process -FilePath $runnerExe -ArgumentList $arguments -WorkingDirectory $testRoot -Credential $credential -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
    if (-not $process.WaitForExit(45000)) {
        Stop-Process -Id $process.Id -Force -Confirm:$false
        if (-not $process.WaitForExit(10000)) { throw 'Disposable timed-out runner did not exit.' }
        throw 'Disposable runner process timeout.'
    }
    $process.Refresh()
    Copy-Item -LiteralPath $stderrPath -Destination (Join-Path $evidenceRoot ($Label + '.stderr.log'))
    $script:summary.checks += @{ name = $Label; exit_code = $process.ExitCode; expected_exit_code = $ExpectedExit }
    if ($process.ExitCode -ne $ExpectedExit) { throw ($Label + ' exit ' + $process.ExitCode) }
}
try {
    $summary.phase = 'create-disposable-fixture'
    New-Item -ItemType Directory -Path $testRoot | Out-Null
    $passwordBytes = New-Object byte[] 24
    $randomGenerator = [Security.Cryptography.RandomNumberGenerator]::Create()
    try { $randomGenerator.GetBytes($passwordBytes) } finally { $randomGenerator.Dispose() }
    $passwordText = [Convert]::ToBase64String($passwordBytes) + 'aA1!'
    $securePassword = ConvertTo-SecureString -String $passwordText -AsPlainText -Force
    $localUser = New-LocalUser -Name $userName -Password $securePassword -AccountNeverExpires -PasswordNeverExpires -Description 'Disposable coord R5-1B Runner acceptance fixture'
    $createdSid = $localUser.SID.Value
    Add-LocalGroupMember -SID 'S-1-5-32-545' -Member $userName
    $credential = [Management.Automation.PSCredential]::new(($env:COMPUTERNAME + '\' + $userName), $securePassword)
    $passwordText = $null
    $summary.runner_sid = $createdSid
    $daemonSid = 'S-1-5-19'
    $summary.daemon_sid = $daemonSid
    foreach ($directory in @('bin', 'config', 'management', 'runner-private', 'data', 'protected', 'logs', 'scratch', 'business/refs/arknights/icons', 'business/task/00', 'business/A1/private-attempt', 'business/results/.receipts', 'business/results/review_queue')) {
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
    $privateKeyPath = Join-Path $testRoot 'runner-private/runner.pk8'
    $publicKeyPath = Join-Path $testRoot 'config/runner-public.key'
    $managementKeyPath = Join-Path $testRoot 'management/management.key'
    $databasePath = Join-Path $testRoot 'data/state.db'
    $managementBytes = New-Object byte[] 32
    $randomGenerator = [Security.Cryptography.RandomNumberGenerator]::Create()
    try { $randomGenerator.GetBytes($managementBytes) } finally { $randomGenerator.Dispose() }
    $managementKey = ([BitConverter]::ToString($managementBytes) -replace '-', '').ToLowerInvariant()
    [IO.File]::WriteAllText((Join-Path $testRoot 'business/refs/arknights/icons/one.png'), 'disposable reference')
    [IO.File]::WriteAllText((Join-Path $testRoot 'business/task/00/1.png'), 'disposable image')
    [IO.File]::WriteAllText((Join-Path $testRoot 'business/A1/private-attempt/secret.json'), '{}')
    $ports = Free-PortPair
    $daemonPort = [int]$ports[0]; $brokerPort = [int]$ports[1]
    if ($daemonPort -eq $brokerPort) { throw 'RPC and Broker fixture ports must be distinct.' }
    $summary.daemon_port = $daemonPort
    $summary.broker_port = $brokerPort
    $daemonUrl = 'http://127.0.0.1:' + $daemonPort
    $command = @{ executable = $publisherExe; sha256 = (Get-FileHash -LiteralPath $publisherExe -Algorithm SHA256).Hash.ToLowerInvariant(); working_directory = (Join-Path $testRoot 'scratch'); args = @('--root', (Join-Path $testRoot 'business')); include_result = $true; timeout_ms = 10000; max_output_bytes = 65536; max_attempts = 2; idempotent = $true }
    $crashCommand = @{ executable = $pingExe; sha256 = (Get-FileHash -LiteralPath $pingExe -Algorithm SHA256).Hash.ToLowerInvariant(); working_directory = (Join-Path $testRoot 'scratch'); args = @('-n', '30', '127.0.0.1'); include_result = $false; timeout_ms = 30000; max_output_bytes = 65536; max_attempts = 1; idempotent = $false }
    # LocalService is the real Core identity; the dedicated user is the Runner.
    # Core gets only the public key. Its service token cannot open the private key.
    Set-FixtureAcl $testRoot
    Add-FixtureGrant $testRoot '*S-1-5-19:(OI)(CI)RX'
    Add-FixtureGrant $testRoot ('*' + $createdSid + ':(OI)(CI)RX')
    foreach ($relative in @('config', 'bin')) {
        $path = Join-Path $testRoot $relative
        Set-FixtureAcl $path
        Add-FixtureGrant $path '*S-1-5-19:(OI)(CI)RX'
        Add-FixtureGrant $path ('*' + $createdSid + ':(OI)(CI)RX')
    }
    $managementPath = Join-Path $testRoot 'management'
    Set-FixtureAcl $managementPath
    Add-FixtureGrant $managementPath '*S-1-5-19:(OI)(CI)R'
    $runnerPrivatePath = Join-Path $testRoot 'runner-private'
    Set-FixtureAcl $runnerPrivatePath
    Add-FixtureGrant $runnerPrivatePath ('*' + $createdSid + ':(OI)(CI)R')
    $dataPath = Join-Path $testRoot 'data'
    Set-FixtureAcl $dataPath
    Add-FixtureGrant $dataPath '*S-1-5-19:(OI)(CI)M'
    foreach ($relative in @('protected', 'business/A1')) { Set-FixtureAcl (Join-Path $testRoot $relative) }
    foreach ($relative in @('logs', 'scratch', 'business/results')) {
        $path = Join-Path $testRoot $relative
        Set-FixtureAcl $path
        Add-FixtureGrant $path ('*' + $createdSid + ':(OI)(CI)M')
        Add-FixtureGrant $path '*S-1-5-19:(OI)(CI)M'
    }
    [IO.File]::WriteAllText($managementKeyPath, $managementKey)
    [IO.File]::WriteAllText($databasePath, '')
    & $runnerExe --generate-keypair --private-key-file $privateKeyPath --public-key-file $publicKeyPath 2> (Join-Path $testRoot 'logs/keygen.stderr.log')
    if ($LASTEXITCODE -ne 0) { throw 'Isolated Ed25519 key generation failed.' }
    $keygenOutput = [IO.File]::ReadAllText((Join-Path $testRoot 'logs/keygen.stderr.log'))
    if ($keygenOutput -match '[A-Za-z0-9_-]{64,}') { throw 'Key generation output included key-like material.' }
    $registry = @{ version = 1; allowed_root = $testRoot; management_rpc_key_file = $managementKeyPath; external_runner = @{ broker_addr = ('127.0.0.1:' + $brokerPort); runner_sid = $createdSid; daemon_sid = $daemonSid; runner_key_id = 'ci-' + [guid]::NewGuid().ToString('N'); runner_public_key_file = $publicKeyPath }; profiles = @{ emote = @{ version = 1; on_task_completed = $command; on_batch_completed = $command }; crash = @{ version = 1; on_task_completed = $crashCommand } } }
    $registry | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $registryPath -Encoding utf8NoBOM
    $probePath = Join-Path $testRoot 'config/acl-probe.json'
    @{ read_files = @($registryPath, $privateKeyPath, $publicKeyPath, $publisherExe, (Join-Path $testRoot 'business/task/00/1.png'), (Join-Path $testRoot 'business/refs/arknights/icons/one.png')); denied_read_files = @($databasePath, $managementKeyPath, (Join-Path $testRoot 'business/A1/private-attempt/secret.json')); denied_write_files = @($registryPath, $publisherExe, (Join-Path $testRoot 'bin/coord.exe')); write_directories = @((Join-Path $testRoot 'scratch'), (Join-Path $testRoot 'logs'), (Join-Path $testRoot 'business/results')); denied_write_directories = @((Join-Path $testRoot 'protected'), (Join-Path $testRoot 'config'), (Join-Path $testRoot 'bin'), $env:windir); denied_rpc_url = $daemonUrl } | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $probePath -Encoding utf8NoBOM
    Run-AsRunner 'identity' '--check-identity'
    & icacls.exe $testRoot /T | Set-Content -LiteralPath (Join-Path $evidenceRoot 'fixture-acls.log')
    if ($LASTEXITCODE -ne 0) { throw 'ACL evidence capture failed.' }
    $summary.phase = 'start-localservice-core-and-check-private-key-denial'
    $localServiceScript = Join-Path $testRoot 'config/localservice-core.ps1'
    $coreCredentialReport = Join-Path $testRoot 'logs/core-credential-boundary.json'
    $daemonStartupReport = Join-Path $testRoot 'logs/daemon-startup.json'
    $daemonStdout = Join-Path $testRoot 'logs/daemon.stdout.log'
    $daemonStderr = Join-Path $testRoot 'logs/daemon.stderr.log'
    $localServiceBody = @'
$ErrorActionPreference = 'Stop'
$privateReadable = $false
$publicReadable = $false
$startup = @{ phase = 'identity_probe' }
try { $stream = [IO.File]::OpenRead('__PRIVATE__'); $stream.Dispose(); $privateReadable = $true } catch {}
try { $stream = [IO.File]::OpenRead('__PUBLIC__'); $stream.Dispose(); $publicReadable = $true } catch {}
$identity = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$report = @{ sid = $identity; private_key_read = $privateReadable; public_key_read = $publicReadable }
[IO.File]::WriteAllText('__REPORT__', ($report | ConvertTo-Json -Compress), [Text.UTF8Encoding]::new($false))
$startup.identity = $identity
$startup.private_key_read = $privateReadable
$startup.public_key_read = $publicReadable
try {
    if ($identity -ne 'S-1-5-19' -or $privateReadable -or -not $publicReadable) {
        $startup.phase = 'identity_gate_failed'
        $startup.exit_code = 7
        [IO.File]::WriteAllText('__STARTUP_REPORT__', ($startup | ConvertTo-Json -Compress), [Text.UTF8Encoding]::new($false))
        exit 7
    }
    $startup.phase = 'core_version_probe'
    $versionOutput = @(& '__COORD__' --version 2>&1 | ForEach-Object { [string]$_ })
    $startup.version_exit_code = $LASTEXITCODE
    $startup.version_output = @($versionOutput | Select-Object -First 4)
    [IO.File]::WriteAllText('__STARTUP_REPORT__', ($startup | ConvertTo-Json -Compress), [Text.UTF8Encoding]::new($false))
    if ($startup.version_exit_code -ne 0) { throw 'coord version probe returned a nonzero exit code' }
    $startup.phase = 'serve_running'
    [IO.File]::WriteAllText('__STARTUP_REPORT__', ($startup | ConvertTo-Json -Compress), [Text.UTF8Encoding]::new($false))
    & '__COORD__' serve --addr '__DAEMON_ADDR__' --db '__DATABASE__' --actions-config '__REGISTRY__' 1> '__DAEMON_STDOUT__' 2> '__DAEMON_STDERR__'
    $startup.phase = 'serve_returned'
    $startup.exit_code = $LASTEXITCODE
    [IO.File]::WriteAllText('__STARTUP_REPORT__', ($startup | ConvertTo-Json -Compress), [Text.UTF8Encoding]::new($false))
    exit $LASTEXITCODE
} catch {
    $startup.phase = 'powershell_exception'
    $startup.exception_type = $_.Exception.GetType().FullName
    $startup.exception_message = [string]$_.Exception.Message
    $startup.error_id = [string]$_.FullyQualifiedErrorId
    $startup.exit_code = 98
    [IO.File]::WriteAllText('__STARTUP_REPORT__', ($startup | ConvertTo-Json -Compress), [Text.UTF8Encoding]::new($false))
    exit 98
}
'@
    $localServiceBody = $localServiceBody.Replace('__PRIVATE__', $privateKeyPath).Replace('__PUBLIC__', $publicKeyPath).Replace('__REPORT__', $coreCredentialReport).Replace('__STARTUP_REPORT__', $daemonStartupReport).Replace('__COORD__', (Join-Path $testRoot 'bin/coord.exe')).Replace('__DAEMON_ADDR__', ('127.0.0.1:' + $daemonPort)).Replace('__DATABASE__', $databasePath).Replace('__REGISTRY__', $registryPath).Replace('__DAEMON_STDOUT__', $daemonStdout).Replace('__DAEMON_STDERR__', $daemonStderr)
    [IO.File]::WriteAllText($localServiceScript, $localServiceBody, [Text.UTF8Encoding]::new($false))
    $windowsPowerShell = Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/powershell.exe'
    $taskAction = New-ScheduledTaskAction -Execute $windowsPowerShell -Argument ('-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + $localServiceScript + '"')
    $taskPrincipal = New-ScheduledTaskPrincipal -UserId 'NT AUTHORITY\LOCAL SERVICE' -LogonType ServiceAccount -RunLevel Limited
    Register-ScheduledTask -TaskName $daemonTaskName -Action $taskAction -Principal $taskPrincipal -Force | Out-Null
    $taskStartedUtc = [DateTime]::UtcNow
    Start-ScheduledTask -TaskName $daemonTaskName
    $ready = $false
    $lastHealthz = $null
    $wellKnownAvailable = $false
    $lastWellKnownExceptionType = $null
    $lastHealthExceptionType = $null
    foreach ($attempt in 1..100) {
        if (Test-Path -LiteralPath $coreCredentialReport) {
            $coreBoundary = Get-Content -LiteralPath $coreCredentialReport -Raw | ConvertFrom-Json
            if ($coreBoundary.sid -ne 'S-1-5-19' -or $coreBoundary.private_key_read -or -not $coreBoundary.public_key_read) { throw 'LocalService key-isolation probe failed.' }
            $summary.localservice_core = $coreBoundary
            try {
                Invoke-RestMethod ($daemonUrl + '/.well-known/agent.json') -TimeoutSec 1 | Out-Null
                $wellKnownAvailable = $true
            } catch { $lastWellKnownExceptionType = $_.Exception.GetType().FullName }
            try {
                $lastHealthz = Invoke-RestMethod ($daemonUrl + '/healthz') -TimeoutSec 1
                if ($wellKnownAvailable -and $lastHealthz.core.status -eq 'READY' -and $lastHealthz.database.status -eq 'READY' -and $lastHealthz.broker.status -eq 'READY') { $ready = $true; break }
            } catch { $lastHealthExceptionType = $_.Exception.GetType().FullName }
        }
        Start-Sleep -Milliseconds 100
    }
    if (-not $ready) {
        $taskInfo = Get-ScheduledTaskInfo -TaskName $daemonTaskName -ErrorAction SilentlyContinue
        $taskState = (Get-ScheduledTask -TaskName $daemonTaskName -ErrorAction SilentlyContinue).State
        $corePath = Join-Path $testRoot 'bin/coord.exe'
        $coreProcessIds = @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq $corePath -and $_.CommandLine -like ('*' + $databasePath + '*') } | Select-Object -ExpandProperty ProcessId)
        $listeners = @(Get-NetTCPConnection -State Listen -LocalPort $daemonPort,$brokerPort -ErrorAction SilentlyContinue | Select-Object LocalAddress,LocalPort,OwningProcess)
        $healthSnapshot = $null
        if ($lastHealthz) {
            $healthSnapshot = @{ status = $lastHealthz.status; core = $lastHealthz.core; database = $lastHealthz.database; broker = $lastHealthz.broker; runner = $lastHealthz.runner }
        }
        $startupLogs = @()
        foreach ($log in @(@{ stream = 'stdout'; path = $daemonStdout }, @{ stream = 'stderr'; path = $daemonStderr })) {
            if (Test-Path -LiteralPath $log.path -PathType Leaf) {
                $logItem = Get-Item -LiteralPath $log.path
                $logText = Get-Content -LiteralPath $log.path -Raw
                $safeLogText = $logText -replace '(?i)(authorization\s*:\s*bearer\s+)\S+', '$1<redacted>'
                $safeLogText = $safeLogText -replace '(?i)((?:management|broker|private|secret|rpc)[-_ ]?(?:key|token)\s*[:=]\s*)\S+', '$1<redacted>'
                $safeLogText = $safeLogText -replace '\b[A-Fa-f0-9]{64,}\b', '<redacted-long-hex>'
                $safeLogText = $safeLogText -replace '\b[A-Za-z0-9_-]{80,}\b', '<redacted-long-token>'
                $safeTail = @($safeLogText -split '\r?\n' | Select-Object -Last 30)
                $startupLogs += @{ stream = $log.stream; bytes = $logItem.Length; tail = $safeTail }
            }
        }
        $startupDetails = $null
        if (Test-Path -LiteralPath $daemonStartupReport -PathType Leaf) {
            try { $startupDetails = Get-Content -LiteralPath $daemonStartupReport -Raw | ConvertFrom-Json } catch { $startupDetails = @{ parse_error = $_.Exception.GetType().FullName } }
        }
        if ($startupDetails) {
            $safeStartupDetails = ConvertTo-Json -InputObject $startupDetails -Depth 10 -Compress
            $safeStartupDetails = $safeStartupDetails -replace '(?i)(authorization\s*:\s*bearer\s+)\S+', '$1<redacted>'
            $safeStartupDetails = $safeStartupDetails -replace '(?i)((?:management|broker|private|secret|rpc)[-_ ]?(?:key|token)\s*[:=]\s*)\S+', '$1<redacted>'
            $safeStartupDetails = $safeStartupDetails -replace '\b[A-Fa-f0-9]{64,}\b', '<redacted-long-hex>'
            $safeStartupDetails = $safeStartupDetails -replace '\b[A-Za-z0-9_-]{80,}\b', '<redacted-long-token>'
            $startupDetails = $safeStartupDetails | ConvertFrom-Json
        }
        $applicationEvents = @()
        $eventStart = $taskStartedUtc.ToLocalTime().AddSeconds(-2)
        foreach ($event in @(Get-WinEvent -FilterHashtable @{ LogName = 'Application'; StartTime = $eventStart } -ErrorAction SilentlyContinue | Where-Object { $_.ProviderName -in @('Application Error', 'Windows Error Reporting', '.NET Runtime', 'SideBySide') } | Sort-Object TimeCreated -Descending | Select-Object -First 12)) {
            $message = [string]$event.Message
            $message = $message -replace '(?i)(authorization\s*:\s*bearer\s+)\S+', '$1<redacted>'
            $message = $message -replace '(?i)((?:management|broker|private|secret|rpc)[-_ ]?(?:key|token)\s*[:=]\s*)\S+', '$1<redacted>'
            $message = $message -replace '\b[A-Fa-f0-9]{64,}\b', '<redacted-long-hex>'
            $message = $message -replace '\b[A-Za-z0-9_-]{80,}\b', '<redacted-long-token>'
            if ($message.Length -gt 1600) { $message = $message.Substring(0, 1600) }
            $applicationEvents += @{ time_utc = $event.TimeCreated.ToUniversalTime().ToString('o'); provider = $event.ProviderName; id = $event.Id; level = $event.LevelDisplayName; message = $message }
        }
        $summary.localservice_startup = @{ observed_at_utc = [DateTime]::UtcNow.ToString('o'); task_started_utc = $taskStartedUtc.ToString('o'); task_state = [string]$taskState; last_task_result = if ($taskInfo) { $taskInfo.LastTaskResult } else { $null }; core_process_ids = $coreProcessIds; listeners = $listeners; well_known_available = $wellKnownAvailable; well_known_exception_type = $lastWellKnownExceptionType; healthz = $healthSnapshot; healthz_exception_type = $lastHealthExceptionType; startup_details = $startupDetails; application_events = $applicationEvents; logs = $startupLogs }
        throw 'LocalService Core/Broker did not reach ready state with private-key access denied.'
    }
    $daemonCore = Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq (Join-Path $testRoot 'bin/coord.exe') -and $_.CommandLine -like ('*' + $databasePath + '*') } | Select-Object -First 1
    if (-not $daemonCore) { throw 'LocalService Core process was not found for the exact fixture database.' }
    $daemonCoreProcessId = [int]$daemonCore.ProcessId
    $summary.localservice_core = $coreBoundary
    Run-AsRunner 'acl-policy' ('--probe-acl "' + $probePath + '"')
    $summary.phase = 'create-isolated-publish-task'
    $batch = Rpc 'batches/create' @{ name = 'R5-1B disposable low privilege runner'; config = @{ action_profile = 'emote' } }
    Rpc 'batches/add_tasks' @{ batch_id = $batch.id; tasks = @(@{ key = '00'; name = 'disposable test'; payload = @{ group_id = '00'; source_image_ids = @('1') } }) } | Out-Null
    $task = (Rpc 'scheduler/claim' @{ batch_id = $batch.id; agent_id = 'emote-0000'; lease_seconds = 30 }).task
    $result = @{ schema_version = 1; group_id = '00'; agent_id = 'emote-0000'; attempt_no = $task.attempt_no; source_image_ids = @('1'); characters = @(); uncertain = @(@{ image_id = '1'; visible_features = 'Visible disposable character evidence'; reference_candidates = @(); checked_paths = @('refs/arknights/icons/one.png'); check_conclusion = 'Checked disposable reference without match'; reason = 'Insufficient visual identity information'; review_priority = 'high' }); general_notes = '' }
    Rpc 'scheduler/submit' @{ id = $task.id; agent_id = 'emote-0000'; lease_token = $task.lease_token; result = $result } | Out-Null
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
    $summary.phase = 'runner-crash-fixture'
    $crashBatch = Rpc 'batches/create' @{ name = 'R5-1B disposable non-idempotent crash'; config = @{ action_profile = 'crash' } }
    Rpc 'batches/add_tasks' @{ batch_id = $crashBatch.id; tasks = @(@{ key = 'one'; name = 'crash fixture'; payload = @{} }) } | Out-Null
    $crashTask = (Rpc 'scheduler/claim' @{ batch_id = $crashBatch.id; agent_id = 'A0'; lease_seconds = 30 }).task
    Rpc 'scheduler/submit' @{ id = $crashTask.id; agent_id = 'A0'; lease_token = $crashTask.lease_token; result = @{ ok = $true } } | Out-Null
    $runnerProcess = Start-Process -FilePath $runnerExe -ArgumentList ('--registry "' + $registryPath + '" --private-key-file "' + $privateKeyPath + '" --once') -WorkingDirectory $testRoot -Credential $credential -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $testRoot 'logs/crash-runner.stderr.log') -RedirectStandardOutput (Join-Path $testRoot 'logs/crash-runner.stdout.log')
    $childProcess = $null
    foreach ($attempt in 1..100) {
        $childProcess = Get-CimInstance Win32_Process -Filter ('ParentProcessId=' + $runnerProcess.Id) | Where-Object ExecutablePath -eq $pingExe | Select-Object -First 1
        if ($childProcess) { break }; Start-Sleep -Milliseconds 100
    }
    if (-not $childProcess) { throw 'Pinned non-idempotent child never started.' }
    $crashAction = (Rpc 'actions/status' @{ batch_id = $crashBatch.id }).actions | Select-Object -First 1
    if ($crashAction.state -ne 'running') { throw 'Crash fixture action is not running.' }
    # -Credential made this exact owned PID a different user's process. Force
    # skips PowerShell's owner/interactive-confirmation path; Kill still runs.
    Stop-Process -Id $runnerProcess.Id -Force -Confirm:$false
    if (-not $runnerProcess.WaitForExit(10000)) { throw 'Disposable crash runner did not exit.' }
    foreach ($attempt in 1..50) { if (-not (Get-Process -Id $childProcess.ProcessId -ErrorAction SilentlyContinue)) { break }; Start-Sleep -Milliseconds 100 }
    if (Get-Process -Id $childProcess.ProcessId -ErrorAction SilentlyContinue) { throw 'Runner crash failed to kill its Job Object child.' }
    $expirePath = Join-Path $testRoot 'config/expire-fixture-lease.py'
    @'
import sqlite3, sys
with sqlite3.connect(sys.argv[1]) as db:
    changed = db.execute("UPDATE completion_actions SET lease_until='2000-01-01T00:00:00Z' WHERE id=? AND state='running'", (sys.argv[2],)).rowcount
    assert changed == 1
'@ | Set-Content -LiteralPath $expirePath -Encoding utf8NoBOM
    & python $expirePath $databasePath $crashAction.id
    if ($LASTEXITCODE -ne 0) { throw 'Disposable lease expiry injection failed.' }
    Run-AsRunner 'crash-recovery' '--once'
    $recovered = (Rpc 'actions/status' @{ batch_id = $crashBatch.id }).actions | Select-Object -First 1
    if ($recovered.state -ne 'uncertain' -or $recovered.attempt_count -ne 1) { throw 'Non-idempotent crash was incorrectly replayed.' }
    if ((Rpc 'tasks/get' @{ id = $crashTask.id }).state -ne 'completed') { throw 'Action crash reopened task state.' }
    $summary.crash_recovery = @{ state = $recovered.state; attempt_count = $recovered.attempt_count; child_killed_on_runner_exit = $true; isolated_forced_lease_expiry = $true }
    $summary.state = 'PASS'
    $summary.phase = 'completed'
} catch {
    $summary.failed_phase = $summary.phase
    $summary.failure = @{ phase = $summary.phase; error = $_.Exception.Message; exception_type = $_.Exception.GetType().FullName }
    throw
} finally {
    $cleanupFailures = [Collections.Generic.List[object]]::new()
    function Add-CleanupFailure([string]$Phase, [System.Management.Automation.ErrorRecord]$Failure) {
        $script:cleanupFailures.Add(@{ phase = $Phase; error = $Failure.Exception.Message })
        $script:summary.state = 'FAIL'
        if (-not $script:summary.failed_phase) { $script:summary.failed_phase = $Phase }
    }
    try {
        foreach ($ownedProcess in @($runnerProcess)) {
            try {
                if ($ownedProcess -and -not $ownedProcess.HasExited) {
                    Stop-Process -Id $ownedProcess.Id -Force -Confirm:$false
                    if (-not $ownedProcess.WaitForExit(10000)) { throw 'Disposable process did not exit during cleanup.' }
                }
            } catch { Add-CleanupFailure 'cleanup-stop-process' $_ }
        }
        try {
            $task = Get-ScheduledTask -TaskName $daemonTaskName -ErrorAction SilentlyContinue
            if ($task) {
                if ($task.State -eq 'Running') { Stop-ScheduledTask -TaskName $daemonTaskName }
                Unregister-ScheduledTask -TaskName $daemonTaskName -Confirm:$false
            }
            $summary.localservice_task_removed = -not (Get-ScheduledTask -TaskName $daemonTaskName -ErrorAction SilentlyContinue)
            if (-not $summary.localservice_task_removed) { throw 'LocalService Core task remains after cleanup.' }
        } catch { Add-CleanupFailure 'cleanup-localservice-task' $_ }
        try {
            if ($daemonCoreProcessId) {
                $ownedCore = Get-CimInstance Win32_Process -Filter ('ProcessId=' + $daemonCoreProcessId)
                if ($ownedCore -and $ownedCore.ExecutablePath -eq (Join-Path $testRoot 'bin/coord.exe') -and $ownedCore.CommandLine -like ('*' + $databasePath + '*')) {
                    Stop-Process -Id $daemonCoreProcessId -Force -Confirm:$false
                    for ($attempt = 0; $attempt -lt 100 -and (Get-Process -Id $daemonCoreProcessId -ErrorAction SilentlyContinue); $attempt++) { Start-Sleep -Milliseconds 100 }
                }
                if (Get-Process -Id $daemonCoreProcessId -ErrorAction SilentlyContinue) { throw 'Exact LocalService Core process did not exit during cleanup.' }
            }
            $fixtureCorePath = Join-Path $testRoot 'bin/coord.exe'
            $fixtureCoreProcesses = @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq $fixtureCorePath -and $_.CommandLine -like ('*' + $databasePath + '*') })
            foreach ($fixtureCore in $fixtureCoreProcesses) { Stop-Process -Id $fixtureCore.ProcessId -Force -Confirm:$false }
            if (@(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq $fixtureCorePath -and $_.CommandLine -like ('*' + $databasePath + '*') }).Count -gt 0) { throw 'Exact disposable Core process remained after task cleanup.' }
        } catch { Add-CleanupFailure 'cleanup-localservice-core-process' $_ }
        if ($createdSid) {
            try {
                $currentUser = Get-LocalUser -Name $userName -ErrorAction SilentlyContinue
                if ($currentUser) {
                    if ($currentUser.SID.Value -ne $createdSid -or $userName -notmatch '^coordT[0-9a-f]{10}$') { throw 'Refusing account cleanup after disposable identity mismatch.' }
                    Remove-LocalUser -SID $createdSid
                }
                $summary.disposable_account_removed = -not (Get-LocalUser -SID $createdSid -ErrorAction SilentlyContinue)
                if (-not $summary.disposable_account_removed) { throw 'Disposable account remains after cleanup.' }
            } catch { Add-CleanupFailure 'cleanup-remove-account' $_ }
        }
        # Preserve both failure sources before deleting the exact fixture. Each
        # cleanup step remains independent so one error cannot suppress summary.
        foreach ($logName in @('crash-runner.stderr.log', 'core-credential-boundary.json', 'keygen.stderr.log')) {
            try {
                $source = Join-Path $testRoot ('logs/' + $logName)
                if (Test-Path -LiteralPath $source) { Copy-Item -LiteralPath $source -Destination (Join-Path $evidenceRoot $logName) }
            } catch { Add-CleanupFailure ('cleanup-save-' + $logName) $_ }
        }
        try {
            $resolvedRoot = [IO.Path]::GetFullPath($testRoot)
            $resolvedParent = [IO.Path]::GetDirectoryName($resolvedRoot)
            if (-not $resolvedParent.Equals($systemVolumeRoot, [StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($resolvedRoot) -notmatch '^coord-r5-broker-[0-9a-f]{32}$') { throw 'Refusing cleanup outside the exact verified system-volume fixture root.' }
            if (Test-Path -LiteralPath $resolvedRoot) { Remove-Item -LiteralPath $resolvedRoot -Recurse -Force }
            $summary.disposable_fixture_removed = -not (Test-Path -LiteralPath $resolvedRoot)
            if (-not $summary.disposable_fixture_removed) { throw 'Disposable fixture remains after cleanup.' }
        } catch { Add-CleanupFailure 'cleanup-remove-fixture' $_ }
    } finally {
        $summary.cleanup_errors = @($cleanupFailures.ToArray())
        $summary | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath (Join-Path $evidenceRoot 'summary.json') -Encoding utf8NoBOM
    }
    if ($cleanupFailures.Count -gt 0) { throw ('Disposable cleanup failed: ' + (($cleanupFailures | ForEach-Object { $_.phase }) -join ', ')) }
}

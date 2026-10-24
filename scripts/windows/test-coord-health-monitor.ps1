[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$CoordExe
)

$ErrorActionPreference = 'Stop'
$CoordExe = (Resolve-Path -LiteralPath $CoordExe).Path
$monitor = Join-Path $PSScriptRoot 'coord-health-monitor.ps1'
$windowsPowerShell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
if (-not (Test-Path -LiteralPath $monitor -PathType Leaf) -or -not (Test-Path -LiteralPath $windowsPowerShell -PathType Leaf)) {
    throw 'Windows PowerShell 5.1 and coord-health-monitor.ps1 are required'
}

$tempRoot = Join-Path $env:TEMP "coord-health-monitor-$([Guid]::NewGuid().ToString('N'))"
$tempRoot = [IO.Path]::GetFullPath($tempRoot)
$tempPrefix = [IO.Path]::GetFullPath($env:TEMP).TrimEnd('\') + '\'
if (-not $tempRoot.StartsWith($tempPrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'health monitor test directory escaped TEMP'
}
New-Item -ItemType Directory -Path $tempRoot | Out-Null
# The key fixture must satisfy the same protected-file policy. Do not rely on
# the operator's TEMP inheritance (which may intentionally grant other SIDs).
$fixtureAcl = Get-Acl -LiteralPath $tempRoot
$fixtureAcl.SetAccessRuleProtection($true, $false)
foreach ($fixtureSid in @('S-1-5-18', 'S-1-5-32-544', [Security.Principal.WindowsIdentity]::GetCurrent().User.Value)) {
    $fixtureAcl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new(
        [Security.Principal.SecurityIdentifier]::new($fixtureSid),
        [Security.AccessControl.FileSystemRights]::FullControl,
        [Security.AccessControl.InheritanceFlags]'ContainerInherit, ObjectInherit',
        [Security.AccessControl.PropagationFlags]::None,
        [Security.AccessControl.AccessControlType]::Allow
    ))
}
Set-Acl -LiteralPath $tempRoot -AclObject $fixtureAcl
$daemonProcess = $null
$previousRpcAuthKey = [Environment]::GetEnvironmentVariable('COORD_RPC_AUTH_KEY', 'Process')

try {
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $listener.Start()
    $port = $listener.LocalEndpoint.Port
    $listener.Stop()

    $fakeServy = Join-Path $tempRoot 'fake-servy-cli.cmd'
    $callsPath = Join-Path $tempRoot 'calls.log'
    $statusPath = Join-Path $tempRoot 'status.json'
    $recoveryPath = Join-Path $tempRoot 'recovery.json'
    $databasePath = Join-Path $tempRoot 'missing.db'
    $fakeServyBody = @'
@echo off
>>"%~dp0calls.log" echo %*
if /I "%~1"=="status" echo Service status for 'coord': Stopped
exit /b 0
'@
    [IO.File]::WriteAllText($fakeServy, $fakeServyBody, [Text.Encoding]::ASCII)

    $exitCodes = @()
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        & $windowsPowerShell -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass `
            -File $monitor `
            -CoordExe $CoordExe `
            -CoordUrl "http://127.0.0.1:$port/" `
            -DbPath $databasePath `
            -StatusPath $statusPath `
            -RecoveryPath $recoveryPath `
            -ServyCli $fakeServy `
            -RestartThreshold 3 `
            -MaxRestartsPerHour 3 `
            -RestartCooldownMinutes 1 2>&1 | Out-Null
        $exitCodes += $LASTEXITCODE

        if ($attempt -lt 3 -and (Test-Path -LiteralPath $callsPath) -and (Get-Item -LiteralPath $callsPath).Length -gt 0) {
            throw "monitor requested recovery before failure threshold on attempt $attempt"
        }
    }

    $status = Get-Content -LiteralPath $statusPath -Raw | ConvertFrom-Json
    $recovery = Get-Content -LiteralPath $recoveryPath -Raw | ConvertFrom-Json
    $calls = [IO.File]::ReadAllLines($callsPath)
    $expectedCalls = @('status --name coord --quiet', 'start --name coord --quiet')
    if (@($exitCodes | Where-Object { $_ -ne 1 }).Count -gt 0) {
        throw "expected probe failure exit codes, got $($exitCodes -join ',')"
    }
    if ($status.status -ne 'FAIL' -or [int]$status.consecutive_failures -ne 3) {
        throw 'health status did not retain three consecutive failures'
    }
    if ($recovery.attempts.Count -ne 1 -or $recovery.attempts[0].action -ne 'start' -or [int]$recovery.attempts[0].exit_code -ne 0) {
        throw 'monitor did not record the isolated Servy start request'
    }
    if ($calls.Count -ne 2 -or $calls[0] -ne $expectedCalls[0] -or $calls[1] -ne $expectedCalls[1]) {
        throw "unexpected fake Servy calls: $($calls -join '; ')"
    }

    Write-Output 'health monitor failure recovery threshold: PASS (3 failures -> targeted start)'

    # Key configuration failures must stop before the CLI/status/recovery path.
    $keyFile = Join-Path $tempRoot 'management.key'
    $invalidStatus = Join-Path $tempRoot 'invalid-key-status.json'
    $invalidRecovery = Join-Path $tempRoot 'invalid-key-recovery.json'
    foreach ($keyCase in @('missing', 'malformed', 'oversized', 'relative', 'unprotected')) {
        $configuredKeyFile = $keyFile
        switch ($keyCase) {
            'missing' { $configuredKeyFile = Join-Path $tempRoot 'never-created.key' }
            'malformed' { [IO.File]::WriteAllText($keyFile, 'not-a-key') }
            'oversized' { [IO.File]::WriteAllText($keyFile, ('a' * 129)) }
            'relative' { $configuredKeyFile = 'relative-management.key' }
            'unprotected' {
                [IO.File]::WriteAllText($keyFile, ('ab' * 32))
                $unprotectedAcl = Get-Acl -LiteralPath $keyFile
                $unprotectedAcl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new('S-1-1-0'), [Security.AccessControl.FileSystemRights]::Read, [Security.AccessControl.AccessControlType]::Allow))
                Set-Acl -LiteralPath $keyFile -AclObject $unprotectedAcl
            }
        }
        & $windowsPowerShell -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $monitor `
            -CoordExe $CoordExe -CoordUrl "http://127.0.0.1:$port/" -DbPath $databasePath `
            -StatusPath $invalidStatus -RecoveryPath $invalidRecovery -ServyCli $fakeServy `
            -RpcAuthKeyFile $configuredKeyFile -RestartThreshold 1 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 2 -or (Test-Path -LiteralPath $invalidStatus) -or (Test-Path -LiteralPath $invalidRecovery) -or [IO.File]::ReadAllLines($callsPath).Count -ne 2) {
            throw ('Invalid key configuration reached recovery: ' + $keyCase)
        }
    }
    Write-Output 'health monitor key configuration rejection: PASS (missing/malformed/oversized/relative/unprotected; no probe or recovery)'

    # Real authenticated daemon + CLI: a wrong inherited key must be overridden
    # only during health invocation; the secret is never put in argv or evidence.
    $managementKey = 'ab' * 32
    Remove-Item -LiteralPath $keyFile
    [IO.File]::WriteAllText($keyFile, $managementKey)
    $brokerKeyFile = Join-Path $tempRoot 'broker.key'
    [IO.File]::WriteAllText($brokerKeyFile, ('cd' * 32))
    $listener.Start(); $authPort = $listener.LocalEndpoint.Port; $listener.Stop()
    $listener.Start(); $brokerPort = $listener.LocalEndpoint.Port; $listener.Stop()
    $registryPath = Join-Path $tempRoot 'registry.json'
    $daemonSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $nativeCommand = @{ executable = $CoordExe; sha256 = (Get-FileHash -LiteralPath $CoordExe -Algorithm SHA256).Hash; working_directory = (Split-Path -Parent $CoordExe); args = @('version'); include_result = $false; timeout_ms = 1000; max_output_bytes = 65536; max_attempts = 1; idempotent = $true }
    $registry = @{ version = 1; allowed_root = (Split-Path -Parent $CoordExe); external_runner = @{ broker_addr = "127.0.0.1:$brokerPort"; key_file = $brokerKeyFile; daemon_rpc_key_file = $keyFile; daemon_sid = $daemonSid; runner_sid = 'S-1-5-21-111-222-333-9999' }; profiles = @{ canary = @{ version = 1; on_task_completed = $nativeCommand } } }
    [IO.File]::WriteAllText($registryPath, ($registry | ConvertTo-Json -Depth 20), [Text.UTF8Encoding]::new($false))
    $authDb = Join-Path $tempRoot 'authenticated.db'
    $daemonProcess = Start-Process -FilePath $CoordExe -ArgumentList @('serve', '--addr', "127.0.0.1:$authPort", '--db', ('"' + $authDb + '"'), '--actions-config', ('"' + $registryPath + '"')) -WorkingDirectory $tempRoot -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $tempRoot 'daemon.stderr.log') -RedirectStandardOutput (Join-Path $tempRoot 'daemon.stdout.log')
    $ready = $false
    foreach ($attempt in 1..100) {
        try { Invoke-RestMethod -Uri "http://127.0.0.1:$authPort/.well-known/agent.json" -TimeoutSec 1 | Out-Null; $ready = $true; break } catch { Start-Sleep -Milliseconds 50 }
    }
    if (-not $ready) { throw 'Authenticated fixture daemon did not start.' }
    [Environment]::SetEnvironmentVariable('COORD_RPC_AUTH_KEY', ('ef' * 32), 'Process')
    $authStatus = Join-Path $tempRoot 'authenticated-status.json'
    & $windowsPowerShell -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $monitor `
        -CoordExe $CoordExe -CoordUrl "http://127.0.0.1:$authPort/" -DbPath $authDb `
        -StatusPath $authStatus -RecoveryPath $invalidRecovery -ServyCli $fakeServy `
        -RpcAuthKeyFile $keyFile -NoRestart 2>&1 | Out-Null
    $authenticatedExit = $LASTEXITCODE
    $authenticatedStatus = Get-Content -LiteralPath $authStatus -Raw | ConvertFrom-Json
    if ($authenticatedExit -ne 0 -or $authenticatedStatus.status -ne 'READY' -or $authenticatedStatus.checks.tasks_list -ne 'PASS' -or (Test-Path -LiteralPath $invalidRecovery)) { throw 'Health monitor did not pass its protected key to the real CLI.' }
    Write-Output 'health monitor authenticated CLI probe: PASS (private key file overrides incorrect inherited key; READY)'

    # Servy must observe the prior environment, not the temporary management key.
    $restoreServy = Join-Path $tempRoot 'restore-check-servy.cmd'
    $restoreMarker = Join-Path $tempRoot 'environment-restored.txt'
    $restoreBody = @'
@echo off
if not "%COORD_RPC_AUTH_KEY%"=="efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef" exit /b 9
>"%~dp0environment-restored.txt" echo restored
if /I "%~1"=="status" echo Service status for 'coord': Stopped
exit /b 0
'@
    [IO.File]::WriteAllText($restoreServy, $restoreBody, [Text.Encoding]::ASCII)
    & $windowsPowerShell -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $monitor `
        -CoordExe $CoordExe -CoordUrl "http://127.0.0.1:$port/" -DbPath $databasePath `
        -StatusPath (Join-Path $tempRoot 'restore-status.json') -RecoveryPath (Join-Path $tempRoot 'restore-recovery.json') -ServyCli $restoreServy `
        -RpcAuthKeyFile $keyFile -RestartThreshold 1 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 1 -or -not (Test-Path -LiteralPath $restoreMarker)) { throw 'Health monitor leaked the temporary key into recovery process environment.' }
    Write-Output 'health monitor key environment restoration: PASS (finally restored before fake Servy recovery)'
} finally {
    [Environment]::SetEnvironmentVariable('COORD_RPC_AUTH_KEY', $previousRpcAuthKey, 'Process')
    if ($daemonProcess -and -not $daemonProcess.HasExited) { Stop-Process -Id $daemonProcess.Id; $daemonProcess.WaitForExit() }
    if (Test-Path -LiteralPath $tempRoot) {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force
    }
}

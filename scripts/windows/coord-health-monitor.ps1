[CmdletBinding()]
param(
    [string]$CoordExe = 'C:\ProgramData\coord\bin\coord.exe',
    [string]$CoordUrl = 'http://127.0.0.1:7777/',
    [string]$DbPath = 'C:\ProgramData\coord\data\coord.db',
    [string]$StatusPath = 'C:\ProgramData\coord\health\status.json',
    [string]$RecoveryPath = 'C:\ProgramData\coord\health\recovery.json',
    [string]$ServyCli = 'C:\Program Files\Servy\servy-cli.exe',
    [string]$RpcAuthKeyFile,
    [int]$RestartThreshold = 3,
    [int]$RunnerRestartThreshold = 3,
    [string]$RunnerServiceName = 'CoordActionRunnerG7',
    [int]$MaxRestartsPerHour = 3,
    [int]$RestartCooldownMinutes = 10,
    [switch]$NoRestart
)

$ErrorActionPreference = 'Stop'
if ($RestartThreshold -lt 1 -or $RunnerRestartThreshold -lt 1 -or $MaxRestartsPerHour -lt 1 -or $RestartCooldownMinutes -lt 1) {
    exit 2
}
if (-not (Test-Path -LiteralPath $CoordExe -PathType Leaf)) {
    exit 2
}
$rpcAuthKey = $null
$hasRpcAuthKeyFile = $PSBoundParameters.ContainsKey('RpcAuthKeyFile')
if ($hasRpcAuthKeyFile) {
    try {
        # Invalid or insufficiently protected configuration is an operator
        # configuration error, never evidence that coord needs restarting.
        if ($RpcAuthKeyFile -notmatch '^[A-Za-z]:[\\/]') { exit 2 }
        $keyItem = Get-Item -LiteralPath $RpcAuthKeyFile -Force -ErrorAction Stop
        if ($keyItem.PSIsContainer -or $keyItem.Length -gt 128) { exit 2 }
        $componentPath = $keyItem.FullName
        while ($componentPath) {
            $component = Get-Item -LiteralPath $componentPath -Force -ErrorAction Stop
            if (($component.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { exit 2 }
            $componentPath = [IO.Path]::GetDirectoryName($componentPath)
        }
        $trustedSids = @('S-1-5-18', 'S-1-5-32-544', [Security.Principal.WindowsIdentity]::GetCurrent().User.Value)
        $keyAcl = Get-Acl -LiteralPath $keyItem.FullName -ErrorAction Stop
        if ($trustedSids -notcontains $keyAcl.GetOwner([Security.Principal.SecurityIdentifier]).Value) { exit 2 }
        foreach ($rule in $keyAcl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier])) {
            if ($rule.AccessControlType -eq [Security.AccessControl.AccessControlType]::Allow -and
                ($rule.PropagationFlags -band [Security.AccessControl.PropagationFlags]::InheritOnly) -eq 0 -and
                $trustedSids -notcontains $rule.IdentityReference.Value) { exit 2 }
        }
        $rpcAuthKey = [IO.File]::ReadAllText($keyItem.FullName).Trim()
        if ($rpcAuthKey -cnotmatch '\A[0-9a-fA-F]{64}\z') { exit 2 }
        $parsedUrl = $null
        if (-not [Uri]::TryCreate($CoordUrl, [UriKind]::Absolute, [ref]$parsedUrl) -or
            $parsedUrl.Scheme -ne 'http' -or $parsedUrl.Host -notin @('127.0.0.1', 'localhost', '[::1]', '::1')) { exit 2 }
    } catch {
        exit 2
    }
}
$healthArgs = @(
    '--url', $CoordUrl,
    'health',
    '--db', $DbPath,
    '--status', $StatusPath,
    '--timeout-ms', '2000',
    '--degraded-after-ms', '2000'
)
$previousRpcAuthKey = [Environment]::GetEnvironmentVariable('COORD_RPC_AUTH_KEY', 'Process')
$previousErrorActionPreference = $ErrorActionPreference
try {
    if ($hasRpcAuthKeyFile) { [Environment]::SetEnvironmentVariable('COORD_RPC_AUTH_KEY', $rpcAuthKey, 'Process') }
    # Windows PowerShell 5.1 can promote native stderr to a terminating error.
    $probeStartedAt = [DateTimeOffset]::UtcNow
    $ErrorActionPreference = 'Continue'
    $null = & $CoordExe @healthArgs 2>&1 | Out-Null
    $healthExit = $LASTEXITCODE
} catch {
    $healthExit = 1
} finally {
    $ErrorActionPreference = $previousErrorActionPreference
    [Environment]::SetEnvironmentVariable('COORD_RPC_AUTH_KEY', $previousRpcAuthKey, 'Process')
    $rpcAuthKey = $null
}

if (-not (Test-Path -LiteralPath $StatusPath -PathType Leaf)) {
    exit 2
}
$health = Get-Content -LiteralPath $StatusPath -Raw | ConvertFrom-Json
try {
    $checkedAt = [DateTimeOffset]::FromUnixTimeMilliseconds([long]$health.checked_at_unix_ms)
} catch {
    exit 2
}
if (([DateTimeOffset]::UtcNow - $checkedAt.ToUniversalTime()).TotalSeconds -gt 120) {
    exit 2
}
if ($checkedAt -lt $probeStartedAt) {
    exit 2
}

$coreRecoveryNeeded = $health.status -eq 'FAIL' -and [int]$health.consecutive_failures -ge $RestartThreshold
$runnerRecoveryNeeded = $health.status -eq 'DEGRADED' -and
    $health.components.broker.status -eq 'READY' -and
    $health.components.runner.status -eq 'UNAVAILABLE' -and
    [int]$health.runner_consecutive_failures -ge $RunnerRestartThreshold
if ($NoRestart -or (-not $coreRecoveryNeeded -and -not $runnerRecoveryNeeded)) {
    exit $healthExit
}
$targetService = if ($coreRecoveryNeeded) { 'coord' } else { $RunnerServiceName }

$attempts = @()
if (Test-Path -LiteralPath $RecoveryPath -PathType Leaf) {
    try {
        $saved = Get-Content -LiteralPath $RecoveryPath -Raw | ConvertFrom-Json
        $attempts = @($saved.attempts)
    } catch {
        $attempts = @()
    }
}
$now = [DateTimeOffset]::UtcNow
$recent = @($attempts | Where-Object {
    try {
        $attemptAt = [DateTimeOffset]::FromUnixTimeMilliseconds([long]$_.attempted_at_unix_ms)
        $attemptService = if ($_.service) { [string]$_.service } else { 'coord' }
        $attemptService -eq $targetService -and ($now - $attemptAt).TotalHours -lt 1 -and $attemptAt -le $now
    } catch {
        $false
    }
})
if ($recent.Count -ge $MaxRestartsPerHour) {
    exit $healthExit
}
if ($recent.Count -gt 0) {
    $lastAttempt = [DateTimeOffset]::FromUnixTimeMilliseconds([long]$recent[-1].attempted_at_unix_ms)
    if (($now - $lastAttempt).TotalMinutes -lt $RestartCooldownMinutes) {
        exit $healthExit
    }
}

$previousErrorActionPreference = $ErrorActionPreference
try {
    $ErrorActionPreference = 'Continue'
    $serviceStateOutput = (& $ServyCli status --name $targetService --quiet 2>&1 | Out-String).Trim()
    $serviceStatusExit = $LASTEXITCODE
} finally {
    $ErrorActionPreference = $previousErrorActionPreference
}
if ($serviceStatusExit -ne 0) {
    exit $healthExit
}
$serviceStateLine = ($serviceStateOutput -split '\r?\n' | Where-Object { $_.Trim() } | Select-Object -Last 1).Trim()
$serviceStateMatch = [regex]::Match($serviceStateLine, '^(?:.*:\s*)?(Running|Stopped)$')
if (-not $serviceStateMatch.Success) {
    exit $healthExit
}
$serviceState = $serviceStateMatch.Groups[1].Value
if ($serviceState -eq 'Running') {
    $action = 'restart'
} elseif ($serviceState -eq 'Stopped') {
    $action = 'start'
} else {
    exit $healthExit
}

$previousErrorActionPreference = $ErrorActionPreference
try {
    $ErrorActionPreference = 'Continue'
    $null = & $ServyCli $action --name $targetService --quiet 2>&1 | Out-Null
    $recoveryExit = $LASTEXITCODE
} catch {
    $recoveryExit = 1
} finally {
    $ErrorActionPreference = $previousErrorActionPreference
}
$recent += [pscustomobject]@{
    attempted_at_utc = $now.ToString('o')
    attempted_at_unix_ms = $now.ToUnixTimeMilliseconds()
    service = $targetService
    action = $action
    exit_code = $recoveryExit
}
$recent = @($recent | Select-Object -Last 12)
$recoveryJson = [pscustomobject]@{ attempts = $recent } | ConvertTo-Json -Depth 4 -Compress
$recoveryDirectory = Split-Path -Parent $RecoveryPath
New-Item -ItemType Directory -Path $recoveryDirectory -Force | Out-Null
$tempPath = "$RecoveryPath.$PID.tmp"
[System.IO.File]::WriteAllText($tempPath, $recoveryJson + [Environment]::NewLine, [System.Text.UTF8Encoding]::new($false))
Move-Item -LiteralPath $tempPath -Destination $RecoveryPath -Force

exit $healthExit

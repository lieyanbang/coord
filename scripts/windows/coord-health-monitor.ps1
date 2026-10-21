[CmdletBinding()]
param(
    [string]$CoordExe = 'C:\ProgramData\coord\bin\coord.exe',
    [string]$CoordUrl = 'http://127.0.0.1:7777/',
    [string]$DbPath = 'C:\ProgramData\coord\data\coord.db',
    [string]$StatusPath = 'C:\ProgramData\coord\health\status.json',
    [string]$RecoveryPath = 'C:\ProgramData\coord\health\recovery.json',
    [string]$ServyCli = 'C:\Program Files\Servy\servy-cli.exe',
    [int]$RestartThreshold = 3,
    [int]$MaxRestartsPerHour = 3,
    [int]$RestartCooldownMinutes = 10,
    [switch]$NoRestart
)

$ErrorActionPreference = 'Stop'
if ($RestartThreshold -lt 1 -or $MaxRestartsPerHour -lt 1 -or $RestartCooldownMinutes -lt 1) {
    exit 2
}
if (-not (Test-Path -LiteralPath $CoordExe -PathType Leaf)) {
    exit 2
}
$healthArgs = @(
    '--url', $CoordUrl,
    'health',
    '--db', $DbPath,
    '--status', $StatusPath,
    '--timeout-ms', '2000',
    '--degraded-after-ms', '2000'
)
try {
    # Windows PowerShell 5.1 can promote native stderr to a terminating error.
    $probeStartedAt = [DateTimeOffset]::UtcNow
    $previousErrorActionPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $null = & $CoordExe @healthArgs 2>&1 | Out-Null
    $healthExit = $LASTEXITCODE
} catch {
    $healthExit = 1
} finally {
    $ErrorActionPreference = $previousErrorActionPreference
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

if ($NoRestart -or $health.status -ne 'FAIL' -or [int]$health.consecutive_failures -lt $RestartThreshold) {
    exit $healthExit
}

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
        ($now - $attemptAt).TotalHours -lt 1 -and $attemptAt -le $now
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
    $serviceStateOutput = (& $ServyCli status --name coord --quiet 2>&1 | Out-String).Trim()
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
    $null = & $ServyCli $action --name coord --quiet 2>&1 | Out-Null
    $recoveryExit = $LASTEXITCODE
} catch {
    $recoveryExit = 1
} finally {
    $ErrorActionPreference = $previousErrorActionPreference
}
$recent += [pscustomobject]@{
    attempted_at_utc = $now.ToString('o')
    attempted_at_unix_ms = $now.ToUnixTimeMilliseconds()
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

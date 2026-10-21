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
} finally {
    if (Test-Path -LiteralPath $tempRoot) {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force
    }
}

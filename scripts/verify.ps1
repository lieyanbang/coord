param([Parameter(Mandatory)][ValidatePattern('^[a-z0-9-]+$')][string]$Stage)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path $PSScriptRoot -Parent
Push-Location $repoRoot
try {
    $checks = @(
        @{ Name='fmt'; Args=@('fmt','--all','--','--check') },
        @{ Name='clippy'; Args=@('clippy','--all-targets','--all-features','--locked','--','-D','warnings') },
        @{ Name='test'; Args=@('test','--all-features','--locked') },
        @{ Name='build'; Args=@('build','--release','--locked') }
    )
    $results = @()
    foreach ($check in $checks) {
        $log = "docs/verification/stage-$Stage-$($check.Name).log"
        $started = [DateTime]::UtcNow
        $cargoArgs = $check.Args
        & cargo @cargoArgs 2>&1 | ForEach-Object {
            $_.ToString().Replace($repoRoot,'<workspace>').Replace($env:USERPROFILE,'<user>')
        } | Set-Content -Encoding utf8 $log
        $code = $LASTEXITCODE
        $results += @{command=('cargo '+($cargoArgs -join ' '));exit=$code;started=$started.ToString('o');seconds=([DateTime]::UtcNow-$started).TotalSeconds;log=$log}
        $results | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 "docs/verification/stage-$Stage-checks.json"
        Write-Output "$Stage $($check.Name): exit=$code"
        if ($code -ne 0) { throw "$($check.Name) failed; inspect $log" }
    }
    if ($Stage -eq 'f1' -and $env:OS -eq 'Windows_NT') {
        $log = "docs/verification/stage-$Stage-health-monitor.log"
        $started = [DateTime]::UtcNow
        $windowsPowerShell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
        $coordExe = (Resolve-Path -LiteralPath 'target/release/coord.exe').Path
        $testScript = 'scripts/windows/test-coord-health-monitor.ps1'
        & $windowsPowerShell -NoLogo -NoProfile -NonInteractive -File $testScript -CoordExe $coordExe 2>&1 | ForEach-Object {
            $_.ToString().Replace($repoRoot,'<workspace>').Replace($env:USERPROFILE,'<user>')
        } | Set-Content -Encoding utf8 $log
        $code = $LASTEXITCODE
        $results += @{command=('powershell.exe -NoLogo -NoProfile -File '+$testScript);exit=$code;started=$started.ToString('o');seconds=([DateTime]::UtcNow-$started).TotalSeconds;log=$log}
        $results | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 "docs/verification/stage-$Stage-checks.json"
        Write-Output "$Stage health-monitor: exit=$code"
        if ($code -ne 0) { throw "health-monitor test failed; inspect $log" }
    }
} finally { Pop-Location }

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
        [System.IO.File]::WriteAllText($log, '', [System.Text.Encoding]::UTF8)
        $previousErrorActionPreference = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try {
            & cargo @cargoArgs 2>&1 | ForEach-Object {
                $_.ToString().Replace($repoRoot,'<workspace>').Replace($env:USERPROFILE,'<user>')
            } | Set-Content -Encoding utf8 $log
            $code = $LASTEXITCODE
        } finally {
            $ErrorActionPreference = $previousErrorActionPreference
        }
        $results += @{command=('cargo '+($cargoArgs -join ' '));exit=$code;started=$started.ToString('o');seconds=([DateTime]::UtcNow-$started).TotalSeconds;log=$log}
        $results | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 "docs/verification/stage-$Stage-checks.json"
        Write-Output "$Stage $($check.Name): exit=$code"
        if ($code -ne 0) { throw "$($check.Name) failed; inspect $log" }
    }
    if (($Stage -eq 'f1' -or $Stage -eq 'r5-1b' -or $Stage -match '^g[67](?:-[a-z0-9]+)*$') -and $env:OS -eq 'Windows_NT') {
        $log = "docs/verification/stage-$Stage-health-monitor.log"
        $started = [DateTime]::UtcNow
        $windowsPowerShell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
        $coordExe = (Resolve-Path -LiteralPath 'target/release/coord.exe').Path
        $testScript = 'scripts/windows/test-coord-health-monitor.ps1'
        $previousErrorActionPreference = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try {
            & $windowsPowerShell -NoLogo -NoProfile -NonInteractive -File $testScript -CoordExe $coordExe 2>&1 | ForEach-Object {
                $_.ToString().Replace($repoRoot,'<workspace>').Replace($env:USERPROFILE,'<user>')
            } | Set-Content -Encoding utf8 $log
            $code = $LASTEXITCODE
        } finally {
            $ErrorActionPreference = $previousErrorActionPreference
        }
        $results += @{command=('powershell.exe -NoLogo -NoProfile -File '+$testScript);exit=$code;started=$started.ToString('o');seconds=([DateTime]::UtcNow-$started).TotalSeconds;log=$log}
        $results | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 "docs/verification/stage-$Stage-checks.json"
        Write-Output "$Stage health-monitor: exit=$code"
        if ($code -ne 0) { throw "health-monitor test failed; inspect $log" }
    }
    if ($Stage -eq 'f2' -and $env:OS -eq 'Windows_NT') {
        $log = "docs/verification/stage-$Stage-relay-tests.log"
        $started = [DateTime]::UtcNow
        $node = (Get-Command node.exe -ErrorAction Stop).Source
        $previousCoordTestBin = $env:COORD_TEST_BIN
        $env:COORD_TEST_BIN = (Resolve-Path -LiteralPath 'target/release/coord.exe').Path
        try {
            & $node --test relay/relay.test.mjs relay/live-coord.test.mjs 2>&1 | ForEach-Object {
                $_.ToString().Replace($repoRoot,'<workspace>').Replace($env:USERPROFILE,'<user>')
            } | Set-Content -Encoding utf8 $log
            $code = $LASTEXITCODE
        } finally {
            $env:COORD_TEST_BIN = $previousCoordTestBin
        }
        $results += @{command=('node --test relay/relay.test.mjs relay/live-coord.test.mjs');exit=$code;started=$started.ToString('o');seconds=([DateTime]::UtcNow-$started).TotalSeconds;log=$log}
        $results | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 "docs/verification/stage-$Stage-checks.json"
        Write-Output "$Stage relay-tests: exit=$code"
        if ($code -ne 0) { throw "relay tests failed; inspect $log" }
    }
    if ($Stage -in @('f3','f3-r') -and $env:OS -eq 'Windows_NT') {
        $log = "docs/verification/stage-$Stage-coord-mcp-tests.log"
        $started = [DateTime]::UtcNow
        $npm = (Get-Command npm.cmd -ErrorAction Stop).Source
        & $npm test --prefix relay/coord-mcp 2>&1 | ForEach-Object {
            $_.ToString().Replace($repoRoot,'<workspace>').Replace($env:USERPROFILE,'<user>')
        } | Set-Content -Encoding utf8 $log
        $code = $LASTEXITCODE
        $results += @{command=('npm test --prefix relay/coord-mcp');exit=$code;started=$started.ToString('o');seconds=([DateTime]::UtcNow-$started).TotalSeconds;log=$log}
        $results | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 "docs/verification/stage-$Stage-checks.json"
        Write-Output "$Stage coord-mcp-tests: exit=$code"
        if ($code -ne 0) { throw "Coord MCP tests failed; inspect $log" }
    }
    if ($Stage -match '^g(?:[0-5]|[67](?:-[a-z0-9]+)*)$' -and $env:OS -eq 'Windows_NT') {
        $node = (Get-Command node.exe -ErrorAction Stop).Source
        $previousCoordTestBin = $env:COORD_TEST_BIN
        $env:COORD_TEST_BIN = (Resolve-Path -LiteralPath 'target/release/coord.exe').Path
        try {
            $log = "docs/verification/stage-$Stage-relay-tests.log"
            $started = [DateTime]::UtcNow
            & $node --test relay/relay.test.mjs relay/live-coord.test.mjs relay/logical-workers.test.mjs 2>&1 | ForEach-Object {
                $_.ToString().Replace($repoRoot,'<workspace>').Replace($env:USERPROFILE,'<user>')
            } | Set-Content -Encoding utf8 $log
            $code = $LASTEXITCODE
            $results += @{command='node --test relay/relay.test.mjs relay/live-coord.test.mjs relay/logical-workers.test.mjs';exit=$code;started=$started.ToString('o');seconds=([DateTime]::UtcNow-$started).TotalSeconds;log=$log}
            $results | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 "docs/verification/stage-$Stage-checks.json"
            Write-Output "$Stage relay-tests: exit=$code"
            if ($code -ne 0) { throw "relay tests failed; inspect $log" }
        } finally {
            $env:COORD_TEST_BIN = $previousCoordTestBin
        }

        $log = "docs/verification/stage-$Stage-coord-mcp-tests.log"
        $started = [DateTime]::UtcNow
        $npm = (Get-Command npm.cmd -ErrorAction Stop).Source
        & $npm test --prefix relay/coord-mcp 2>&1 | ForEach-Object {
            $_.ToString().Replace($repoRoot,'<workspace>').Replace($env:USERPROFILE,'<user>')
        } | Set-Content -Encoding utf8 $log
        $code = $LASTEXITCODE
        $results += @{command='npm test --prefix relay/coord-mcp';exit=$code;started=$started.ToString('o');seconds=([DateTime]::UtcNow-$started).TotalSeconds;log=$log}
        $results | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 "docs/verification/stage-$Stage-checks.json"
        Write-Output "$Stage coord-mcp-tests: exit=$code"
        if ($code -ne 0) { throw "Coord MCP tests failed; inspect $log" }

        $log = "docs/verification/stage-$Stage-phase-g-tools.log"
        $started = [DateTime]::UtcNow
        $python = (Get-Command python.exe -ErrorAction Stop).Source
        $previousNoBytecode = $env:PYTHONDONTWRITEBYTECODE
        $env:PYTHONDONTWRITEBYTECODE = '1'
        try {
            & $python -m unittest discover -s scripts/phase_g -p test_phase_g_tools.py -v 2>&1 | ForEach-Object {
                $_.ToString().Replace($repoRoot,'<workspace>').Replace($env:USERPROFILE,'<user>')
            } | Set-Content -Encoding utf8 $log
            $code = $LASTEXITCODE
        } finally {
            $env:PYTHONDONTWRITEBYTECODE = $previousNoBytecode
        }
        $results += @{command='python -m unittest discover -s scripts/phase_g -p test_phase_g_tools.py -v';exit=$code;started=$started.ToString('o');seconds=([DateTime]::UtcNow-$started).TotalSeconds;log=$log}
        $results | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 "docs/verification/stage-$Stage-checks.json"
        Write-Output "$Stage phase-g-tools: exit=$code"
        if ($code -ne 0) { throw "Phase G utility tests failed; inspect $log" }
    }
} finally { Pop-Location }

param([Parameter(Mandatory)][string]$MonitorSource)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$base='C:\ProgramData\coord\g7'
$state=Get-Content -Raw -LiteralPath (Join-Path $base 'private/site-state.json')|ConvertFrom-Json
if($state.state-ne 'PREPARED' -or $state.base-ne $base){throw 'Verified prepared site required.'}
$servy='C:\Program Files\Servy\servy-cli.exe'
$runner=$state.runner_service;$probeService=$state.probe_service;$registry=$state.registry
$key=[IO.File]::ReadAllText((Join-Path $base 'daemon-private/management.key')).Trim()
$private=Join-Path $base 'private'
$summary=[ordered]@{state='FAIL';phase='start-probe';runner_sid=$state.runner_sid;daemon_sid=$state.daemon_sid;production_replacement='NOT_RUN';checks=@()}
function Stop-OwnedService([string]$Name){
 if($Name-notin @($runner,$probeService)){throw 'Refusing unrelated service stop.'}
 $service=Get-Service -Name $Name
 if($service.Status-ne 'Stopped'){Stop-Service -Name $Name; $service.WaitForStatus('Stopped',[timespan]::FromSeconds(20))}
}
function Set-RunnerParameters([string]$Parameters,[bool]$Health=$false){
 Stop-OwnedService $runner
 $path=Join-Path $private 'runner-config.json'
 & $servy export --name $runner --config json --path $path --quiet *> (Join-Path $private 'runner-export.log')
 if($LASTEXITCODE-ne 0){throw 'Owned Runner export failed.'}
 $config=Get-Content -Raw -LiteralPath $path|ConvertFrom-Json
 if($config.Name-ne $runner -or $config.ExecutablePath-ne (Join-Path $base 'bin/coord-action-runner.exe')){throw 'Owned Runner configuration mismatch.'}
 $config.Parameters=$Parameters
 $config.EnableHealthMonitoring=$Health
 if($Health){
  $old=Get-Content -Raw -LiteralPath (Join-Path $state.backup_root 'services/coord.json')|ConvertFrom-Json
  $config.HeartbeatInterval=10;$config.MaxFailedChecks=3;$config.RecoveryAction=$old.RecoveryAction;$config.MaxRestartAttempts=3;$config.RecoveryOnCleanExit=$false
 }
 $config|ConvertTo-Json -Depth 30|Set-Content -LiteralPath $path -Encoding utf8NoBOM
 & $servy import --config json --path $path --quiet *> (Join-Path $private 'runner-import.log')
 if($LASTEXITCODE-ne 0){throw 'Owned Runner configuration update failed.'}
}
function Run-RunnerProbe([string]$Label,[string]$Extra,[string]$Marker){
 $summary.phase=$Label
 Set-RunnerParameters ('--registry "'+$registry+'" '+$Extra)
 $log=Join-Path $base 'runtime/logs/runner.stderr.log'
 if(Test-Path -LiteralPath $log){Copy-Item -LiteralPath $log -Destination (Join-Path $private ($Label+'.previous.log'))}
 [IO.File]::WriteAllText($log,'')
 $started=[DateTime]::UtcNow
 & $servy start --name $runner --quiet *> (Join-Path $private ($Label+'.start.log'))
 $controlExit=$LASTEXITCODE
 $output=''
 foreach($try in 1..150){
  if(Test-Path -LiteralPath $log){$output=[IO.File]::ReadAllText($log)}
  if($output.Contains($Marker)){break};Start-Sleep -Milliseconds 100
 }
 Stop-OwnedService $runner
 Copy-Item -LiteralPath $log -Destination (Join-Path $private ($Label+'.stderr.log'))
 if(-not $output.Contains($Marker)){throw ('Real service probe failed: '+$Label)}
 $identity=$output.Split("`n")[0]|ConvertFrom-Json
 if($identity.sid-ne $state.runner_sid -or $identity.privileged-ne $false -or $identity.sid-eq $state.daemon_sid){throw 'Actual service token identity failed.'}
 $summary.checks+=@{name=$Label;state='PASS';started=$started.ToString('o');identity=$identity;service_start_exit=$controlExit;note='One-shot service may already be stopped when service-control polling returns.'}
}
function Rpc([string]$Method,$Params){
 $body=@{jsonrpc='2.0';id=1;method=$Method;params=$Params}|ConvertTo-Json -Depth 35 -Compress
 $reply=Invoke-RestMethod -Uri $state.probe_url -Method Post -ContentType 'application/json' -Headers @{Authorization=('Bearer '+$key)} -Body $body -TimeoutSec 10
 if($reply.error){throw ('Candidate RPC rejected '+$Method)}
 return $reply.result
}
try{
 & $servy start --name $probeService --quiet *> (Join-Path $private 'probe-start.log')
 if($LASTEXITCODE-ne 0){throw 'Candidate daemon service failed to start.'}
 $ready=$false
 foreach($try in 1..100){try{Invoke-RestMethod ($state.probe_url+'.well-known/agent.json') -TimeoutSec 1|Out-Null;$ready=$true;break}catch{Start-Sleep -Milliseconds 100}}
 if(-not $ready){throw 'Candidate daemon did not become ready.'}
 Run-RunnerProbe 'service-identity' '--check-identity' '"privileged":false'
 Run-RunnerProbe 'service-acl-rpc' ('--probe-acl "'+(Join-Path $base 'config/acl-probe.json')+'"') '"acl_probe":"PASS"'
 $summary.phase='system-authenticated-health'
 Copy-Item -LiteralPath $MonitorSource -Destination (Join-Path $base 'config/coord-health-monitor.ps1')
 $sameKey=[IO.File]::ReadAllText((Join-Path $base 'monitor-private/management.key')).Trim()-ceq $key
 if(-not $sameKey){throw 'Monitor/daemon key copies differ.'}
 $systemScript=@'
$ErrorActionPreference='Stop'
$b='C:\ProgramData\coord\g7'
$exe=Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
& $exe -NoLogo -NoProfile -NonInteractive -File (Join-Path $b 'config/coord-health-monitor.ps1') -CoordExe (Join-Path $b 'bin/coord.exe') -CoordUrl 'http://127.0.0.1:17777/' -DbPath (Join-Path $b 'verification/state.db') -StatusPath (Join-Path $b 'runtime/system-health.json') -RecoveryPath (Join-Path $b 'runtime/system-recovery.json') -RpcAuthKeyFile (Join-Path $b 'monitor-private/management.key') -NoRestart
$code=$LASTEXITCODE
$health=Get-Content -Raw -LiteralPath (Join-Path $b 'runtime/system-health.json')|ConvertFrom-Json
@{sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value;exit=$code;status=$health.status;observed_at=[DateTime]::UtcNow.ToString('o')}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $b 'private/system-health-proof.json') -Encoding utf8
exit $code
'@
 $systemPath=Join-Path $private 'system-health-proof.ps1'
 $systemScript|Set-Content -LiteralPath $systemPath -Encoding utf8NoBOM
 $taskName='coord-g7-health-probe'
 if(Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue){throw 'Probe task name already exists.'}
 $action=New-ScheduledTaskAction -Execute (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe') -Argument ('-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "'+$systemPath+'"')
 $principal=New-ScheduledTaskPrincipal -UserId SYSTEM -LogonType ServiceAccount -RunLevel Highest
 Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Settings (New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([timespan]::FromMinutes(1)) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries)|Out-Null
 Start-ScheduledTask -TaskName $taskName
 foreach($try in 1..150){if(Test-Path -LiteralPath (Join-Path $private 'system-health-proof.json')){break};Start-Sleep -Milliseconds 200}
 $system=Get-Content -Raw -LiteralPath (Join-Path $private 'system-health-proof.json')|ConvertFrom-Json
 if($system.sid-ne 'S-1-5-18' -or $system.exit-ne 0 -or $system.status-ne 'READY'){throw 'Actual SYSTEM authenticated health failed.'}
 Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
 $summary.system_health=$system;$summary.management_key_copies_match=$sameKey
 $summary.phase='native-service-publish-index'
 Set-RunnerParameters ('--registry "'+$registry+'"') $true
 & $servy start --name $runner --quiet *> (Join-Path $private 'runner-runtime-start.log')
 if($LASTEXITCODE-ne 0){throw 'Actual dedicated Runner service failed.'}
 $batch=Rpc 'batches/create' @{name='G7 Arasaka isolated native Runner acceptance';config=@{action_profile='g7-probe-v1';require_progress=$true;max_attempts=3}}
 Rpc 'batches/add_tasks' @{batch_id=$batch.id;tasks=@(@{key='g7-probe-only';name='Synthetic marker';payload=@{group_id='00';source_image_ids=@('1')}})}|Out-Null
 $claim=(Rpc 'scheduler/claim' @{batch_id=$batch.id;agent_id='emote-0000';lease_seconds=30}).task
 $checkpoint=[guid]::NewGuid().ToString()
 Rpc 'scheduler/progress' @{id=$claim.id;agent_id='emote-0000';lease_token=$claim.lease_token;attempt_no=$claim.attempt_no;sequence=1;request_id=$checkpoint;checkpoint='Inspected synthetic marker and its reference'}|Out-Null
 Rpc 'scheduler/extend_progress' @{id=$claim.id;agent_id='emote-0000';lease_token=$claim.lease_token;attempt_no=$claim.attempt_no;progress_sequence=1;request_id=[guid]::NewGuid().ToString();lease_seconds=30}|Out-Null
 $result=@{schema_version=1;group_id='00';agent_id='emote-0000';attempt_no=$claim.attempt_no;source_image_ids=@('1');characters=@();uncertain=@(@{image_id='1';visible_features='Synthetic single-pixel marker; no game character is depicted';reference_candidates=@();checked_paths=@('refs/arknights/icons/marker.png');check_conclusion='Checked the synthetic reference; this is not a character classification';reason='Explicit synthetic deployment fixture';review_priority='low'});general_notes='G7 isolated service acceptance only'}
 Rpc 'scheduler/submit' @{id=$claim.id;agent_id='emote-0000';lease_token=$claim.lease_token;result=$result}|Out-Null
 $actions=$null
 foreach($try in 1..150){$actions=Rpc 'actions/status' @{batch_id=$batch.id};if(@($actions.actions).Count-eq 2 -and @($actions.actions|Where-Object state -ne 'succeeded').Count-eq 0){break};Start-Sleep -Milliseconds 100}
 if(@($actions.actions).Count-ne 2 -or @($actions.actions|Where-Object state -ne 'succeeded').Count){throw 'Actual service publish/index did not both succeed.'}
 $summary.publish=@{batch_id=$batch.id;task_id=$claim.id;actions=$actions;result_sha256=(Get-FileHash -LiteralPath (Join-Path $base 'probe/business/results/00.json')).Hash.ToLowerInvariant();index_sha256=(Get-FileHash -LiteralPath (Join-Path $base 'probe/business/results/index.json')).Hash.ToLowerInvariant()}
 $summary.phase='completed';$summary.state='PASS'
}catch{$summary.failure=@{phase=$summary.phase;message=$_.Exception.Message;exception_type=$_.Exception.GetType().FullName};throw}
finally{
 $summary|ConvertTo-Json -Depth 30|Set-Content -LiteralPath (Join-Path $private 'site-result.json') -Encoding utf8NoBOM
 $summary|ConvertTo-Json -Depth 30
}

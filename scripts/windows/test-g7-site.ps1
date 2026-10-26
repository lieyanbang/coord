param(
 [Parameter(Mandatory)][string]$MonitorSource,
 [Parameter(Mandatory)][string]$DatabaseAuditScript,
 [Parameter(Mandatory)][string]$RehearsalRoot
)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$base=[IO.Path]::GetFullPath($RehearsalRoot)
$state=Get-Content -Raw -LiteralPath (Join-Path $base 'private/site-state.json')|ConvertFrom-Json
if($state.state-ne 'PREPARED' -or $state.base-ne $base -or [IO.Path]::GetDirectoryName($base)-ine [IO.Path]::GetFullPath([string]$state.backup_root) -or [IO.Path]::GetFileName($base)-notmatch '^r5-1b-[a-f0-9]{8}$'){throw 'Verified protected-copy rehearsal site required.'}
$monitorPath=[IO.Path]::GetFullPath($MonitorSource)
if([IO.Path]::GetFileName($monitorPath)-ine 'coord-health-monitor.ps1' -or [string]::IsNullOrWhiteSpace([string]$state.artifact_health_monitor_sha256) -or (Get-FileHash -LiteralPath $monitorPath -Algorithm SHA256).Hash.ToLowerInvariant() -ne [string]$state.artifact_health_monitor_sha256){throw 'Matched CI health-monitor script hash is required.'}
$servy='C:\Program Files\Servy\servy-cli.exe'
$runner=$state.runner_service;$probeService=$state.probe_service;$registry=$state.registry
$privateKeyPath=[IO.Path]::GetFullPath([string]$state.test_runner_private_key_path)
$expectedPrivateKeyPath=[IO.Path]::GetFullPath((Join-Path $base 'test-runner-private/runner-test.pk8'))
if(-not $privateKeyPath.Equals($expectedPrivateKeyPath,[StringComparison]::OrdinalIgnoreCase) -or -not(Test-Path -LiteralPath $privateKeyPath -PathType Leaf)){throw 'Prepared test-only Runner private-key path mismatch.'}
if(-not(Test-Path -LiteralPath $DatabaseAuditScript -PathType Leaf)){throw 'Read-only database audit script is required.'}
$copyDb=Join-Path $base 'verification/state.db'
$key=[IO.File]::ReadAllText((Join-Path $base 'daemon-private/management.key')).Trim()
$private=Join-Path $base 'private'
$taskName=[string]$state.system_health_task_name
$probeTaskCreated=$false
$summary=[ordered]@{state='FAIL';phase='broker-unavailable-compatibility';runner_sid=$state.runner_sid;daemon_sid=$state.daemon_sid;health_monitor_sha256=[string]$state.artifact_health_monitor_sha256;production_replacement='NOT_RUN';checks=@()}
function Stop-OwnedService([string]$Name){
 if($Name-notin @($runner,$probeService)){throw 'Refusing unrelated service stop.'}
 $service=Get-Service -Name $Name
 if($service.Status-ne 'Stopped'){Stop-Service -Name $Name; $service.WaitForStatus('Stopped',[timespan]::FromSeconds(20))}
}
function Set-RunnerParameters([string]$ExtraArguments,[bool]$Health=$false){
 Stop-OwnedService $runner
 $path=Join-Path $private 'runner-config.json'
 & $servy export --name $runner --config json --path $path --quiet *> (Join-Path $private 'runner-export.log')
 if($LASTEXITCODE-ne 0){throw 'Owned Runner export failed.'}
 $config=Get-Content -Raw -LiteralPath $path|ConvertFrom-Json
 if($config.Name-ne $runner -or $config.ExecutablePath-ne (Join-Path $base 'bin/coord-action-runner.exe')){throw 'Owned Runner configuration mismatch.'}
 $config.Parameters='--registry "'+$registry+'" --private-key-file "'+$privateKeyPath+'"'
 if(-not[string]::IsNullOrWhiteSpace($ExtraArguments)){$config.Parameters+=' '+$ExtraArguments}
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
 Set-RunnerParameters $Extra
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
function Get-CopyDatabaseAudit([string]$Path){
 $raw=@(& $DatabaseAuditScript -DbPath $Path)
 if($LASTEXITCODE-ne 0 -or $raw.Count-ne 1){throw 'Read-only backup-copy SQLite audit failed.'}
 $report=($raw[0]|Out-String|ConvertFrom-Json)
 if($report.state-ne 'PASS' -or $report.database.integrity_check-ne 'ok'){throw 'Backup-copy SQLite integrity gate failed.'}
 return $report.database
}
try{
 $summary.phase='broker-unavailable-compatibility'
 $signedConfig=Get-Content -Raw -LiteralPath $registry|ConvertFrom-Json
 if(-not $signedConfig.external_runner -or -not $signedConfig.external_runner.runner_key_id -or -not $signedConfig.external_runner.runner_public_key_file){throw 'Signed Runner configuration is required for the second rehearsal phase.'}
 $migrationBefore=Get-CopyDatabaseAudit $copyDb
 $migrationBefore|ConvertTo-Json -Depth 20|Set-Content -LiteralPath (Join-Path $private 'migration-before.json') -Encoding utf8NoBOM
 $compatConfig=$signedConfig|ConvertTo-Json -Depth 30|ConvertFrom-Json
 $compatConfig.external_runner=$null
 $compatConfig|ConvertTo-Json -Depth 30|Set-Content -LiteralPath $registry -Encoding utf8NoBOM
 & $servy start --name $probeService --quiet *> (Join-Path $private 'probe-start.log')
 if($LASTEXITCODE-ne 0){throw 'Candidate daemon service failed to start.'}
 $ready=$false
 $compatHealth=$null
 foreach($try in 1..100){try{$compatHealth=Invoke-RestMethod ($state.probe_url+'healthz') -TimeoutSec 1;if($compatHealth.core.status-eq 'READY' -and $compatHealth.database.status-eq 'READY' -and $compatHealth.broker.status-eq 'UNAVAILABLE'){ $ready=$true;break }}catch{};Start-Sleep -Milliseconds 100}
 if(-not $ready -or $compatHealth.status-ne 'DEGRADED' -or $compatHealth.runner.status-ne 'UNAVAILABLE'){throw 'Broker-unavailable Scheduler/Core health contract failed.'}
 $compatTasks=Rpc 'tasks/list' @{limit=1;skip_reclaim=$true}
 $migrationAfter=Get-CopyDatabaseAudit $copyDb
 $migrationAfter|ConvertTo-Json -Depth 20|Set-Content -LiteralPath (Join-Path $private 'migration-after.json') -Encoding utf8NoBOM
 $protectedBefore=ConvertTo-Json -InputObject @($migrationBefore.protected) -Depth 30 -Compress
 $protectedAfter=ConvertTo-Json -InputObject @($migrationAfter.protected) -Depth 30 -Compress
 $actionsBefore=ConvertTo-Json -InputObject $migrationBefore.completion_action_states -Depth 10 -Compress
 $actionsAfter=ConvertTo-Json -InputObject $migrationAfter.completion_action_states -Depth 10 -Compress
 if(-not $migrationBefore.integrity_check.Equals('ok',[StringComparison]::OrdinalIgnoreCase) -or $migrationBefore.runner_nonces_table -or $migrationBefore.task_attempts_table){throw 'Backup copy did not match the expected pre-migration schema.'}
 if(-not $migrationAfter.runner_nonces_table -or -not $migrationAfter.task_attempts_table -or $migrationAfter.runner_nonce_rows-ne 0){throw 'Compatibility Core did not complete the additive Runner/attempt migration.'}
 if($migrationBefore.user_version-ne $migrationAfter.user_version -or $migrationAfter.integrity_check-ne 'ok' -or $protectedBefore-cne $protectedAfter -or $actionsBefore-cne $actionsAfter){throw 'Compatibility migration changed protected rows, receipts, actions, or DB integrity.'}
 $summary.compatibility=@{state='PASS';health=$compatHealth;authenticated_tasks_list='PASS';scheduler_stayed_ready=$true;broker_explicitly_unavailable=$true;database_before=@{schema_version=$migrationBefore.schema_version;user_version=$migrationBefore.user_version;integrity_check=$migrationBefore.integrity_check;protected=$migrationBefore.protected;completion_action_states=$migrationBefore.completion_action_states};database_after=@{schema_version=$migrationAfter.schema_version;user_version=$migrationAfter.user_version;integrity_check=$migrationAfter.integrity_check;runner_nonces_table=$migrationAfter.runner_nonces_table;runner_nonce_rows=$migrationAfter.runner_nonce_rows;task_attempts_table=$migrationAfter.task_attempts_table;protected=$migrationAfter.protected;completion_action_states=$migrationAfter.completion_action_states};protected_rows_receipts_and_actions_unchanged=$true;copy_only=$true;compatibility_tasks_list_count=@($compatTasks).Count}
 Stop-OwnedService $probeService
 $signedConfig|ConvertTo-Json -Depth 30|Set-Content -LiteralPath $registry -Encoding utf8NoBOM
 & $servy start --name $probeService --quiet *> (Join-Path $private 'probe-signed-start.log')
 if($LASTEXITCODE-ne 0){throw 'Signed Broker Core service failed to start.'}
 $brokerReady=$false
 $signedHealth=$null
 foreach($try in 1..100){try{$signedHealth=Invoke-RestMethod ($state.probe_url+'healthz') -TimeoutSec 1;if($signedHealth.core.status-eq 'READY' -and $signedHealth.database.status-eq 'READY' -and $signedHealth.broker.status-eq 'READY' -and $signedHealth.runner.status-eq 'UNAVAILABLE'){$brokerReady=$true;break}}catch{};Start-Sleep -Milliseconds 100}
 if(-not $brokerReady){throw 'Signed Broker listener did not become READY independently of Runner.'}
 $summary.signed_broker_start=@{state='PASS';health=$signedHealth;authenticated_tasks_list='PASS';runner_waiting_for_first_signed_request=$true}
 $summary.phase='runner-service-identity'
 Run-RunnerProbe 'service-identity' '--check-identity' '"privileged":false'
 Run-RunnerProbe 'service-acl-rpc' ('--probe-acl "'+(Join-Path $base 'config/acl-probe.json')+'"') '"acl_probe":"PASS"'
 $summary.phase='runner-signed-health'
 Set-RunnerParameters '' $true
 & $servy start --name $runner --quiet *> (Join-Path $private 'runner-health-start.log')
 if($LASTEXITCODE-ne 0){throw 'Actual dedicated Runner service failed to start for its signed heartbeat.'}
 $runnerReady=$false;$runnerHealth=$null
 foreach($try in 1..150){try{$runnerHealth=Invoke-RestMethod ($state.probe_url+'healthz') -TimeoutSec 1;if($runnerHealth.status-eq 'READY' -and $runnerHealth.broker.status-eq 'READY' -and $runnerHealth.runner.status-eq 'READY'){$runnerReady=$true;break}}catch{};Start-Sleep -Milliseconds 200}
 if(-not $runnerReady){throw 'Signed Runner authentication/heartbeat did not reach READY.'}
 $summary.runner_health=@{state='PASS';health=$runnerHealth;runner_sid=$state.runner_sid;test_private_key_path=$privateKeyPath;production_private_key='NOT_GENERATED_DURING_REHEARSAL';private_key_acl_probe='PASS'}
 $summary.phase='system-authenticated-health'
 Copy-Item -LiteralPath $MonitorSource -Destination (Join-Path $base 'config/coord-health-monitor.ps1')
 $sameKey=[IO.File]::ReadAllText((Join-Path $base 'monitor-private/management.key')).Trim()-ceq $key
 if(-not $sameKey){throw 'Monitor/daemon key copies differ.'}
$systemScript=@'
$ErrorActionPreference='Stop'
$b='__R5B1B_REHEARSAL_ROOT__'
$exe=Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
& $exe -NoLogo -NoProfile -NonInteractive -File (Join-Path $b 'config/coord-health-monitor.ps1') -CoordExe (Join-Path $b 'bin/coord.exe') -CoordUrl 'http://127.0.0.1:17777/' -DbPath (Join-Path $b 'verification/state.db') -StatusPath (Join-Path $b 'runtime/system-health.json') -RecoveryPath (Join-Path $b 'runtime/system-recovery.json') -RpcAuthKeyFile (Join-Path $b 'monitor-private/management.key') -NoRestart
$code=$LASTEXITCODE
$health=Get-Content -Raw -LiteralPath (Join-Path $b 'runtime/system-health.json')|ConvertFrom-Json
@{sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value;exit=$code;status=$health.status;observed_at=[DateTime]::UtcNow.ToString('o')}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $b 'private/system-health-proof.json') -Encoding utf8
exit $code
'@
$systemScript=$systemScript.Replace('__R5B1B_REHEARSAL_ROOT__',$base.Replace("'","''"))
 $systemPath=Join-Path $private 'system-health-proof.ps1'
 $systemScript|Set-Content -LiteralPath $systemPath -Encoding utf8NoBOM
 if(Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue){throw 'Probe task name already exists.'}
 $action=New-ScheduledTaskAction -Execute (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe') -Argument ('-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "'+$systemPath+'"')
 $principal=New-ScheduledTaskPrincipal -UserId SYSTEM -LogonType ServiceAccount -RunLevel Highest
 Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Settings (New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([timespan]::FromMinutes(1)) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries)|Out-Null
 $probeTaskCreated=$true
 Start-ScheduledTask -TaskName $taskName
 foreach($try in 1..150){if(Test-Path -LiteralPath (Join-Path $private 'system-health-proof.json')){break};Start-Sleep -Milliseconds 200}
 $system=Get-Content -Raw -LiteralPath (Join-Path $private 'system-health-proof.json')|ConvertFrom-Json
 if($system.sid-ne 'S-1-5-18' -or $system.exit-ne 0 -or $system.status-ne 'READY'){throw 'Actual SYSTEM authenticated health failed.'}
 Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
 $probeTaskCreated=$false
 $summary.system_health=$system;$summary.management_key_copies_match=$sameKey
 $summary.phase='native-service-publish-index'
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
 if($probeTaskCreated){try{Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction Stop;$summary.temporary_system_task_cleanup='PASS'}catch{$summary.temporary_system_task_cleanup='FAIL'}}
 if($summary.state-ne 'PASS'){
  foreach($name in @($runner,$probeService)){try{Stop-OwnedService $name}catch{}}
  $summary.failed_rehearsal_services_stopped=$true
 }
 $summary|ConvertTo-Json -Depth 30|Set-Content -LiteralPath (Join-Path $private 'site-result.json') -Encoding utf8NoBOM
 $summary|ConvertTo-Json -Depth 30
}

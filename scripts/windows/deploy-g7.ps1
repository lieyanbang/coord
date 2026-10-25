param([Parameter(Mandatory)][string]$UploadRoot)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$base='C:\ProgramData\coord\g7'
$private=Join-Path $base 'private'
$state=Get-Content -Raw (Join-Path $private 'site-state.json')|ConvertFrom-Json
$backup=$state.backup_root
$servy='C:\Program Files\Servy\servy-cli.exe'
$registry=Join-Path $base 'config/registry-production.json'
if($state.state -ne 'PREPARED' -or $state.runner_sid -eq $state.daemon_sid){throw 'Actual site preparation required.'}
foreach($report in @('site-result.json','site-recovery-result.json')){
 $proof=Get-Content -Raw (Join-Path $private $report)|ConvertFrom-Json
 if($proof.state -ne 'PASS'){throw 'Actual site acceptance and recovery required.'}
}
if(Test-Path (Join-Path $private 'production-deployment.json')){throw 'Inspect existing deployment instead of repeating.'}
$manifest=Get-Content -Raw (Join-Path $UploadRoot 'stage-g7-artifacts.json')|ConvertFrom-Json
if($manifest.state -ne 'PASS' -or $manifest.commit -ne $state.commit){throw 'Matching accepted artifact required.'}
foreach($entry in $manifest.files){if((Get-FileHash (Join-Path $base ('bin/'+$entry.name))).Hash.ToLowerInvariant() -ne $entry.sha256){throw 'Binary drift.'}}
if((Get-FileHash 'C:\ProgramData\coord\bin\coord.exe').Hash.ToLowerInvariant() -ne 'b58d3620b1b6de8035bfcb97e99f2e4fe654f48b1cb5719ef9fca013f5652e0d'){throw 'Installed baseline drift.'}
$key=[IO.File]::ReadAllText((Join-Path $base 'daemon-private/management.key')).Trim()
function Rpc([string]$Method,$Params,[bool]$Authenticated=$true){
 $headers=@{};if($Authenticated){$headers.Authorization='Bearer '+$key}
 $reply=Invoke-RestMethod 'http://127.0.0.1:7777/' -Method Post -ContentType application/json -Headers $headers -Body (@{jsonrpc='2.0';id=1;method=$Method;params=$Params}|ConvertTo-Json -Depth 30 -Compress) -TimeoutSec 10
 if($reply.error){throw ('RPC failed: '+$Method)};return $reply.result
}
foreach($priority in @('urgent','high')){if(@(Rpc 'tasks/list' @{state='pending';priority=$priority;limit=100;skip_reclaim=$true} $false).Count -gt 0){throw 'Urgent/high queue changed; inspect overlap.'}}
$fresh=Join-Path $private ('pre-deploy-'+[DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ')+'.db')
& 'C:\Users\lieyanbang\AppData\Local\Programs\Python\Python314\python.exe' (Join-Path $backup 'sqlite-online-backup.py') --source 'C:\ProgramData\coord\data\coord.db' --destination $fresh *> (Join-Path $private 'pre-deploy-backup.json')
if($LASTEXITCODE -ne 0){throw 'Fresh online backup failed.'}
$config=Get-Content -Raw (Join-Path $base 'config/registry-probe.json')|ConvertFrom-Json
$config.external_runner.broker_addr='127.0.0.1:7779'
$config.profiles.PSObject.Properties.Remove('g7-probe-v1')
$config.profiles.PSObject.Properties.Remove('g7-crash-probe-v1')
$browser=Join-Path $base 'browser/business'
if(Test-Path $browser){throw 'Browser fixture already exists.'}
New-Item -ItemType Directory -Path $browser|Out-Null
foreach($part in @('refs','task')){Copy-Item -LiteralPath (Join-Path $base ('canary/business/'+$part)) -Destination (Join-Path $browser $part) -Recurse}
foreach($part in @('results/.receipts','results/review_queue')){New-Item -ItemType Directory -Path (Join-Path $browser $part) -Force|Out-Null}
& icacls.exe (Join-Path $browser 'results') /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' ('*'+$state.runner_sid+':(OI)(CI)M') '*S-1-5-19:(OI)(CI)M'|Out-Null
if($LASTEXITCODE -ne 0){throw 'Browser results ACL failed.'}
$profile=($config.profiles.'g7-canary-v1'|ConvertTo-Json -Depth 20|ConvertFrom-Json)
$profile.on_task_completed.args=@('--root',$browser);$profile.on_batch_completed.args=@('--root',$browser)
$config.profiles|Add-Member -NotePropertyName 'g7-browser-v1' -NotePropertyValue $profile
$config|ConvertTo-Json -Depth 30|Set-Content $registry -Encoding utf8NoBOM
$policy=@{version=2;mode='logical';enabled=$true;signing_key_file=(Join-Path $base 'relay-private/identity.key');legacy_client_sha256=@('eeb595ffaeec3cc157243f766c320f4133f1d9a6af05ad21ee8fb373850122ba');bindings=@()}
$policy|ConvertTo-Json -Depth 10|Set-Content (Join-Path $base 'relay-private/identity.json') -Encoding utf8NoBOM
$relayConfig=Get-Content -Raw 'C:\ProgramData\coord-relay-agent\config.json'|ConvertFrom-Json
$relayConfig|Add-Member -NotePropertyName coordAuthKeyFile -NotePropertyValue (Join-Path $base 'daemon-private/management.key')
$relayConfig|Add-Member -NotePropertyName identityPolicyFile -NotePropertyValue (Join-Path $base 'relay-private/identity.json')
$coordConfig=Get-Content -Raw (Join-Path $backup 'services/coord.json')|ConvertFrom-Json
if($coordConfig.Parameters -match 'actions-config'){throw 'Unexpected existing actions configuration.'}
$coordConfig.Parameters+=' --actions-config "'+$registry+'"'
$coordConfig|ConvertTo-Json -Depth 30|Set-Content (Join-Path $private 'coord-production.json') -Encoding utf8NoBOM
function Stop-Owned([string]$Name){if($Name -notin @('coord','CoordRelayAgent',$state.runner_service)){throw 'Unowned service.'};$s=Get-Service $Name;if($s.Status -ne 'Stopped'){Stop-Service $Name;$s.WaitForStatus('Stopped',[timespan]::FromSeconds(30))}}
function Start-Owned([string]$Name){& $servy start --name $Name --quiet *> (Join-Path $private ($Name+'-production-start.log'));if($LASTEXITCODE -ne 0){throw ('Service start failed: '+$Name)}}
$summary=[ordered]@{state='FAIL';phase='begin';commit=$state.commit;backup_root=$backup;fresh_snapshot=$fresh;started_at=[DateTime]::UtcNow.ToString('o')}
try{
 Disable-ScheduledTask -TaskName coord-health-monitor|Out-Null
 Stop-ScheduledTask -TaskName coord-health-monitor
 Stop-Owned $state.runner_service;Stop-Owned 'CoordRelayAgent';Stop-Owned 'coord'
 $summary.phase='install'
 Copy-Item (Join-Path $base 'bin/coord.exe') 'C:\ProgramData\coord\bin\coord.exe' -Force
 & $servy import --config json --path (Join-Path $private 'coord-production.json') --quiet *> (Join-Path $private 'coord-production-import.log')
 if($LASTEXITCODE -ne 0){throw 'coord configuration import failed.'}
 foreach($file in @('agent.mjs','protocol.mjs','identity.cjs')){Copy-Item (Join-Path $UploadRoot $file) (Join-Path 'C:\ProgramData\coord-relay-agent' $file) -Force}
 $relayConfig|ConvertTo-Json -Depth 10|Set-Content 'C:\ProgramData\coord-relay-agent\config.json' -Encoding utf8NoBOM
 Copy-Item (Join-Path $UploadRoot 'coord-health-monitor.ps1') 'C:\ProgramData\coord\health\coord-health-monitor.ps1' -Force
 $task=Get-ScheduledTask -TaskName coord-health-monitor
 $args=$task.Actions[0].Arguments+' -RpcAuthKeyFile "'+(Join-Path $base 'monitor-private/management.key')+'"'
 $action=New-ScheduledTaskAction -Execute $task.Actions[0].Execute -Argument $args
 Set-ScheduledTask -TaskName coord-health-monitor -Action $action|Out-Null
 $runnerPath=Join-Path $private 'runner-production.json'
 & $servy export --name $state.runner_service --config json --path $runnerPath --quiet *> (Join-Path $private 'runner-production-export.log')
 if($LASTEXITCODE -ne 0){throw 'Runner export failed.'}
 $runner=Get-Content -Raw $runnerPath|ConvertFrom-Json
 $runner.Parameters='--registry "'+$registry+'"';$runner.StartupType=$coordConfig.StartupType
 $runner.EnableHealthMonitoring=$true;$runner.HeartbeatInterval=10;$runner.MaxFailedChecks=3;$runner.RecoveryAction=$coordConfig.RecoveryAction;$runner.MaxRestartAttempts=3;$runner.RecoveryOnCleanExit=$false
 $runner|ConvertTo-Json -Depth 30|Set-Content $runnerPath -Encoding utf8NoBOM
 & $servy import --config json --path $runnerPath --quiet *> (Join-Path $private 'runner-production-import.log')
 if($LASTEXITCODE -ne 0){throw 'Runner configuration failed.'}
 & sc.exe config $state.runner_service start= auto depend= coord *> (Join-Path $private 'runner-production-scm.log')
 if($LASTEXITCODE -ne 0){throw 'Runner dependency failed.'}
 $summary.phase='start-and-verify'
 Start-Owned 'coord'
 $ready=$false
 foreach($try in 1..100){try{Rpc 'tasks/list' @{limit=1;skip_reclaim=$true}|Out-Null;$ready=$true;break}catch{Start-Sleep -Milliseconds 200}}
 if(-not $ready){throw 'Authenticated coord did not become ready.'}
 $unauth=Invoke-WebRequest 'http://127.0.0.1:7777/' -Method Post -ContentType application/json -Body '{"jsonrpc":"2.0","id":1,"method":"tasks/list","params":{}}' -SkipHttpErrorCheck
 if($unauth.StatusCode -ne 401){throw 'Management RPC is not authenticated.'}
 Start-Owned $state.runner_service;Start-Owned 'CoordRelayAgent'
 Enable-ScheduledTask -TaskName coord-health-monitor|Out-Null
 $proofTime=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds();Start-ScheduledTask -TaskName coord-health-monitor
 $health=$null
 foreach($try in 1..100){Start-Sleep -Milliseconds 200;$health=Get-Content -Raw 'C:\ProgramData\coord\health\status.json'|ConvertFrom-Json;if($health.checked_at_unix_ms -ge $proofTime -and $health.status -eq 'READY'){break}}
 if($health.status -ne 'READY' -or $health.checked_at_unix_ms -lt $proofTime){throw 'Actual production SYSTEM monitor not READY.'}
 $summary.state='PASS';$summary.phase='production-ready';$summary.management_unauthenticated_status=$unauth.StatusCode;$summary.health=$health
 $summary.binary_sha256=(Get-FileHash 'C:\ProgramData\coord\bin\coord.exe').Hash.ToLowerInvariant()
 $summary.services=@(Get-CimInstance Win32_Service|Where-Object Name -in @('coord','CoordRelayAgent',$state.runner_service,'Servy')|Select-Object Name,State,StartMode,StartName,ProcessId)
 $summary.monitor=@{principal=(Get-ScheduledTask coord-health-monitor).Principal.UserId;last_result=(Get-ScheduledTaskInfo coord-health-monitor).LastTaskResult}
}catch{
 $summary.failure=$_.Exception.Message
 # Preserve the migrated live database and any accepted work. Rehearsal proved
 # the old executable can read this additive schema; never restore stale data.
 try{
  foreach($name in @($state.runner_service,'CoordRelayAgent','coord')){Stop-Owned $name}
  Copy-Item (Join-Path $backup 'binary/coord.exe') 'C:\ProgramData\coord\bin\coord.exe' -Force
  & $servy import --config json --path (Join-Path $backup 'services/coord.json') --quiet *> (Join-Path $private 'rollback-import.log')
  if($LASTEXITCODE -ne 0){throw 'Rollback service import failed.'}
  Get-ChildItem (Join-Path $backup 'relay') -File|Copy-Item -Destination 'C:\ProgramData\coord-relay-agent' -Force
  Copy-Item (Join-Path $backup 'monitor/coord-health-monitor.ps1') 'C:\ProgramData\coord\health\coord-health-monitor.ps1' -Force
  Register-ScheduledTask -TaskName coord-health-monitor -Xml (Get-Content -Raw (Join-Path $backup 'monitor/coord-health-monitor.xml')) -Force|Out-Null
  Start-Owned 'coord';Start-Owned 'CoordRelayAgent'
  $summary.rollback='restored old binary/service/relay/monitor; live DB preserved'
 }catch{$summary.rollback='FAILED: '+$_.Exception.Message}
}finally{
 $summary.finished_at=[DateTime]::UtcNow.ToString('o')
 $summary|ConvertTo-Json -Depth 20|Set-Content (Join-Path $private 'production-deployment.json') -Encoding utf8NoBOM
 $summary|ConvertTo-Json -Depth 20
}
if($summary.state -ne 'PASS'){throw 'Controlled Windows deployment failed; inspect redacted evidence.'}

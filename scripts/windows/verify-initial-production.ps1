$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$base='C:\ProgramData\coord\g7'
$state=Get-Content -Raw (Join-Path $base 'private/initial-operation.json')|ConvertFrom-Json
$before=Get-Content -Raw (Join-Path $state.backup_root 'baseline.json')|ConvertFrom-Json
$backupAcl=Get-Acl -LiteralPath $state.backup_root
foreach($rule in $backupAcl.Access){
 if($rule.AccessControlType -eq 'Allow' -and $rule.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value -notin @('S-1-5-18','S-1-5-32-544')){throw 'Backup permits an unapproved principal.'}
}
$now=Get-CimInstance Win32_Service
$owned=@('coord','CoordActionRunnerG7','CoordRelayAgent')
$stable=@('ArasakaDDNS','ArasakaFFmpegWatchdog','ArasakaFFmpegWorker','CodexProxy','MediaSTT','OCI-A1-Grabber','Servy')
$protected=@()
$transient=@()
foreach($old in $before.services){
 if($old.Name -in $owned){continue}
 $s=$now|Where-Object Name -eq $old.Name
 if(-not $s -or $s.StartName -ne $old.StartName -or $s.StartMode -ne $old.StartMode){throw ('Unrelated service configuration changed: '+$old.Name)}
 if($s.State -ne $old.State -or $s.ProcessId -ne $old.ProcessId){
  if($old.Name -in $stable){throw ('Protected application service changed: '+$old.Name)}
  $transient+=@{name=$old.Name;before_state=$old.State;current_state=$s.State;before_pid=$old.ProcessId;current_pid=$s.ProcessId}
 }
 $protected+=$old.Name
}
foreach($name in $owned){if(($now|Where-Object Name -eq $name).State -ne 'Running'){throw 'Owned production service not running.'}}
$health=Get-Content -Raw 'C:\ProgramData\coord\health\status.json'|ConvertFrom-Json
if($health.status -ne 'READY' -or [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()-$health.checked_at_unix_ms -gt 120000){throw 'Fresh authenticated READY required.'}
$monitor=Get-ScheduledTask coord-health-monitor
if($monitor.Principal.UserId -ne 'SYSTEM' -or (Get-ScheduledTaskInfo coord-health-monitor).LastTaskResult -ne 0 -or $monitor.State -eq 'Disabled'){throw 'SYSTEM monitor failed.'}
$expected=@{'coord.exe'='b23e43f30a076d3d3dc75e90a07a7a3f9de35ad33351bb2e9bf8938f9f220ed4';'coord-action-runner.exe'='72c2e72892405b16042f1e7a1066eb35493695d54de1183447ab771192fe14f8';'coord-emote-action.exe'='cac5496ed1dea50f837f28403feb62515de1ba1459ea2557abe6097ec07bec55'}
foreach($name in $expected.Keys){if((Get-FileHash (Join-Path $base ('bin/'+$name))).Hash.ToLowerInvariant() -ne $expected[$name]){throw 'Installed hash drift.'}}
$listeners=@(Get-NetTCPConnection -LocalPort 7777,7779 -State Listen)
if($listeners.Count -ne 2 -or @($listeners|Where-Object LocalAddress -ne '127.0.0.1').Count){throw 'Listener scope failed.'}
$body='{"jsonrpc":"2.0","id":1,"method":"tasks/list","params":{"limit":1,"skip_reclaim":true}}'
$missing=Invoke-WebRequest 'http://127.0.0.1:7777/' -Method Post -ContentType application/json -Body $body -SkipHttpErrorCheck
$denied=Invoke-WebRequest 'http://127.0.0.1:7777/' -Method Post -ContentType application/json -Headers @{Authorization='Bearer unrelated-runner-credential'} -Body $body -SkipHttpErrorCheck
if($missing.StatusCode -ne 401 -or $denied.StatusCode -ne 401){throw 'Management security boundary failed.'}
$registry=Get-Content -Raw (Join-Path $base 'config/registry-production.json')|ConvertFrom-Json
$oldRegistry=Get-Content -Raw (Join-Path $state.backup_root 'registry-before.json')|ConvertFrom-Json
foreach($p in $oldRegistry.profiles.PSObject.Properties){if(($p.Value|ConvertTo-Json -Depth 30 -Compress) -ne ($registry.profiles.($p.Name)|ConvertTo-Json -Depth 30 -Compress)){throw 'Old profile changed.'}}
$identity=Get-Content -Raw (Join-Path $base 'relay-private/identity.json')|ConvertFrom-Json
$oldPolicy=Get-Content -Raw (Join-Path $state.backup_root 'identity-before.json')|ConvertFrom-Json
if(($identity.bindings[0..2]|ConvertTo-Json -Depth 20 -Compress) -ne ($oldPolicy.bindings|ConvertTo-Json -Depth 20 -Compress)){throw 'Old identity bindings changed.'}
$runnerToken=@(Get-Content (Join-Path $base 'runtime/logs/runner.stderr.log')|Where-Object{$_ -match '^\{"sid":'}|ForEach-Object{$_|ConvertFrom-Json})|Select-Object -Last 1
if($runnerToken.sid -ne $registry.external_runner.runner_sid -or $runnerToken.privileged -ne $false){throw 'Current Runner service token failed.'}
@{state='PASS';observed_at=[DateTime]::UtcNow.ToString('o');batch_id=$state.batch_id;backup_acl_admin_system_only=$true;unrelated_service_configurations_preserved=$protected.Count;protected_application_services_unchanged=$stable;os_transient_state_changes=$transient;owned_services=@($now|Where-Object Name -in $owned|Select-Object Name,State,StartName,ProcessId);health=$health;monitor=@{principal=$monitor.Principal.UserId;last_exit=(Get-ScheduledTaskInfo coord-health-monitor).LastTaskResult};hashes=$expected;management_rejections=@{missing=$missing.StatusCode;unrelated_credential=$denied.StatusCode;legacy_broker_key_read='NOT_PERFORMED'};loopback_only=$true;prior_profiles_and_bindings_unchanged=$true;runner_token=$runnerToken;rollback_requires_expired_formal_binding_tombstone=$true}|ConvertTo-Json -Depth 20

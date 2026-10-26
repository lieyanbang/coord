param(
 [Parameter(Mandatory)][string]$UploadRoot,
 [Parameter(Mandatory)][string]$DatabaseAuditScript,
 [Parameter(Mandatory)][string]$RehearsalRoot
)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$base=[IO.Path]::GetFullPath($RehearsalRoot)
$productionRoot='C:\ProgramData\coord\g7'
$productionCore='C:\ProgramData\coord\bin\coord.exe'
$productionRunner='C:\ProgramData\coord\g7\bin\coord-action-runner.exe'
$productionPublisher='C:\ProgramData\coord\g7\bin\coord-emote-action.exe'
$private=Join-Path $base 'private'
$state=Get-Content -Raw (Join-Path $private 'site-state.json')|ConvertFrom-Json
$backup=$state.backup_root
$backupManifest=Get-Content -Raw (Join-Path $backup 'backup-manifest.json')|ConvertFrom-Json
$servy='C:\Program Files\Servy\servy-cli.exe'
$registry=Join-Path $productionRoot 'config/registry-production.json'
$runnerService=[string]$state.production_runner_service
$runnerSid=[string]$state.production_runner_sid
if($state.state -ne 'PREPARED' -or $state.runner_sid -eq $state.daemon_sid -or $runnerService -ne 'CoordActionRunnerG7' -or [string]::IsNullOrWhiteSpace($runnerSid)){throw 'Protected-copy rehearsal and current production Runner identity are required.'}
if([IO.Path]::GetDirectoryName($base)-ine [IO.Path]::GetFullPath([string]$backup) -or [IO.Path]::GetFileName($base)-notmatch '^r5-1b-[a-f0-9]{8}$'){throw 'Production staging must be an exact protected-backup child.'}
if(-not(Test-Path -LiteralPath $DatabaseAuditScript -PathType Leaf)){throw 'Read-only database audit script is required.'}
foreach($report in @('site-result.json','site-recovery-result.json')){
 $proof=Get-Content -Raw (Join-Path $private $report)|ConvertFrom-Json
 if($proof.state -ne 'PASS'){throw 'Actual site acceptance and recovery required.'}
}
if(Test-Path (Join-Path $private 'production-deployment.json')){throw 'Inspect existing deployment instead of repeating.'}
$manifest=Get-Content -Raw (Join-Path $UploadRoot 'stage-r5-1b-artifacts.json')|ConvertFrom-Json
if($manifest.state -ne 'PASS' -or $manifest.commit -ne $state.commit -or @($manifest.files).Count -ne 3){throw 'Matching accepted R5-1B artifact required.'}
if($manifest.platform-ne 'windows-2022-x86_64-pc-windows-msvc' -or $manifest.zip.name-ne 'coord-r5-1b-windows-2022.zip' -or [string]$manifest.zip.sha256-ne [string]$state.artifact_zip_sha256){throw 'Windows R5-1B ZIP identity/hash differs from the accepted rehearsal artifact.'}
$artifactZip=Join-Path $UploadRoot ([string]$manifest.zip.name)
if(-not(Test-Path -LiteralPath $artifactZip -PathType Leaf) -or (Get-FileHash -LiteralPath $artifactZip -Algorithm SHA256).Hash.ToLowerInvariant()-ne [string]$manifest.zip.sha256){throw 'Downloaded R5-1B ZIP hash mismatch.'}
foreach($name in @('coord.exe','coord-action-runner.exe','coord-emote-action.exe')){if(@($manifest.files|Where-Object name -eq $name).Count-ne 1){throw 'R5-1B manifest binary set differs from the expected deployment set.'}}
foreach($entry in $manifest.files){if((Get-FileHash (Join-Path $base ('bin/'+$entry.name))).Hash.ToLowerInvariant() -ne $entry.sha256){throw 'Binary drift.'}}
$healthMonitorEntry=$manifest.health_monitor_script
$healthMonitorSource=Join-Path $UploadRoot 'coord-health-monitor.ps1'
if(-not $healthMonitorEntry -or $healthMonitorEntry.name -ne 'coord-health-monitor.ps1' -or [string]$healthMonitorEntry.sha256 -ne [string]$state.artifact_health_monitor_sha256 -or -not(Test-Path -LiteralPath $healthMonitorSource -PathType Leaf) -or (Get-FileHash -LiteralPath $healthMonitorSource -Algorithm SHA256).Hash.ToLowerInvariant() -ne [string]$healthMonitorEntry.sha256){throw 'Matching CI SYSTEM health-monitor script and hash required.'}
if((Get-FileHash $productionCore).Hash.ToLowerInvariant() -ne [string]$backupManifest.installed_binary_sha256){throw 'Installed Core differs from the protected backup.'}
if((Get-FileHash $productionRunner).Hash.ToLowerInvariant() -ne [string]$backupManifest.binary_sha256.runner){throw 'Installed Runner differs from the protected backup.'}
if((Get-FileHash $productionPublisher).Hash.ToLowerInvariant() -ne [string]$backupManifest.binary_sha256.publisher){throw 'Installed Action publisher differs from the protected backup.'}
$currentRunner=Get-CimInstance Win32_Service -Filter "Name='$runnerService'"
if(-not $currentRunner){throw 'Production Runner service is missing.'}
$currentRunnerSid=([Security.Principal.NTAccount]::new([string]$currentRunner.StartName)).Translate([Security.Principal.SecurityIdentifier]).Value
if($currentRunnerSid -ne $runnerSid -or $currentRunner.PathName -notmatch [regex]::Escape($productionRunner)){throw 'Production Runner service identity/path changed after the rehearsal baseline.'}
$currentCore=Get-CimInstance Win32_Service -Filter "Name='coord'"
if(-not $currentCore){throw 'Production Coord Core service is missing.'}
$currentCoreSid=([Security.Principal.NTAccount]::new([string]$currentCore.StartName)).Translate([Security.Principal.SecurityIdentifier]).Value
if($currentCoreSid-ne 'S-1-5-19' -or $currentCore.PathName -notmatch [regex]::Escape($productionCore)){throw 'Production Core identity/path differs from the expected LocalService installation.'}
$coordConfig=Get-Content -Raw (Join-Path $backup 'services/coord.json')|ConvertFrom-Json
if($coordConfig.Name-ne 'coord' -or [IO.Path]::GetFullPath([string]$coordConfig.ExecutablePath)-ine [IO.Path]::GetFullPath($productionCore) -or $coordConfig.StartName-ne $currentCore.StartName){throw 'Protected Core service export does not match the live identity/path.'}
$monitorTask=Get-ScheduledTask -TaskName coord-health-monitor
if($monitorTask.Principal.UserId-ne 'SYSTEM'){throw 'Coord health monitor is not the real SYSTEM task.'}
if($monitorTask.State -eq 'Disabled'){throw 'Coord SYSTEM health monitoring is disabled; inspect its operator state before deployment.'}
if(Get-NetTCPConnection -State Listen -LocalPort 7777,7779 -ErrorAction SilentlyContinue){throw 'Production listener state changed; refresh the live baseline before deployment.'}
$relayBefore=Get-CimInstance Win32_Service -Filter "Name='CoordRelayAgent'"
$servyBefore=Get-CimInstance Win32_Service -Filter "Name='Servy'"
$coreService=Get-Service -Name coord
$runnerServiceState=Get-Service -Name $runnerService
if($coreService.Status -ne 'Stopped' -or $runnerServiceState.Status -ne 'Stopped'){throw 'Production Core/Runner state changed; refresh the read-only baseline before deployment.'}
$key=[IO.File]::ReadAllText((Join-Path $productionRoot 'daemon-private/management.key')).Trim()
function Rpc([string]$Method,$Params,[bool]$Authenticated=$true){
 $headers=@{};if($Authenticated){$headers.Authorization='Bearer '+$key}
 $reply=Invoke-RestMethod 'http://127.0.0.1:7777/' -Method Post -ContentType application/json -Headers $headers -Body (@{jsonrpc='2.0';id=1;method=$Method;params=$Params}|ConvertTo-Json -Depth 30 -Compress) -TimeoutSec 10
 if($reply.error){throw ('RPC failed: '+$Method)};return $reply.result
}
function Get-DatabaseAudit([string]$Path){
 $raw=@(& $DatabaseAuditScript -DbPath $Path)
 if($LASTEXITCODE -ne 0 -or $raw.Count -ne 1){throw 'Read-only deployment database audit failed.'}
 $report=($raw[0]|Out-String|ConvertFrom-Json)
 if($report.state -ne 'PASS' -or $report.database.integrity_check -ne 'ok'){throw 'Deployment database audit/integrity gate failed.'}
 return $report.database
}
function Invoke-SystemHealth([string]$ExpectedStatus){
 $previousLastRun=(Get-ScheduledTaskInfo -TaskName coord-health-monitor).LastRunTime.ToUniversalTime()
 $probeTime=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
 Start-ScheduledTask -TaskName coord-health-monitor
 $health=$null
 foreach($attempt in 1..150){
  if(Test-Path -LiteralPath 'C:\ProgramData\coord\health\status.json'){
   try{$candidate=Get-Content -Raw -LiteralPath 'C:\ProgramData\coord\health\status.json'|ConvertFrom-Json;if([long]$candidate.checked_at_unix_ms -ge $probeTime){$health=$candidate;break}}catch{}
  }
  Start-Sleep -Milliseconds 200
 }
 if(-not $health){throw 'Fresh SYSTEM health snapshot was not written.'}
 $task=$null;$info=$null
 foreach($attempt in 1..50){$task=Get-ScheduledTask -TaskName coord-health-monitor;$info=Get-ScheduledTaskInfo -TaskName coord-health-monitor;if($task.State -eq 'Ready' -and $info.LastRunTime.ToUniversalTime() -gt $previousLastRun -and $info.LastTaskResult -eq 0){break};Start-Sleep -Milliseconds 200}
 if($task.Principal.UserId -ne 'SYSTEM' -or $task.State -ne 'Ready' -or $info.LastTaskResult -ne 0){throw 'Real SYSTEM health task identity/result gate failed.'}
  if($health.status -ne $ExpectedStatus -or $health.checks.tasks_list -ne 'PASS' -or $health.checks.sqlite_read_only -ne 'PASS' -or [DateTimeOffset]::FromUnixTimeMilliseconds([long]$health.checked_at_unix_ms).UtcDateTime -lt $info.LastRunTime.ToUniversalTime().AddSeconds(-5)){throw ('SYSTEM health did not meet the fresh expected '+$ExpectedStatus+' Scheduler/SQLite state.')}
 return [pscustomobject]@{principal=$task.Principal.UserId;last_result=$info.LastTaskResult;last_run_utc=$info.LastRunTime.ToUniversalTime().ToString('o');state=[string]$task.State;proof_time_unix_ms=$probeTime;health=$health}
}
function Wait-ScheduledSystemHealth([string]$ExpectedStatus,[datetime]$AfterRunUtc,[int]$RequiredRuns=2){
 $deadline=[DateTime]::UtcNow.AddSeconds(155);$lastRun=$AfterRunUtc.ToUniversalTime();$runs=@()
 while([DateTime]::UtcNow -lt $deadline -and $runs.Count -lt $RequiredRuns){
  $task=Get-ScheduledTask -TaskName coord-health-monitor
  $info=Get-ScheduledTaskInfo -TaskName coord-health-monitor
  $runUtc=$info.LastRunTime.ToUniversalTime()
  if($runUtc -gt $lastRun -and $task.State -eq 'Ready'){
   $health=Get-Content -Raw -LiteralPath 'C:\ProgramData\coord\health\status.json'|ConvertFrom-Json
    $healthCheckedUtc=[DateTimeOffset]::FromUnixTimeMilliseconds([long]$health.checked_at_unix_ms).UtcDateTime
    if($task.Principal.UserId -ne 'SYSTEM' -or $info.LastTaskResult -ne 0 -or $health.status -ne $ExpectedStatus -or $health.checks.tasks_list -ne 'PASS' -or $health.checks.sqlite_read_only -ne 'PASS' -or $healthCheckedUtc -lt $runUtc.AddSeconds(-5) -or $healthCheckedUtc -gt [DateTime]::UtcNow.AddSeconds(5)) {throw 'Consecutive scheduled SYSTEM health result failed or was stale.'}
   $runs+=@{last_run_utc=$runUtc.ToString('o');last_result=$info.LastTaskResult;status=$health.status;components=$health.components;checked_at_unix_ms=$health.checked_at_unix_ms}
   $lastRun=$runUtc
  }
  Start-Sleep -Milliseconds 500
 }
 if($runs.Count -ne $RequiredRuns){throw 'Two consecutive scheduled SYSTEM health results did not arrive within the bounded wait.'}
 return $runs
}
$fresh=Join-Path $private ('pre-deploy-'+[DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ')+'.db')
& 'C:\Users\lieyanbang\AppData\Local\Programs\Python\Python314\python.exe' (Join-Path $backup 'sqlite-online-backup.py') --source 'C:\ProgramData\coord\data\coord.db' --destination $fresh *> (Join-Path $private 'pre-deploy-backup.json')
if($LASTEXITCODE -ne 0){throw 'Fresh online backup failed.'}
$liveDbAudit=Get-DatabaseAudit 'C:\ProgramData\coord\data\coord.db'
$freshDbAudit=Get-DatabaseAudit $fresh
$liveProtected=ConvertTo-Json -InputObject @($liveDbAudit.protected) -Depth 30 -Compress
$freshProtected=ConvertTo-Json -InputObject @($freshDbAudit.protected) -Depth 30 -Compress
$liveActions=ConvertTo-Json -InputObject $liveDbAudit.completion_action_states -Depth 10 -Compress
$freshActions=ConvertTo-Json -InputObject $freshDbAudit.completion_action_states -Depth 10 -Compress
if($liveProtected -cne $freshProtected -or $liveActions -cne $freshActions){throw 'Fresh online backup does not match live protected Batch/Canary and Action evidence.'}
$backupDbAudit=Get-DatabaseAudit (Join-Path $backup 'coord.db')
$backupProtected=ConvertTo-Json -InputObject @($backupDbAudit.protected) -Depth 30 -Compress
$backupActions=ConvertTo-Json -InputObject $backupDbAudit.completion_action_states -Depth 10 -Compress
if($backupProtected -cne $freshProtected -or $backupActions -cne $freshActions){throw 'Current protected Batch/Canary/Action state drifted from the accepted backup.'}
$priorityAuditPath=Join-Path $private 'pre-deploy-priority-audit.py'
$priorityAudit=@'
import json,sqlite3,sys
from urllib.parse import quote
uri='file:'+quote(sys.argv[1].replace('\\','/'),safe='/:')+'?mode=ro'
db=sqlite3.connect(uri,uri=True,timeout=3)
db.execute('PRAGMA query_only=ON')
pending=[{'priority':priority,'count':count} for priority,count in db.execute("SELECT lower(priority),COUNT(*) FROM tasks WHERE lower(state)='pending' AND lower(priority) IN ('urgent','high') GROUP BY lower(priority) ORDER BY lower(priority)")]
actions={state:count for state,count in db.execute('SELECT state,COUNT(*) FROM completion_actions GROUP BY state')}
print(json.dumps({'pending_urgent_high':pending,'completion_action_states':actions},sort_keys=True))
'@
[IO.File]::WriteAllText($priorityAuditPath,$priorityAudit,[Text.UTF8Encoding]::new($false))
$priorityRaw=& 'C:\Users\lieyanbang\AppData\Local\Programs\Python\Python314\python.exe' $priorityAuditPath $fresh
if($LASTEXITCODE -ne 0){throw 'Read-only urgent/high and Action outbox gate failed.'}
$priorityState=($priorityRaw|Out-String)|ConvertFrom-Json
if(@($priorityState.pending_urgent_high).Count -gt 0){throw 'Urgent/high pending task exists in the exact pre-deployment database; resolve overlap before mutation.'}
$runnerStartupAllowed=@($priorityState.completion_action_states.PSObject.Properties|Where-Object{$_.Name -notin @('succeeded','failed')}).Count -eq 0
$runnerKeyId='g7-'+[guid]::NewGuid().ToString('N')
$privateKeyDirectory=[IO.Path]::GetFullPath([string]$state.production_runner_private_key_directory)
$publicKeyDirectory=[IO.Path]::GetFullPath([string]$state.production_runner_public_key_directory)
if($privateKeyDirectory-ine [IO.Path]::GetFullPath((Join-Path $productionRoot 'runner-private')) -or $publicKeyDirectory-ine [IO.Path]::GetFullPath((Join-Path $productionRoot 'broker-public'))){throw 'Production signing-key directories do not match the approved Coord locations.'}
foreach($path in @('C:\ProgramData','C:\ProgramData\coord',$productionRoot)){$item=Get-Item -LiteralPath $path -Force;if($item.Attributes-band [IO.FileAttributes]::ReparsePoint){throw 'Production signing-key parent path contains a reparse point.'}}
$privateKeyPath=Join-Path $privateKeyDirectory ($runnerKeyId+'.pk8')
$publicKeyPath=Join-Path $publicKeyDirectory ($runnerKeyId+'.pub')
if((Test-Path -LiteralPath $privateKeyDirectory) -or (Test-Path -LiteralPath $publicKeyDirectory) -or (Test-Path -LiteralPath $privateKeyPath) -or (Test-Path -LiteralPath $publicKeyPath)){throw 'Production signing-key path already exists; inspect instead of reusing or changing its ACL.'}
$keyParentAcl=Get-Acl -LiteralPath $productionRoot
$keyParentOwnerSid=([Security.Principal.NTAccount]::new([string]$keyParentAcl.Owner)).Translate([Security.Principal.SecurityIdentifier]).Value
if($keyParentOwnerSid-notin @('S-1-5-18','S-1-5-32-544')){throw 'Production signing-key parent owner is not SYSTEM or Administrators.'}
$directoryMutationMask=[int][Security.AccessControl.FileSystemRights]::Write -bor [int][Security.AccessControl.FileSystemRights]::Modify -bor [int][Security.AccessControl.FileSystemRights]::Delete -bor [int][Security.AccessControl.FileSystemRights]::DeleteSubdirectoriesAndFiles -bor [int][Security.AccessControl.FileSystemRights]::ChangePermissions -bor [int][Security.AccessControl.FileSystemRights]::TakeOwnership
$unapprovedKeyParentWriters=@()
foreach($rule in $keyParentAcl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier])){
 if($rule.AccessControlType-eq [Security.AccessControl.AccessControlType]::Allow -and $rule.IdentityReference.Value-notin @('S-1-5-18','S-1-5-32-544') -and (([int]$rule.FileSystemRights -band $directoryMutationMask)-ne 0)){$unapprovedKeyParentWriters+=@($rule.IdentityReference.Value)}
}
if($unapprovedKeyParentWriters.Count){throw 'Production signing-key parent grants a non-admin identity directory mutation rights.'}
$config=Get-Content -Raw (Join-Path $backup 'config/registry-production.json')|ConvertFrom-Json
if(-not $config.external_runner){throw 'Protected production registry lacks its Runner configuration.'}
$config.external_runner.broker_addr='127.0.0.1:7779'
foreach($legacyField in @('key_file','legacy_key_file')){$config.external_runner.PSObject.Properties.Remove($legacyField)}
$config.external_runner.runner_sid=$runnerSid
$config.external_runner.daemon_sid=$state.daemon_sid
$config.external_runner.runner_key_id=$runnerKeyId
$config.external_runner.runner_public_key_file=$publicKeyPath
if(-not $config.profiles.'g7-canary-v1' -or $config.profiles.'r5-1b-browser-v1'){throw 'Production profile baseline is missing or the new R5 profile already exists.'}
$browserRoot=Join-Path $productionRoot 'r5-1b-canary'
$browser=Join-Path $browserRoot 'business'
if(Test-Path -LiteralPath $browserRoot){throw 'Fresh R5 Canary root already exists; inspect instead of overwriting.'}
$publisherEntry=@($manifest.files|Where-Object name -eq 'coord-emote-action.exe')
if($publisherEntry.Count-ne 1){throw 'Matching candidate Action publisher hash is missing.'}
$profile=($config.profiles.'g7-canary-v1'|ConvertTo-Json -Depth 20|ConvertFrom-Json)
$profile.on_task_completed.executable=$productionPublisher;$profile.on_task_completed.sha256=$publisherEntry[0].sha256
$profile.on_batch_completed.executable=$productionPublisher;$profile.on_batch_completed.sha256=$publisherEntry[0].sha256
$profile.on_task_completed.args=@('--root',$browser);$profile.on_batch_completed.args=@('--root',$browser)
$config.profiles|Add-Member -NotePropertyName 'r5-1b-browser-v1' -NotePropertyValue $profile
$signedConfigPath=Join-Path $private 'registry-signed-production.json'
$config|ConvertTo-Json -Depth 30|Set-Content $signedConfigPath -Encoding utf8NoBOM
$compatConfig=$config|ConvertTo-Json -Depth 30|ConvertFrom-Json
$compatConfig.external_runner=$null
$compatConfigPath=Join-Path $private 'registry-compatibility.json'
$compatConfig|ConvertTo-Json -Depth 30|Set-Content $compatConfigPath -Encoding utf8NoBOM
if($coordConfig.Parameters -match 'actions-config'){throw 'Unexpected existing actions configuration.'}
$coordConfig.Parameters+=' --actions-config "'+$registry+'"'
$coordConfig|ConvertTo-Json -Depth 30|Set-Content (Join-Path $private 'coord-production.json') -Encoding utf8NoBOM
function Stop-Owned([string]$Name){if($Name -notin @('coord',$runnerService)){throw 'Unowned service.'};$s=Get-Service $Name;if($s.Status -ne 'Stopped'){Stop-Service $Name;$s.WaitForStatus('Stopped',[timespan]::FromSeconds(30))}}
function Start-Owned([string]$Name){if($Name -notin @('coord',$runnerService)){throw 'Unowned service.'};& $servy start --name $Name --quiet *> (Join-Path $private ($Name+'-production-start.log'));if($LASTEXITCODE -ne 0){throw ('Service start failed: '+$Name)}}
function Grant-OneFileAce([string]$Path,[string]$Ace){& icacls.exe $Path /grant:r $Ace|Out-Null;if($LASTEXITCODE-ne 0){throw 'Exact new signing-key file ACL update failed.'}}
function New-KeyStoreDirectory([string]$Path,[string[]]$FullControlSids,[string[]]$ReadSids){
 if(Test-Path -LiteralPath $Path){throw 'Refusing to change an existing signing-key directory.'}
 New-Item -ItemType Directory -Path $Path|Out-Null
 & icacls.exe $Path /inheritance:r|Out-Null
 if($LASTEXITCODE-ne 0){throw 'Signing-key directory inheritance protection failed.'}
 foreach($sid in $FullControlSids){Grant-OneFileAce $Path ('*'+$sid+':(OI)(CI)F')}
 foreach($sid in $ReadSids){Grant-OneFileAce $Path ('*'+$sid+':(OI)(CI)RX')}
 $acl=Get-Acl -LiteralPath $Path
 $expected=@($FullControlSids)+@($ReadSids)
 $rules=@($acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier]))
 $expectedInheritance=[int][Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [int][Security.AccessControl.InheritanceFlags]::ObjectInherit
 $readExecuteMask=[int][Security.AccessControl.FileSystemRights]::ReadAndExecute -bor [int][Security.AccessControl.FileSystemRights]::Synchronize
 if(-not $acl.AreAccessRulesProtected -or $rules.Count-ne $expected.Count){throw 'Signing-key directory DACL is not protected/exact.'}
 foreach($rule in $rules){
  $sid=$rule.IdentityReference.Value
  if($rule.AccessControlType-ne [Security.AccessControl.AccessControlType]::Allow -or $rule.IsInherited -or $sid-notin $expected -or [int]$rule.InheritanceFlags-ne $expectedInheritance -or $rule.PropagationFlags-ne [Security.AccessControl.PropagationFlags]::None){throw 'Unexpected signing-key directory ACL entry.'}
  $wanted=if($sid-in $FullControlSids){[int][Security.AccessControl.FileSystemRights]::FullControl}else{$readExecuteMask}
  if([int]$rule.FileSystemRights-ne $wanted){throw 'Signing-key directory rights differ from the exact required rights.'}
 }
 foreach($sid in $expected){if(@($rules|Where-Object{$_.IdentityReference.Value-eq $sid}).Count-ne 1){throw 'Signing-key directory DACL is missing an exact required identity.'}}
 return [pscustomobject]@{path=$Path;allow_sids=@($expected|Sort-Object -Unique);rule_count=$rules.Count;dacl_protected=$acl.AreAccessRulesProtected;runner_read=($ReadSids-contains $runnerSid);localservice_read=($ReadSids-contains 'S-1-5-19')}
}
function Set-KeyStageDirectoryAcl([string]$Path){
 & icacls.exe $Path /inheritance:r|Out-Null
 if($LASTEXITCODE-ne 0){throw 'Private key staging directory inheritance protection failed.'}
 Grant-OneFileAce $Path '*S-1-5-18:(OI)(CI)F'
 Grant-OneFileAce $Path '*S-1-5-32-544:(OI)(CI)F'
 $acl=Get-Acl -LiteralPath $Path
 $rules=@($acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier]))
 if(-not $acl.AreAccessRulesProtected -or $rules.Count-ne 2 -or @($rules|Where-Object{$_.AccessControlType-ne [Security.AccessControl.AccessControlType]::Allow -or $_.IsInherited -or $_.IdentityReference.Value-notin @('S-1-5-18','S-1-5-32-544') -or [int]$_.FileSystemRights-ne [int][Security.AccessControl.FileSystemRights]::FullControl}).Count-ne 0){throw 'Private key staging directory is not restricted to SYSTEM and Administrators.'}
}
function Set-NewKeyFileAcl([string]$Path,[string[]]$FullControlSids,[string[]]$ReadSids){
 & icacls.exe $Path /inheritance:r|Out-Null
 if($LASTEXITCODE-ne 0){throw 'Signing-key file inheritance protection failed.'}
 foreach($sid in $FullControlSids){Grant-OneFileAce $Path ('*'+$sid+':F')}
 foreach($sid in $ReadSids){Grant-OneFileAce $Path ('*'+$sid+':R')}
 $acl=Get-Acl -LiteralPath $Path
 $expected=@($FullControlSids)+@($ReadSids)
 $rules=@($acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier]))
 if(-not $acl.AreAccessRulesProtected -or $rules.Count-ne $expected.Count){throw 'New signing-key file DACL is not protected/exact.'}
 foreach($rule in $rules){
  $sid=$rule.IdentityReference.Value
  if($rule.AccessControlType-ne [Security.AccessControl.AccessControlType]::Allow -or $rule.IsInherited -or $sid-notin $expected){throw 'Unexpected signing-key file ACL entry.'}
  # icacls R maps Generic Read to read data/attributes/ACL plus Synchronize.
  # Accept that exact read mask, while excluding Execute and all write rights.
  $readOnlyMask=[int][Security.AccessControl.FileSystemRights]::Read -bor [int][Security.AccessControl.FileSystemRights]::Synchronize
  $wanted=if($sid-in $FullControlSids){[int][Security.AccessControl.FileSystemRights]::FullControl}else{$readOnlyMask}
  if([int]$rule.FileSystemRights-ne $wanted){throw 'Signing-key file ACE rights differ from the exact required rights.'}
 }
 foreach($sid in $expected){if(@($rules|Where-Object{$_.IdentityReference.Value-eq $sid}).Count-ne 1){throw 'Signing-key file DACL is missing an exact required identity.'}}
 return [pscustomobject]@{allow_sids=@($expected|Sort-Object -Unique);rule_count=$rules.Count;dacl_protected=$acl.AreAccessRulesProtected;runner_read_only=($ReadSids-contains $runnerSid);localservice_read_only=($ReadSids-contains 'S-1-5-19')}
}
$summary=[ordered]@{state='FAIL';phase='begin';commit=$state.commit;backup_root=$backup;rehearsal_root=$base;fresh_snapshot=$fresh;health_monitor_sha256=[string]$healthMonitorEntry.sha256;started_at=[DateTime]::UtcNow.ToString('o');predeployment_database=@{backup_schema_version=$backupDbAudit.schema_version;live_schema_version=$liveDbAudit.schema_version;snapshot_schema_version=$freshDbAudit.schema_version;user_version=$liveDbAudit.user_version;integrity_check=$freshDbAudit.integrity_check;protected=$freshDbAudit.protected;completion_action_states=$priorityState.completion_action_states;pending_urgent_high=$priorityState.pending_urgent_high};runner_startup_allowed=$runnerStartupAllowed;production_runner_service=$runnerService;production_runner_sid=$runnerSid;runner_key=@{key_id=$runnerKeyId;private_acl_allow_sids=@();public_acl_allow_sids=@();private_content_hash='NOT_COMPUTED';public_key_sha256='PENDING'}}
$monitorWasEnabled=$true
try{
 Disable-ScheduledTask -TaskName coord-health-monitor|Out-Null
 Stop-ScheduledTask -TaskName coord-health-monitor
  Stop-Owned $runnerService;Stop-Owned 'coord'
 $summary.phase='install'
  foreach($entry in @(@{source=(Join-Path $base 'bin/coord.exe');destination=$productionCore;name='coord.exe'},@{source=(Join-Path $base 'bin/coord-action-runner.exe');destination=$productionRunner;name='coord-action-runner.exe'},@{source=(Join-Path $base 'bin/coord-emote-action.exe');destination=$productionPublisher;name='coord-emote-action.exe'})){
   Copy-Item -LiteralPath $entry.source -Destination $entry.destination -Force
   $actual=(Get-FileHash -LiteralPath $entry.destination).Hash.ToLowerInvariant()
   $expected=([string](@($manifest.files|Where-Object name -eq $entry.name)[0].sha256)).ToLowerInvariant()
   if($actual -ne $expected){throw ('Installed candidate hash mismatch: '+$entry.name)}
  }
  $compatConfig|ConvertTo-Json -Depth 30|Set-Content -LiteralPath $registry -Encoding utf8NoBOM
 & $servy import --config json --path (Join-Path $private 'coord-production.json') --quiet *> (Join-Path $private 'coord-production-import.log')
 if($LASTEXITCODE -ne 0){throw 'coord configuration import failed.'}
 $productionMonitorPath='C:\ProgramData\coord\health\coord-health-monitor.ps1'
 Copy-Item -LiteralPath $healthMonitorSource -Destination $productionMonitorPath -Force
 if((Get-FileHash -LiteralPath $productionMonitorPath -Algorithm SHA256).Hash.ToLowerInvariant()-ne [string]$healthMonitorEntry.sha256){throw 'Installed SYSTEM health-monitor script hash mismatch.'}
 $task=Get-ScheduledTask -TaskName coord-health-monitor
  $monitorKeyPath=Join-Path $productionRoot 'monitor-private/management.key'
  $args=[string]$task.Actions[0].Arguments
  $keyArgCount=[regex]::Matches($args,'(?i)-RpcAuthKeyFile\b').Count
  if($keyArgCount -gt 1){throw 'Existing SYSTEM monitor action has duplicate RPC-key parameters.'}
  if($keyArgCount -eq 0){$args+=' -RpcAuthKeyFile "'+$monitorKeyPath+'"'}elseif($args -notmatch [regex]::Escape($monitorKeyPath)){throw 'Existing SYSTEM monitor RPC-key path differs from the protected Coord key.'}
 $action=New-ScheduledTaskAction -Execute $task.Actions[0].Execute -Argument $args
 Set-ScheduledTask -TaskName coord-health-monitor -Action $action|Out-Null
 $runnerPath=Join-Path $private 'runner-production.json'
  & $servy export --name $runnerService --config json --path $runnerPath --quiet *> (Join-Path $private 'runner-production-export.log')
 if($LASTEXITCODE -ne 0){throw 'Runner export failed.'}
 $runner=Get-Content -Raw $runnerPath|ConvertFrom-Json
  if($runner.Name -ne $runnerService -or [IO.Path]::GetFullPath([string]$runner.ExecutablePath)-ine [IO.Path]::GetFullPath($productionRunner) -or $runner.StartName -ne $currentRunner.StartName){throw 'Exported production Runner identity or binary path drifted.'}
$runner.Parameters='--registry "'+$registry+'" --private-key-file "'+$privateKeyPath+'"'
 $runner.EnableHealthMonitoring=$true;$runner.HeartbeatInterval=10;$runner.MaxFailedChecks=3;$runner.RecoveryAction=$coordConfig.RecoveryAction;$runner.MaxRestartAttempts=3;$runner.RecoveryOnCleanExit=$false
 $runner|ConvertTo-Json -Depth 30|Set-Content $runnerPath -Encoding utf8NoBOM
 & $servy import --config json --path $runnerPath --quiet *> (Join-Path $private 'runner-production-import.log')
 if($LASTEXITCODE -ne 0){throw 'Runner configuration failed.'}
  & sc.exe config $runnerService start= auto depend= coord *> (Join-Path $private 'runner-production-scm.log')
 if($LASTEXITCODE -ne 0){throw 'Runner dependency failed.'}
 $summary.phase='scheduler-compatibility'
 Enable-ScheduledTask -TaskName coord-health-monitor|Out-Null
 Start-Owned 'coord'
 $compatReady=$false
 foreach($try in 1..100){try{Rpc 'tasks/list' @{limit=1;skip_reclaim=$true}|Out-Null;$compatReady=$true;break}catch{Start-Sleep -Milliseconds 200}}
 if(-not $compatReady){throw 'Authenticated Scheduler did not become ready in Broker-unavailable compatibility mode.'}
 $compatHealth=Invoke-RestMethod 'http://127.0.0.1:7777/healthz' -TimeoutSec 5
 if($compatHealth.status-ne 'DEGRADED' -or $compatHealth.core.status-ne 'READY' -or $compatHealth.database.status-ne 'READY' -or $compatHealth.broker.status-ne 'UNAVAILABLE' -or $compatHealth.runner.status-ne 'UNAVAILABLE'){throw 'Scheduler compatibility mode did not expose explicit Broker/Runner unavailability.'}
 $compatPidBefore=(Get-CimInstance Win32_Service -Filter "Name='coord'").ProcessId
 $compatSystem=Invoke-SystemHealth 'DEGRADED'
 $compatPidAfter=(Get-CimInstance Win32_Service -Filter "Name='coord'").ProcessId
 $migratedDbAudit=Get-DatabaseAudit 'C:\ProgramData\coord\data\coord.db'
 $migratedProtected=ConvertTo-Json -InputObject @($migratedDbAudit.protected) -Depth 30 -Compress
 $snapshotProtected=ConvertTo-Json -InputObject @($freshDbAudit.protected) -Depth 30 -Compress
 $migratedActions=ConvertTo-Json -InputObject $migratedDbAudit.completion_action_states -Depth 10 -Compress
 $snapshotActions=ConvertTo-Json -InputObject $freshDbAudit.completion_action_states -Depth 10 -Compress
 if(-not $migratedDbAudit.runner_nonces_table -or -not $migratedDbAudit.task_attempts_table -or $migratedDbAudit.runner_nonce_rows-ne 0 -or $migratedDbAudit.integrity_check-ne 'ok' -or $migratedDbAudit.user_version-ne $freshDbAudit.user_version -or $migratedProtected-cne $snapshotProtected -or $migratedActions-cne $snapshotActions){throw 'Live additive migration changed protected rows/actions or failed SQLite integrity.'}
 if($compatPidAfter-ne $compatPidBefore){throw 'SYSTEM DEGRADED monitoring restarted the recovered Scheduler unexpectedly.'}
 $summary.scheduler_recovery=@{state='PASS';authenticated_rpc='PASS';health=$compatHealth;system_task=$compatSystem;core_pid_unchanged_during_degraded_monitoring=$true;database_before=@{schema_version=$freshDbAudit.schema_version;user_version=$freshDbAudit.user_version;integrity_check=$freshDbAudit.integrity_check;protected=$freshDbAudit.protected;completion_action_states=$freshDbAudit.completion_action_states};database_after=@{schema_version=$migratedDbAudit.schema_version;user_version=$migratedDbAudit.user_version;integrity_check=$migratedDbAudit.integrity_check;runner_nonces_table=$migratedDbAudit.runner_nonces_table;runner_nonce_rows=$migratedDbAudit.runner_nonce_rows;task_attempts_table=$migratedDbAudit.task_attempts_table;protected=$migratedDbAudit.protected;completion_action_states=$migratedDbAudit.completion_action_states};protected_batches_receipts_actions_unchanged=$true}
 if(-not $runnerStartupAllowed){throw 'Pre-existing nonterminal Action outbox entries forbid starting any Runner; Scheduler remains recovered in Broker-unavailable mode.'}
  $summary.phase='production-runner-key'
  $privateDirectoryAcl=New-KeyStoreDirectory $privateKeyDirectory @('S-1-5-18','S-1-5-32-544') @($runnerSid)
  $publicDirectoryAcl=New-KeyStoreDirectory $publicKeyDirectory @('S-1-5-18','S-1-5-32-544') @('S-1-5-19',$runnerSid)
  $summary.runner_key_directories=@{private=$privateDirectoryAcl;public=$publicDirectoryAcl}
  $keygenLog=Join-Path $private 'production-runner-keygen.log'
  $keyStageRoot=Join-Path $private ('key-staging-'+$runnerKeyId)
  New-Item -ItemType Directory -Path $keyStageRoot|Out-Null
  Set-KeyStageDirectoryAcl $keyStageRoot
  $stagedPrivateKeyPath=Join-Path $keyStageRoot 'runner.pk8'
  $stagedPublicKeyPath=Join-Path $keyStageRoot 'runner.pub'
  & $productionRunner --generate-keypair --private-key-file $stagedPrivateKeyPath --public-key-file $stagedPublicKeyPath *> $keygenLog
  if($LASTEXITCODE-ne 0 -or -not(Test-Path -LiteralPath $stagedPrivateKeyPath -PathType Leaf) -or -not(Test-Path -LiteralPath $stagedPublicKeyPath -PathType Leaf)){throw 'Protected production Runner key generation failed.'}
  $keygenText=[IO.File]::ReadAllText($keygenLog)
  if($keygenText -match '[A-Za-z0-9_+/=-]{64,}'){throw 'Production key-generation output contained key-like material.'}
  $privateAcl=Set-NewKeyFileAcl $stagedPrivateKeyPath @('S-1-5-18','S-1-5-32-544') @($runnerSid)
  $publicAcl=Set-NewKeyFileAcl $stagedPublicKeyPath @('S-1-5-18','S-1-5-32-544') @('S-1-5-19',$runnerSid)
  Move-Item -LiteralPath $stagedPrivateKeyPath -Destination $privateKeyPath
  Move-Item -LiteralPath $stagedPublicKeyPath -Destination $publicKeyPath
  $privateAcl=Set-NewKeyFileAcl $privateKeyPath @('S-1-5-18','S-1-5-32-544') @($runnerSid)
  $publicAcl=Set-NewKeyFileAcl $publicKeyPath @('S-1-5-18','S-1-5-32-544') @('S-1-5-19',$runnerSid)
  $summary.runner_key=@{key_id=$runnerKeyId;private_key_path=$privateKeyPath;public_key_path=$publicKeyPath;private_directory_acl=$privateDirectoryAcl;public_directory_acl=$publicDirectoryAcl;private_acl_allow_sids=$privateAcl.allow_sids;public_acl_allow_sids=$publicAcl.allow_sids;private_file_hash='NOT_COMPUTED';private_dacl_protected=$privateAcl.dacl_protected;runner_private_read_only=$privateAcl.runner_read_only;core_private_read='DENIED_BY_EXACT_DACL';public_dacl_protected=$publicAcl.dacl_protected;core_public_read_only=$publicAcl.localservice_read_only;public_key_sha256=(Get-FileHash -LiteralPath $publicKeyPath -Algorithm SHA256).Hash.ToLowerInvariant()}
  $summary.phase='r5-canary-fixture'
  New-Item -ItemType Directory -Path $browserRoot|Out-Null
  foreach($part in @('refs','task')){Copy-Item -LiteralPath (Join-Path $base ('canary/business/'+$part)) -Destination (Join-Path $browser $part) -Recurse}
  foreach($part in @('results/.receipts','results/review_queue')){New-Item -ItemType Directory -Path (Join-Path $browser $part) -Force|Out-Null}
  $browserResults=Join-Path $browser 'results'
  & icacls.exe $browserResults /inheritance:r|Out-Null
  if($LASTEXITCODE-ne 0){throw 'New R5 Canary output ACL failed.'}
  foreach($ace in @('*S-1-5-18:(OI)(CI)F','*S-1-5-32-544:(OI)(CI)F',('*'+$runnerSid+':(OI)(CI)M'))){& icacls.exe $browserResults /grant:r $ace|Out-Null;if($LASTEXITCODE-ne 0){throw 'New R5 Canary output ACL failed.'}}
 $summary.phase='signed-broker'
 $config|ConvertTo-Json -Depth 30|Set-Content -LiteralPath $registry -Encoding utf8NoBOM
 Stop-Owned 'coord'
 Start-Owned 'coord'
 $brokerReady=$false;$brokerHealth=$null
 foreach($try in 1..100){try{Rpc 'tasks/list' @{limit=1;skip_reclaim=$true}|Out-Null;$brokerHealth=Invoke-RestMethod 'http://127.0.0.1:7777/healthz' -TimeoutSec 2;if($brokerHealth.core.status-eq 'READY' -and $brokerHealth.database.status-eq 'READY' -and $brokerHealth.broker.status-eq 'READY' -and $brokerHealth.runner.status-eq 'UNAVAILABLE'){$brokerReady=$true;break}}catch{Start-Sleep -Milliseconds 200}}
 if(-not $brokerReady){throw 'Signed Broker did not become READY independently of Runner.'}
 $unauth=Invoke-WebRequest 'http://127.0.0.1:7777/' -Method Post -ContentType application/json -Body '{"jsonrpc":"2.0","id":1,"method":"tasks/list","params":{}}' -SkipHttpErrorCheck
 if($unauth.StatusCode -ne 401){throw 'Management RPC is not authenticated.'}
 $summary.broker_recovery=@{state='PASS';health=$brokerHealth;unauthenticated_rpc_status=$unauth.StatusCode;production_runner_key_id=$runnerKeyId;legacy_broker_key_read='NOT_PERFORMED';core_private_key_read='NOT_PERFORMED'}
 $summary.phase='signed-runner'
  Start-Owned $runnerService
 $runnerReady=$false;$runnerHealth=$null
 foreach($try in 1..150){try{$runnerHealth=Invoke-RestMethod 'http://127.0.0.1:7777/healthz' -TimeoutSec 2;if($runnerHealth.status-eq 'READY' -and $runnerHealth.broker.status-eq 'READY' -and $runnerHealth.runner.status-eq 'READY'){$runnerReady=$true;break}}catch{};Start-Sleep -Milliseconds 200}
 if(-not $runnerReady){throw 'Production Runner signature authentication/heartbeat did not reach READY.'}
 Enable-ScheduledTask -TaskName coord-health-monitor|Out-Null
 $systemReady=Invoke-SystemHealth 'READY'
  $corePidBeforeMonitoring=(Get-CimInstance Win32_Service -Filter "Name='coord'").ProcessId
  $runnerPidBeforeMonitoring=(Get-CimInstance Win32_Service -Filter ("Name='"+$runnerService+"'")).ProcessId
  $scheduledRuns=Wait-ScheduledSystemHealth 'READY' ([DateTime]::Parse($systemReady.last_run_utc).ToUniversalTime()) 2
  $corePidAfterMonitoring=(Get-CimInstance Win32_Service -Filter "Name='coord'").ProcessId
  $runnerPidAfterMonitoring=(Get-CimInstance Win32_Service -Filter ("Name='"+$runnerService+"'")).ProcessId
  if($corePidAfterMonitoring-ne $corePidBeforeMonitoring -or $runnerPidAfterMonitoring-ne $runnerPidBeforeMonitoring){throw 'SYSTEM health monitoring restarted Core or Runner during stable READY acceptance.'}
  $finalDbAudit=Get-DatabaseAudit 'C:\ProgramData\coord\data\coord.db'
  $finalProtected=ConvertTo-Json -InputObject @($finalDbAudit.protected) -Depth 30 -Compress
  $finalActions=ConvertTo-Json -InputObject $finalDbAudit.completion_action_states -Depth 10 -Compress
  if($finalProtected-cne $freshProtected -or $finalActions-cne $freshActions){throw 'Production health acceptance changed protected task/receipt or Action state.'}
  $relayAfter=Get-CimInstance Win32_Service -Filter "Name='CoordRelayAgent'"
  $servyAfter=Get-CimInstance Win32_Service -Filter "Name='Servy'"
  if($relayAfter.State-ne $relayBefore.State -or $relayAfter.ProcessId-ne $relayBefore.ProcessId -or $relayAfter.StartName-ne $relayBefore.StartName -or $servyAfter.State-ne $servyBefore.State -or $servyAfter.ProcessId-ne $servyBefore.ProcessId -or $servyAfter.StartName-ne $servyBefore.StartName){throw 'CoordRelayAgent or Servy changed during the scoped Core/Runner recovery.'}
  $summary.state='PASS';$summary.phase='production-ready';$summary.health=$runnerHealth;$summary.system_monitor=@{initial=$systemReady;consecutive_scheduled_runs=$scheduledRuns;core_pid_unchanged=$true;runner_pid_unchanged=$true}
  $summary.final_database=@{integrity_check=$finalDbAudit.integrity_check;protected=$finalDbAudit.protected;completion_action_states=$finalDbAudit.completion_action_states;protected_batch_canary_receipt_and_action_state_unchanged=$true}
  $summary.binary_sha256=@{core=(Get-FileHash $productionCore -Algorithm SHA256).Hash.ToLowerInvariant();runner=(Get-FileHash $productionRunner -Algorithm SHA256).Hash.ToLowerInvariant();publisher=(Get-FileHash $productionPublisher -Algorithm SHA256).Hash.ToLowerInvariant()}
  $summary.production_health_monitor_sha256=(Get-FileHash -LiteralPath $productionMonitorPath -Algorithm SHA256).Hash.ToLowerInvariant()
  $summary.services=@(Get-CimInstance Win32_Service|Where-Object Name -in @('coord',$runnerService,'CoordRelayAgent','Servy')|Select-Object Name,State,StartMode,StartName,ProcessId)
 $summary.monitor=@{principal=(Get-ScheduledTask coord-health-monitor).Principal.UserId;last_result=(Get-ScheduledTaskInfo coord-health-monitor).LastTaskResult}
}catch{
 $summary.failure=$_.Exception.Message
 # Recover only to the new Core's authenticated Scheduler-compatible mode.
 # Never start the old G7 Core, which still needs the protected legacy broker.key.
 try{
  foreach($name in @($runnerService,'coord')){Stop-Owned $name}
  Set-Service -Name $runnerService -StartupType Manual
  $candidateCore=Join-Path $base 'bin/coord.exe'
  Copy-Item -LiteralPath $candidateCore -Destination 'C:\ProgramData\coord\bin\coord.exe' -Force
  if((Get-FileHash -LiteralPath $candidateCore).Hash -ne (Get-FileHash -LiteralPath 'C:\ProgramData\coord\bin\coord.exe').Hash){throw 'Compatibility Core hash did not verify.'}
  $compatConfig|ConvertTo-Json -Depth 30|Set-Content -LiteralPath $registry -Encoding utf8NoBOM
  Start-Owned 'coord'
  $compatReady=$false
  foreach($try in 1..100){try{Rpc 'tasks/list' @{limit=1;skip_reclaim=$true}|Out-Null;$compatReady=$true;break}catch{Start-Sleep -Milliseconds 200}}
  if(-not $compatReady){throw 'New Core did not recover authenticated Scheduler readiness in compatibility mode.'}
  $compatHealth=Invoke-RestMethod 'http://127.0.0.1:7777/healthz' -TimeoutSec 5
  if($compatHealth.status -ne 'DEGRADED' -or $compatHealth.core.status -ne 'READY' -or $compatHealth.database.status -ne 'READY' -or $compatHealth.broker.status -ne 'UNAVAILABLE'){throw 'Compatibility mode did not expose an explicit Broker-unavailable state.'}
  Enable-ScheduledTask -TaskName coord-health-monitor|Out-Null
  $compatSystem=Invoke-SystemHealth 'DEGRADED'
  $summary.relay_service='LEFT_RUNNING_AND_UNCHANGED'
  $summary.state='PARTIAL'
  $summary.rollback='new Core running in authenticated Scheduler compatibility mode; Broker explicitly UNAVAILABLE; Runner stopped/manual; live DB preserved'
  $summary.compatibility_health=$compatHealth;$summary.compatibility_system_task=$compatSystem
 }catch{$summary.rollback='FAILED: '+$_.Exception.Message}
}finally{
 if($monitorWasEnabled){
  try{
   $taskAfter=Get-ScheduledTask -TaskName coord-health-monitor
   if($taskAfter.State -eq 'Disabled'){Enable-ScheduledTask -TaskName coord-health-monitor|Out-Null}
   $taskAfter=Get-ScheduledTask -TaskName coord-health-monitor
   if($taskAfter.State -eq 'Disabled'){throw 'SYSTEM health task remained disabled after deployment recovery.'}
   $summary.monitor_task_restored_enabled=$true
   $summary.monitor_task_last_result=(Get-ScheduledTaskInfo -TaskName coord-health-monitor).LastTaskResult
  }catch{
   $summary.state='FAIL'
   $summary.monitor_task_restore_error=$_.Exception.Message
  }
 }
 $summary.finished_at=[DateTime]::UtcNow.ToString('o')
 $summary|ConvertTo-Json -Depth 20|Set-Content (Join-Path $private 'production-deployment.json') -Encoding utf8NoBOM
 $summary|ConvertTo-Json -Depth 20
}
if($summary.state -ne 'PASS'){throw 'Controlled Windows deployment failed; inspect redacted evidence.'}

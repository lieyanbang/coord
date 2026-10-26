param(
 [Parameter(Mandatory)][string]$RehearsalRoot,
 [switch]$RequireCanaryComplete
)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$base=[IO.Path]::GetFullPath($RehearsalRoot)
$productionRoot='C:\ProgramData\coord\g7'
$private=Join-Path $base 'private'
$state=Get-Content -Raw (Join-Path $private 'site-state.json')|ConvertFrom-Json
if($state.base-ne $base -or [IO.Path]::GetDirectoryName($base)-ine [IO.Path]::GetFullPath([string]$state.backup_root) -or [IO.Path]::GetFileName($base)-notmatch '^r5-1b-[a-f0-9]{8}$'){throw 'Exact protected-copy rehearsal state is required.'}
$deployment=Get-Content -Raw (Join-Path $private 'production-deployment.json')|ConvertFrom-Json
if($deployment.state-ne 'PASS'){throw 'Production B3 deployment must be PASS before this verifier.'}
$runnerService=[string]$state.production_runner_service
$runnerSid=[string]$state.production_runner_sid
$key=[IO.File]::ReadAllText((Join-Path $productionRoot 'daemon-private/management.key')).Trim()
function Rpc([string]$Method,$Params){
 $r=Invoke-RestMethod 'http://127.0.0.1:7777/' -Method Post -ContentType application/json -Headers @{Authorization=('Bearer '+$key)} -Body (@{jsonrpc='2.0';id=1;method=$Method;params=$Params}|ConvertTo-Json -Depth 30 -Compress)
 if($r.error){throw ('RPC failed: '+$Method)};return $r.result
}
function Assert-KeyAcl([string]$Path,[string[]]$FullControlSids,[string[]]$ReadSids,[switch]$Directory){
 $item=Get-Item -LiteralPath $Path -Force
 if(($item.Attributes-band [IO.FileAttributes]::ReparsePoint)-ne 0){throw 'Signing-key object is a reparse point.'}
 $acl=Get-Acl -LiteralPath $Path
 $expected=@($FullControlSids)+@($ReadSids)
 $rules=@($acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier]))
 if(-not $acl.AreAccessRulesProtected -or $rules.Count-ne $expected.Count){throw 'Signing-key DACL is not protected/exact.'}
 $readMask=[int][Security.AccessControl.FileSystemRights]::Read -bor [int][Security.AccessControl.FileSystemRights]::Synchronize
 $directoryMask=[int][Security.AccessControl.FileSystemRights]::ReadAndExecute -bor [int][Security.AccessControl.FileSystemRights]::Synchronize
 $directoryInheritance=[int][Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [int][Security.AccessControl.InheritanceFlags]::ObjectInherit
 foreach($rule in $rules){
  $sid=$rule.IdentityReference.Value
  if($rule.AccessControlType-ne [Security.AccessControl.AccessControlType]::Allow -or $rule.IsInherited -or $sid-notin $expected){throw 'Unexpected signing-key DACL entry.'}
  if($Directory){
   if([int]$rule.InheritanceFlags-ne $directoryInheritance -or $rule.PropagationFlags-ne [Security.AccessControl.PropagationFlags]::None){throw 'Signing-key directory inheritance differs from the exact policy.'}
   $wanted=if($sid-in $FullControlSids){[int][Security.AccessControl.FileSystemRights]::FullControl}else{$directoryMask}
  }else{
   if($rule.InheritanceFlags-ne [Security.AccessControl.InheritanceFlags]::None -or $rule.PropagationFlags-ne [Security.AccessControl.PropagationFlags]::None){throw 'Signing-key file ACE unexpectedly inherits.'}
   $wanted=if($sid-in $FullControlSids){[int][Security.AccessControl.FileSystemRights]::FullControl}else{$readMask}
  }
  if([int]$rule.FileSystemRights-ne $wanted){throw 'Signing-key rights differ from the exact policy.'}
 }
 foreach($sid in $expected){if(@($rules|Where-Object{$_.IdentityReference.Value-eq $sid}).Count-ne 1){throw 'Signing-key DACL is missing an exact identity.'}}
 return [pscustomobject]@{path=$Path;allow_sids=@($expected|Sort-Object -Unique);rule_count=$rules.Count;dacl_protected=$acl.AreAccessRulesProtected;private_content_read='NOT_PERFORMED'}
}
$keyInfo=$deployment.runner_key
$expectedPrivateDirectory=[IO.Path]::GetFullPath((Join-Path $productionRoot 'runner-private'))
$expectedPublicDirectory=[IO.Path]::GetFullPath((Join-Path $productionRoot 'broker-public'))
if([IO.Path]::GetFullPath([string]$keyInfo.private_key_path)-ine [IO.Path]::GetFullPath((Join-Path $expectedPrivateDirectory ($keyInfo.key_id+'.pk8'))) -or [IO.Path]::GetFullPath([string]$keyInfo.public_key_path)-ine [IO.Path]::GetFullPath((Join-Path $expectedPublicDirectory ($keyInfo.key_id+'.pub')))){throw 'Production signing-key paths differ from the recorded R5 key id.'}
$privateDirectoryAcl=Assert-KeyAcl $expectedPrivateDirectory @('S-1-5-18','S-1-5-32-544') @($runnerSid) -Directory
$publicDirectoryAcl=Assert-KeyAcl $expectedPublicDirectory @('S-1-5-18','S-1-5-32-544') @('S-1-5-19',$runnerSid) -Directory
$privateFileAcl=Assert-KeyAcl ([string]$keyInfo.private_key_path) @('S-1-5-18','S-1-5-32-544') @($runnerSid)
$publicFileAcl=Assert-KeyAcl ([string]$keyInfo.public_key_path) @('S-1-5-18','S-1-5-32-544') @('S-1-5-19',$runnerSid)
$publicKeyHash=(Get-FileHash -LiteralPath ([string]$keyInfo.public_key_path) -Algorithm SHA256).Hash.ToLowerInvariant()
if($publicKeyHash-ne [string]$keyInfo.public_key_sha256){throw 'Production public-key hash differs from the accepted deployment report.'}
$compare=@'
import sqlite3,sys,json,hashlib
from urllib.parse import quote
def ro(p):return sqlite3.connect('file:'+quote(p.replace('\\','/'),safe='/:')+'?mode=ro',uri=True)
with ro(sys.argv[1]) as a,ro(sys.argv[2]) as b:
    columns=[row[1] for row in a.execute('PRAGMA table_info(tasks)')]
    query='SELECT '+','.join('"'+c.replace('"','""')+'"' for c in columns)+' FROM tasks'
    rows=a.execute(query).fetchall()
    for row in rows:
        current=b.execute(query+' WHERE id=?',(row[columns.index('id')],)).fetchone()
        assert current==row,'Original task fields changed'
    quick=b.execute('PRAGMA quick_check').fetchall()
    assert quick==[('ok',)]
    print(json.dumps({'state':'PASS','original_tasks':len(rows),'original_columns':len(columns),'all_original_task_fields_match':True,'quick_check':'ok','original_field_fingerprint':hashlib.sha256(json.dumps(rows,sort_keys=True).encode()).hexdigest()}))
'@
$comparePath=Join-Path $private 'compare-production.py'
$compare|Set-Content $comparePath -Encoding utf8NoBOM
$raw=& 'C:\Users\lieyanbang\AppData\Local\Programs\Python\Python314\python.exe' $comparePath (Join-Path $state.backup_root 'coord.db') 'C:\ProgramData\coord\data\coord.db'
if($LASTEXITCODE -ne 0){throw 'Original task preservation failed.'}
$comparison=$raw|ConvertFrom-Json
$batchesPath=Join-Path $private 'canary-batches.json'
$batches=$null;$actions=$null;$operatorProgress=$null;$browserProgress=$null
if(Test-Path -LiteralPath $batchesPath){
 $batches=Get-Content -Raw -LiteralPath $batchesPath|ConvertFrom-Json
 if($batches.operator_batch_id){$actions=Rpc 'actions/status' @{batch_id=$batches.operator_batch_id};$operatorProgress=Rpc 'batches/status' @{batch_id=$batches.operator_batch_id}}
 if($batches.browser_batch_id){$browserProgress=Rpc 'batches/status' @{batch_id=$batches.browser_batch_id}}
}elseif($RequireCanaryComplete){throw 'R5 Canary evidence is not present.'}
$originalServices=Get-Content -Raw (Join-Path $state.backup_root 'services/baseline.json')|ConvertFrom-Json
$current=Get-CimInstance Win32_Service
$protected=@('ArasakaDDNS','ArasakaFFmpegWatchdog','ArasakaFFmpegWorker','CodexProxy','MediaSTT','OCI-A1-Grabber','Servy')
$serviceProof=@()
foreach($name in $protected){
 $old=$originalServices|Where-Object Name -eq $name;$now=$current|Where-Object Name -eq $name
 if(-not $old -or -not $now -or $old.State -ne $now.State -or $old.StartName -ne $now.StartName -or $old.StartMode -ne $now.StartMode -or $old.ProcessId -ne $now.ProcessId){throw ('Unrelated service drift: '+$name)}
 $serviceProof+=@{name=$name;state=$now.State;pid=$now.ProcessId;same_identity_start_mode_pid=$true}
}
if(Get-NetTCPConnection -LocalPort 17777,17778,17779 -State Listen -ErrorAction SilentlyContinue){throw 'Owned rehearsal listener remains.'}
$hashes=@{}
foreach($entry in $state.artifact_files){
 $path=switch($entry.name){'coord.exe'{'C:\ProgramData\coord\bin\coord.exe'}'coord-action-runner.exe'{Join-Path $productionRoot 'bin/coord-action-runner.exe'}'coord-emote-action.exe'{Join-Path $productionRoot 'bin/coord-emote-action.exe'}default{throw 'Unexpected staged artifact name.'}}
 $hash=(Get-FileHash -LiteralPath $path).Hash.ToLowerInvariant()
 if($hash-ne [string]$entry.sha256){throw ('Installed production artifact differs from accepted CI: '+$entry.name)}
 $hashes[$entry.name]=$hash
}
$r5CanaryResults=Join-Path $productionRoot 'r5-1b-canary/business/results'
$published=@(Get-ChildItem -LiteralPath $r5CanaryResults -File -ErrorAction SilentlyContinue|ForEach-Object{@{name=$_.Name;bytes=$_.Length;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}})
$health=Get-Content -Raw 'C:\ProgramData\coord\health\status.json'|ConvertFrom-Json
$healthMonitorPath='C:\ProgramData\coord\health\coord-health-monitor.ps1'
$healthMonitorHash=(Get-FileHash -LiteralPath $healthMonitorPath -Algorithm SHA256).Hash.ToLowerInvariant()
if($healthMonitorHash-ne [string]$state.artifact_health_monitor_sha256){throw 'Installed SYSTEM health-monitor script differs from the matched CI artifact.'}
$runtimeLog=Join-Path $productionRoot 'runtime/logs/runner.stderr.log'
$runtimeIdentity=@(Get-Content -LiteralPath $runtimeLog|Where-Object{$_ -match '^\{"sid":'}|ForEach-Object{$_|ConvertFrom-Json})|Select-Object -Last 1
$runtimeService=Get-CimInstance Win32_Service -Filter ("Name='"+$runnerService+"'")
$runtimeSid=([Security.Principal.NTAccount]::new([string]$runtimeService.StartName)).Translate([Security.Principal.SecurityIdentifier]).Value
if(-not $runtimeIdentity -or $runtimeIdentity.sid -ne $runnerSid -or $runtimeSid-ne $runnerSid -or $runtimeIdentity.privileged -ne $false){throw 'Current installed Runner token identity failed.'}
$coreService=Get-CimInstance Win32_Service -Filter "Name='coord'"
$coreSid=([Security.Principal.NTAccount]::new([string]$coreService.StartName)).Translate([Security.Principal.SecurityIdentifier]).Value
if($coreSid-ne 'S-1-5-19'){throw 'Current installed Core identity differs from the accepted LocalService boundary.'}
if($health.status -ne 'READY' -or $health.core.status-ne 'READY' -or $health.database.status-ne 'READY' -or $health.broker.status-ne 'READY' -or $health.runner.status-ne 'READY' -or [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()-$health.checked_at_unix_ms -gt 120000){throw 'Fresh production Core/DB/Broker/Runner READY required.'}
if($RequireCanaryComplete){
 if(-not $batches -or -not $batches.operator_batch_id -or -not $batches.browser_batch_id){throw 'Both new R5 Canary batches must be recorded.'}
 $progress=$operatorProgress
 if($progress.total -ne 10 -or $progress.states.completed -ne 10 -or $progress.active -ne 0){throw 'Production synthetic canary incomplete.'}
 if(@($actions.actions).Count -ne 11 -or @($actions.actions|Where-Object state -ne 'succeeded').Count){throw 'All ten publish actions and aggregate action must succeed.'}
 foreach($relative in @('index.json','review_queue/candidates.json')){if(-not(Test-Path (Join-Path $r5CanaryResults $relative))){throw 'Aggregate R5 Canary files missing.'}}
}
$summary=@{state='PASS';observed_at=[DateTime]::UtcNow.ToString('o');original_database=$comparison;unchanged_services=$serviceProof;production_services=@($current|Where-Object Name -in @('coord','CoordRelayAgent',$runnerService)|Select-Object Name,State,StartName,ProcessId);health=$health;hashes=$hashes;health_monitor_sha256=$healthMonitorHash;operator_progress=$operatorProgress;browser_progress=$browserProgress;actions=$actions;published=$published;runner_identity_log=(Get-Content -Raw (Join-Path $private 'service-identity.stderr.log'));runner_acl_log=(Get-Content -Raw (Join-Path $private 'service-acl-rpc.stderr.log'));runner_privileges=(& sc.exe qprivs $runnerService|Out-String);test_listeners_released=$true;formal_classification_created=$false}
$summary.current_runner_identity=$runtimeIdentity
$summary.runner_key_acl=@{private_directory=$privateDirectoryAcl;private_file=$privateFileAcl;public_directory=$publicDirectoryAcl;public_file=$publicFileAcl;public_key_sha256=$publicKeyHash;private_key_content='NOT_READ';legacy_broker_key_content='NOT_READ'}
$probeBody='{"jsonrpc":"2.0","id":1,"method":"tasks/list","params":{"limit":1,"skip_reclaim":true}}'
$missing=Invoke-WebRequest 'http://127.0.0.1:7777/' -Method Post -ContentType application/json -Body $probeBody -SkipHttpErrorCheck
$unrelatedCredentialDenied=Invoke-WebRequest 'http://127.0.0.1:7777/' -Method Post -ContentType application/json -Headers @{Authorization='Bearer unrelated-runner-credential'} -Body $probeBody -SkipHttpErrorCheck
if($missing.StatusCode -ne 401 -or $unrelatedCredentialDenied.StatusCode -ne 401){throw 'Current management credential separation failed.'}
$summary.management_rejections=@{missing=$missing.StatusCode;unrelated_credential=$unrelatedCredentialDenied.StatusCode;legacy_broker_key_read='NOT_PERFORMED'}
$candidatePath=Join-Path $r5CanaryResults 'review_queue/candidates.json'
if(Test-Path -LiteralPath $candidatePath){$summary.review_candidates=@{sha256=(Get-FileHash $candidatePath).Hash.ToLowerInvariant();bytes=(Get-Item $candidatePath).Length}}else{$summary.review_candidates='NOT_YET_GENERATED'}
$listeners=@(Get-NetTCPConnection -LocalPort 7777,7779 -State Listen)
if($listeners.Count -ne 2 -or @($listeners|Where-Object LocalAddress -ne '127.0.0.1').Count){throw 'Both authenticated RPC and broker must bind exact loopback.'}
$daemonProcess=Get-CimInstance Win32_Process -Filter ('ProcessId='+($listeners|Where-Object LocalPort -eq 7777).OwningProcess)
if($daemonProcess.ExecutablePath -ne 'C:\ProgramData\coord\bin\coord.exe'){throw 'Actual daemon executable path mismatch.'}
$summary.actual_listener_binary=@{path=$daemonProcess.ExecutablePath;pid=$daemonProcess.ProcessId;sha256=(Get-FileHash $daemonProcess.ExecutablePath).Hash.ToLowerInvariant()}
if($summary.actual_listener_binary.sha256 -ne $hashes['coord.exe']){throw 'Actual daemon and candidate hashes differ.'}
$summary.monitor=@{principal=(Get-ScheduledTask coord-health-monitor).Principal.UserId;last_exit=(Get-ScheduledTaskInfo coord-health-monitor).LastTaskResult;state=(Get-ScheduledTask coord-health-monitor).State.ToString()}
if($summary.monitor.principal -ne 'SYSTEM' -or $summary.monitor.last_exit -ne 0){throw 'Current SYSTEM monitor failed.'}
$summary|ConvertTo-Json -Depth 30

param([switch]$RequireCanaryComplete)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$base='C:\ProgramData\coord\g7'
$private=Join-Path $base 'private'
$state=Get-Content -Raw (Join-Path $private 'site-state.json')|ConvertFrom-Json
$key=[IO.File]::ReadAllText((Join-Path $base 'daemon-private/management.key')).Trim()
function Rpc([string]$Method,$Params){
 $r=Invoke-RestMethod 'http://127.0.0.1:7777/' -Method Post -ContentType application/json -Headers @{Authorization=('Bearer '+$key)} -Body (@{jsonrpc='2.0';id=1;method=$Method;params=$Params}|ConvertTo-Json -Depth 30 -Compress)
 if($r.error){throw ('RPC failed: '+$Method)};return $r.result
}
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
$batches=Get-Content -Raw (Join-Path $private 'canary-batches.json')|ConvertFrom-Json
$actions=Rpc 'actions/status' @{batch_id=$batches.operator_batch_id}
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
foreach($name in @('coord.exe','coord-action-runner.exe','coord-emote-action.exe')){$hashes[$name]=(Get-FileHash (Join-Path $base ('bin/'+$name))).Hash.ToLowerInvariant()}
$published=@(Get-ChildItem (Join-Path $base 'canary/business/results') -File|ForEach-Object{@{name=$_.Name;bytes=$_.Length;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}})
$health=Get-Content -Raw 'C:\ProgramData\coord\health\status.json'|ConvertFrom-Json
$runtimeIdentity=@(Get-Content (Join-Path $base 'runtime/logs/runner.stderr.log')|Where-Object{$_ -match '^\{"sid":'}|ForEach-Object{$_|ConvertFrom-Json})|Select-Object -Last 1
if(-not $runtimeIdentity -or $runtimeIdentity.sid -ne $state.runner_sid -or $runtimeIdentity.privileged -ne $false){throw 'Current installed Runner token identity failed.'}
if($health.status -ne 'READY' -or [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()-$health.checked_at_unix_ms -gt 120000){throw 'Fresh production READY required.'}
if($RequireCanaryComplete){
 $progress=Rpc 'batches/status' @{batch_id=$batches.operator_batch_id}
 if($progress.total -ne 10 -or $progress.states.completed -ne 10 -or $progress.active -ne 0){throw 'Production synthetic canary incomplete.'}
 if(@($actions.actions).Count -ne 11 -or @($actions.actions|Where-Object state -ne 'succeeded').Count){throw 'All ten publish actions and aggregate action must succeed.'}
 foreach($relative in @('index.json','review_queue/candidates.json')){if(-not(Test-Path (Join-Path $base ('canary/business/results/'+$relative)))){throw 'Aggregate files missing.'}}
}
$summary=@{state='PASS';observed_at=[DateTime]::UtcNow.ToString('o');original_database=$comparison;unchanged_services=$serviceProof;production_services=@($current|Where-Object Name -in @('coord','CoordRelayAgent','CoordActionRunnerG7')|Select-Object Name,State,StartName,ProcessId);health=$health;hashes=$hashes;operator_progress=(Rpc 'batches/status' @{batch_id=$batches.operator_batch_id});browser_progress=(Rpc 'batches/status' @{batch_id=$batches.browser_batch_id});actions=$actions;published=$published;runner_identity_log=(Get-Content -Raw (Join-Path $private 'service-identity.stderr.log'));runner_acl_log=(Get-Content -Raw (Join-Path $private 'service-acl-rpc.stderr.log'));runner_privileges=(& sc.exe qprivs CoordActionRunnerG7|Out-String);test_listeners_released=$true;formal_classification_created=$false}
$summary.current_runner_identity=$runtimeIdentity
$probeBody='{"jsonrpc":"2.0","id":1,"method":"tasks/list","params":{"limit":1,"skip_reclaim":true}}'
$missing=Invoke-WebRequest 'http://127.0.0.1:7777/' -Method Post -ContentType application/json -Body $probeBody -SkipHttpErrorCheck
$broker=[IO.File]::ReadAllText((Join-Path $base 'config/broker.key')).Trim()
$brokerDenied=Invoke-WebRequest 'http://127.0.0.1:7777/' -Method Post -ContentType application/json -Headers @{Authorization=('Bearer '+$broker)} -Body $probeBody -SkipHttpErrorCheck
if($missing.StatusCode -ne 401 -or $brokerDenied.StatusCode -ne 401){throw 'Current management credential separation failed.'}
$summary.management_rejections=@{missing=$missing.StatusCode;runner_broker_credential=$brokerDenied.StatusCode}
$candidatePath=Join-Path $base 'canary/business/results/review_queue/candidates.json'
$summary.review_candidates=@{sha256=(Get-FileHash $candidatePath).Hash.ToLowerInvariant();bytes=(Get-Item $candidatePath).Length}
$listeners=@(Get-NetTCPConnection -LocalPort 7777,7779 -State Listen)
if($listeners.Count -ne 2 -or @($listeners|Where-Object LocalAddress -ne '127.0.0.1').Count){throw 'Both authenticated RPC and broker must bind exact loopback.'}
$daemonProcess=Get-CimInstance Win32_Process -Filter ('ProcessId='+($listeners|Where-Object LocalPort -eq 7777).OwningProcess)
if($daemonProcess.ExecutablePath -ne 'C:\ProgramData\coord\bin\coord.exe'){throw 'Actual daemon executable path mismatch.'}
$summary.actual_listener_binary=@{path=$daemonProcess.ExecutablePath;pid=$daemonProcess.ProcessId;sha256=(Get-FileHash $daemonProcess.ExecutablePath).Hash.ToLowerInvariant()}
if($summary.actual_listener_binary.sha256 -ne $hashes['coord.exe']){throw 'Actual daemon and candidate hashes differ.'}
$summary.monitor=@{principal=(Get-ScheduledTask coord-health-monitor).Principal.UserId;last_exit=(Get-ScheduledTaskInfo coord-health-monitor).LastTaskResult;state=(Get-ScheduledTask coord-health-monitor).State.ToString()}
if($summary.monitor.principal -ne 'SYSTEM' -or $summary.monitor.last_exit -ne 0){throw 'Current SYSTEM monitor failed.'}
$summary|ConvertTo-Json -Depth 30

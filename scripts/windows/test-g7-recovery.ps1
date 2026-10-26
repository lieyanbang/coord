param([Parameter(Mandatory)][string]$RehearsalRoot)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$b=[IO.Path]::GetFullPath($RehearsalRoot)
$state=Get-Content -Raw -LiteralPath (Join-Path $b 'private/site-state.json')|ConvertFrom-Json
$site=Get-Content -Raw -LiteralPath (Join-Path $b 'private/site-result.json')|ConvertFrom-Json
if($site.state-ne 'PASS' -or $state.base-ne $b -or [IO.Path]::GetDirectoryName($b)-ine [IO.Path]::GetFullPath([string]$state.backup_root) -or [IO.Path]::GetFileName($b)-notmatch '^r5-1b-[a-f0-9]{8}$'){throw 'Accepted protected-copy rehearsal site gate required.'}
$private=Join-Path $b 'private'
$servy='C:\Program Files\Servy\servy-cli.exe'
$backupManifest=Get-Content -Raw -LiteralPath (Join-Path $state.backup_root 'backup-manifest.json')|ConvertFrom-Json
if($backupManifest.state-ne 'PASS' -or [IO.Path]::GetFullPath([string]$backupManifest.backup_root)-ine [IO.Path]::GetFullPath([string]$state.backup_root)){throw 'Verified exact rollback backup manifest required.'}
$key=[IO.File]::ReadAllText((Join-Path $b 'daemon-private/management.key')).Trim()
$summary=[ordered]@{state='FAIL';phase='isolated-runner-crash';scope='candidate services and copied database only';production_database_modified=$false}
function Rpc([string]$Method,$Params){
 $body=@{jsonrpc='2.0';id=1;method=$Method;params=$Params}|ConvertTo-Json -Depth 30 -Compress
 $response=Invoke-RestMethod -Uri $state.probe_url -Method Post -ContentType 'application/json' -Headers @{Authorization=('Bearer '+$key)} -Body $body -TimeoutSec 10
 if($response.error){throw ('Candidate RPC failed: '+$Method)}
 return $response.result
}
function Owned-Runner{
 $hostProcess=(Get-CimInstance Win32_Service -Filter ("Name='"+$state.runner_service+"'")).ProcessId
 $matches=@(Get-CimInstance Win32_Process -Filter ("Name='coord-action-runner.exe' AND ParentProcessId="+$hostProcess))
 if($matches.Count-ne 1){throw 'Cannot identify one child of the exact owned Runner service.'}
 return $matches[0]
}
$rollbackProcess=$null
try{
 $batch=Rpc 'batches/create' @{name='G7 isolated non-idempotent service crash';config=@{action_profile='g7-crash-probe-v1'}}
 Rpc 'batches/add_tasks' @{batch_id=$batch.id;tasks=@(@{key='g7-crash-only';name='Pinned loopback ping fixture';payload=@{}})}|Out-Null
 $task=(Rpc 'scheduler/claim' @{batch_id=$batch.id;agent_id='emote-0000';lease_seconds=30}).task
 Rpc 'scheduler/submit' @{id=$task.id;agent_id='emote-0000';lease_token=$task.lease_token;result=@{synthetic=$true}}|Out-Null
 $child=$null;$runnerProcess=$null
 foreach($try in 1..100){
  $runnerProcess=Owned-Runner
  $child=Get-CimInstance Win32_Process -Filter ("Name='ping-fixture.exe' AND ParentProcessId="+$runnerProcess.ProcessId)|Select-Object -First 1
  if($child){break};Start-Sleep -Milliseconds 100
 }
 if(-not $child){throw 'Pinned child did not start under the real Runner service.'}
 $action=(Rpc 'actions/status' @{batch_id=$batch.id}).actions|Select-Object -First 1
 if($action.state-ne 'running'){throw 'Crash action is not running.'}
 Stop-Process -Id $runnerProcess.ProcessId -Force -Confirm:$false
 foreach($try in 1..50){if(-not(Get-Process -Id $child.ProcessId -ErrorAction SilentlyContinue)){break};Start-Sleep -Milliseconds 100}
 if(Get-Process -Id $child.ProcessId -ErrorAction SilentlyContinue){throw 'Runner crash did not terminate its Job child.'}
 $db=Join-Path $b 'verification/state.db'
 if([IO.Path]::GetFullPath($db)-ine [IO.Path]::GetFullPath((Join-Path $b 'verification/state.db'))){throw 'Refusing lease injection outside the exact copied database.'}
 $expire=@'
import os,sqlite3,sys
expected=os.path.normcase(os.path.abspath(os.path.join(sys.argv[3],'verification','state.db')))
assert os.path.normcase(os.path.abspath(sys.argv[1]))==expected
with sqlite3.connect(sys.argv[1]) as db:
    changed=db.execute("UPDATE completion_actions SET lease_until='2000-01-01T00:00:00Z' WHERE id=? AND state='running'",(sys.argv[2],)).rowcount
    assert changed==1
'@
 $expirePath=Join-Path $private 'expire-copied-action.py'
 $expire|Set-Content -LiteralPath $expirePath -Encoding utf8NoBOM
 & python.exe $expirePath $db $action.id $b
 if($LASTEXITCODE-ne 0){throw 'Copied action lease injection failed.'}
 $recovered=$null;$newRunner=$null
 foreach($try in 1..300){
  $recovered=(Rpc 'actions/status' @{batch_id=$batch.id}).actions|Select-Object -First 1
  if($recovered.state-eq 'uncertain'){$newRunner=Owned-Runner;break};Start-Sleep -Milliseconds 200
 }
 if($recovered.state-ne 'uncertain' -or $recovered.attempt_count-ne 1 -or -not $newRunner -or $newRunner.ProcessId-eq $runnerProcess.ProcessId){throw 'Bounded service restart/uncertain no-replay recovery failed.'}
 if((Rpc 'tasks/get' @{id=$task.id}).state-ne 'completed'){throw 'Action crash reopened the task.'}
 $summary.crash=@{batch_id=$batch.id;task_id=$task.id;action_id=$action.id;state=$recovered.state;attempt_count=$recovered.attempt_count;job_child_killed=$true;old_runner_pid=$runnerProcess.ProcessId;new_runner_pid=$newRunner.ProcessId;forced_expiry='only exact copied database'}
 foreach($name in @($state.runner_service,$state.probe_service)){
  & $servy stop --name $name --quiet *> (Join-Path $private ($name+'.stop-after-probe.log'))
  if($LASTEXITCODE-ne 0){throw 'Owned test service failed to stop.'}
 }
 $summary.phase='isolated-prior-binary-rollback'
 $rollbackDb=Join-Path $b 'verification/rollback.db'
 & python.exe (Join-Path $state.backup_root 'sqlite-online-backup.py') --source $db --destination $rollbackDb *> (Join-Path $private 'rollback-copy.json')
 if($LASTEXITCODE-ne 0){throw 'Migration rollback copy failed.'}
 if(Get-NetTCPConnection -LocalPort 17778 -State Listen -ErrorAction SilentlyContinue){throw 'Rollback loopback port occupied.'}
 $oldExe=Join-Path $state.backup_root 'binary/coord.exe'
 if((Get-FileHash -LiteralPath $oldExe -Algorithm SHA256).Hash.ToLowerInvariant()-ne [string]$backupManifest.installed_binary_sha256){throw 'Prior binary restore hash does not match the verified backup manifest.'}
 $rollbackProcess=Start-Process -FilePath $oldExe -ArgumentList @('serve','--addr','127.0.0.1:17778','--db',$rollbackDb) -WorkingDirectory $b -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $private 'rollback.stdout.log') -RedirectStandardError (Join-Path $private 'rollback.stderr.log')
 $ready=$false
 foreach($try in 1..100){
  try{$response=Invoke-RestMethod -Uri 'http://127.0.0.1:17778/' -Method Post -ContentType 'application/json' -Body '{"jsonrpc":"2.0","id":1,"method":"tasks/list","params":{"skip_reclaim":true,"limit":1}}' -TimeoutSec 1;if($response.error){throw 'Prior binary RPC failed'};$ready=$true;break}catch{Start-Sleep -Milliseconds 100}
 }
 if(-not $ready){throw 'Prior binary cannot open migrated copied database.'}
 $compare=@'
import sqlite3,sys,json,hashlib
from urllib.parse import quote
def ro(path):return sqlite3.connect('file:'+quote(path.replace('\\','/'),safe='/:')+'?mode=ro',uri=True)
with ro(sys.argv[1]) as original,ro(sys.argv[2]) as restored:
    columns=[row[1] for row in original.execute('PRAGMA table_info(tasks)')]
    query='SELECT '+','.join('"'+c.replace('"','""')+'"' for c in columns)+' FROM tasks'
    rows=original.execute(query).fetchall()
    for row in rows:
        item=restored.execute(query+' WHERE id=?',(row[columns.index('id')],)).fetchone()
        assert item==row,'Original task fields changed in rollback copy'
    print(json.dumps({'original_tasks':len(rows),'original_columns':len(columns),'all_original_task_fields_match':True,'prior_binary_rpc':'PASS'}))
'@
 $comparePath=Join-Path $private 'compare-rollback.py'
 $compare|Set-Content -LiteralPath $comparePath -Encoding utf8NoBOM
 $comparison=& python.exe $comparePath (Join-Path $state.backup_root 'coord.db') $rollbackDb
 if($LASTEXITCODE-ne 0){throw 'Original task preservation failed in rollback copy.'}
 $summary.rollback=$comparison|ConvertFrom-Json
 $summary.state='PASS';$summary.phase='completed'
}catch{$summary.failure=@{phase=$summary.phase;message=$_.Exception.Message;exception_type=$_.Exception.GetType().FullName};throw}
finally{
 foreach($name in @($state.runner_service,$state.probe_service)){
  try{$service=Get-Service -Name $name -ErrorAction SilentlyContinue;if($service -and $service.Status-ne 'Stopped'){& $servy stop --name $name --quiet *> (Join-Path $private ($name+'.final-stop.log'));if($LASTEXITCODE-ne 0){throw 'Owned rehearsal service failed to stop.'}}}catch{$summary.final_service_cleanup='FAIL'}
 }
 if($rollbackProcess-and -not $rollbackProcess.HasExited){Stop-Process -Id $rollbackProcess.Id -Force -Confirm:$false;[void]$rollbackProcess.WaitForExit(10000)}
 if(-not $summary.final_service_cleanup){$summary.final_service_cleanup='PASS'}
 Start-Sleep -Milliseconds 250
 if(Get-NetTCPConnection -State Listen -LocalPort 17777,17778 -ErrorAction SilentlyContinue){$summary.final_listener_cleanup='FAIL'}else{$summary.final_listener_cleanup='PASS'}
 $summary|ConvertTo-Json -Depth 15|Set-Content -LiteralPath (Join-Path $private 'site-recovery-result.json') -Encoding utf8NoBOM
 $summary|ConvertTo-Json -Depth 15
}
if($summary.state -ne 'PASS' -or $summary.final_service_cleanup -ne 'PASS' -or $summary.final_listener_cleanup -ne 'PASS'){throw 'Protected-copy recovery rehearsal or exact owned-service cleanup failed.'}

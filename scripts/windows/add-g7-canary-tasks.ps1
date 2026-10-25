$ErrorActionPreference='Stop'
$base='C:\ProgramData\coord\g7'
$b=Get-Content -Raw (Join-Path $base 'private/canary-batches.json')|ConvertFrom-Json
if($b.state -ne 'EMPTY_BATCHES_BOUND' -or $b.task_count -ne 0){throw 'Only empty, pre-bound canaries may be initialized.'}
$key=[IO.File]::ReadAllText((Join-Path $base 'daemon-private/management.key')).Trim()
function Rpc([string]$Method,$Params){
 $r=Invoke-RestMethod 'http://127.0.0.1:7777/' -Method Post -ContentType application/json -Headers @{Authorization=('Bearer '+$key)} -Body (@{jsonrpc='2.0';id=1;method=$Method;params=$Params}|ConvertTo-Json -Depth 30 -Compress)
 if($r.error){throw ('RPC failed: '+$Method)};return $r.result
}
$counts=@{}
foreach($kind in @('operator','browser')){
 $id=$b.($kind+'_batch_id');$count=if($kind -eq 'operator'){8}else{4}
 $status=Rpc 'batches/status' @{batch_id=$id};if($status.total -ne 0){throw 'Canary already contains tasks.'}
 $tasks=@(0..($count-1)|ForEach-Object{@{key=('g7-'+$kind+'-synthetic-'+$_);name='G7 synthetic single-pixel marker';payload=@{synthetic=$true;group_id=$_.ToString('00');source_image_ids=@(($_+1).ToString());reference_path='refs/arknights/icons/marker.png';instruction='Deployment acceptance fixture only. One-pixel marker, no game character; use uncertain with reason and reference check.'}}})
 Rpc 'batches/add_tasks' @{batch_id=$id;tasks=$tasks}|Out-Null
 $counts[$kind]=Rpc 'batches/status' @{batch_id=$id}
}
$b.state='SYNTHETIC_TASKS_READY';$b.task_count=12
$b|ConvertTo-Json -Depth 20|Set-Content (Join-Path $base 'private/canary-batches.json') -Encoding utf8NoBOM
@{state='PASS';batches=$counts;formal_tasks_created=0;classification_agents_started=0}|ConvertTo-Json -Depth 15

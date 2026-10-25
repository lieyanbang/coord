$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$base='C:\ProgramData\coord\g7'
$path=Join-Path $base 'relay-private/identity.json'
$client='2e12506a52763d439506792019e34debb54b912d2575c82c3ef275af5cd01ab1'
$batch='d2313c78-22da-4cb8-bcd7-ece9897e0db1'
$raw=Get-Content -Raw $path|ConvertFrom-Json
if($raw.version -ne 2 -or $raw.mode -ne 'logical' -or -not $raw.enabled){throw 'Expected enabled G7 logical policy.'}
if(@($raw.bindings|Where-Object id -eq 'g7-browser-reconnect').Count){throw 'Binding exists; inspect persisted state before repeating.'}
$original=@($raw.bindings|Where-Object{$_.id -eq 'g7-browser' -and $_.batch_id -eq $batch})
if($original.Count -ne 1 -or $original[0].expires_at -le [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()){throw 'Expected live browser binding.'}
$added=$original[0]|ConvertTo-Json -Depth 15|ConvertFrom-Json
$added.id='g7-browser-reconnect';$added.client_sha256=$client;$added.worker_ids=@('0000','0001')
$beforeHash=(Get-FileHash $path).Hash.ToLowerInvariant()
$backup=Join-Path $base ('private/reconnect-'+[DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ')+'-'+[guid]::NewGuid().ToString('N').Substring(0,8))
New-Item -ItemType Directory -Path $backup|Out-Null
Copy-Item $path (Join-Path $backup 'identity-before.json')
if((Get-FileHash (Join-Path $backup 'identity-before.json')).Hash.ToLowerInvariant() -ne $beforeHash){throw 'Policy backup mismatch.'}
$oldLegacy=$raw.legacy_client_sha256|ConvertTo-Json -Compress
$oldBindings=$raw.bindings|ConvertTo-Json -Depth 20 -Compress
$raw.bindings=@($raw.bindings)+@($added)
$temp=$path+'.reconnect-new'
if(Test-Path $temp){throw 'Candidate already exists; do not overwrite.'}
$raw|ConvertTo-Json -Depth 20|Set-Content $temp -Encoding utf8NoBOM
$service=Get-Content -Raw 'C:\ProgramData\coord\backup\g7-20261025T050716Z-eea8ecd6\services\CoordRelayAgent.json'|ConvertFrom-Json
$node=$service.ExecutablePath
if(-not(Test-Path $node)){throw 'Actual Relay Node executable missing.'}
$check=@'
const m=require('C:/ProgramData/coord-relay-agent/identity.cjs');
const fs=require('node:fs/promises');
(async()=>{
 const old=await m.loadPolicy('C:/ProgramData/coord/g7/relay-private/identity.json');
 const next=await m.loadPolicy('C:/ProgramData/coord/g7/relay-private/identity.json.reconnect-new');
 if(JSON.stringify(old.legacy_client_sha256)!==JSON.stringify(next.legacy_client_sha256)||JSON.stringify(old.bindings)!==JSON.stringify(next.bindings.slice(0,-1))||next.key!==old.key)throw Error('Existing policy/key changed');
 const b=next.bindings.at(-1);
 if(b.id!=='g7-browser-reconnect'||b.client_sha256!=='2e12506a52763d439506792019e34debb54b912d2575c82c3ef275af5cd01ab1'||b.batch_id!=='d2313c78-22da-4cb8-bcd7-ece9897e0db1'||JSON.stringify(b.worker_ids)!=='["0000","0001"]')throw Error('Unexpected new grant');
})().catch(()=>process.exit(1));
'@
& $node -e $check
if($LASTEXITCODE -ne 0){throw 'Candidate actual policy validation failed.'}
$before=@(Get-CimInstance Win32_Service|Where-Object Name -in @('coord','CoordActionRunnerG7','Servy')|Select-Object Name,ProcessId,State,StartName)
$summary=@{state='FAIL';backup_root=$backup;before_sha256=$beforeHash;added_binding=$added}
try{
 [IO.File]::Move($temp,$path,$true)
 Restart-Service CoordRelayAgent
 $s=Get-Service CoordRelayAgent;$s.WaitForStatus('Running',[timespan]::FromSeconds(25))
 $after=@(Get-CimInstance Win32_Service|Where-Object Name -in @('coord','CoordActionRunnerG7','Servy')|Select-Object Name,ProcessId,State,StartName)
 if(($before|ConvertTo-Json -Compress) -ne ($after|ConvertTo-Json -Compress)){throw 'Unrelated daemon/Runner/Servy changed.'}
 $summary.state='PASS';$summary.after_sha256=(Get-FileHash $path).Hash.ToLowerInvariant();$summary.legacy_allowlist_unchanged=($oldLegacy -eq ($raw.legacy_client_sha256|ConvertTo-Json -Compress));$summary.original_bindings_retained=$true;$summary.unrestarted_services=$after
}catch{
 Copy-Item (Join-Path $backup 'identity-before.json') $path -Force
 Restart-Service CoordRelayAgent
 $summary.rollback='prior policy restored and Relay restarted';$summary.failure=$_.Exception.Message
}finally{
 $summary|ConvertTo-Json -Depth 20|Set-Content (Join-Path $backup 'result.json') -Encoding utf8NoBOM
 $summary|ConvertTo-Json -Depth 20
}
if($summary.state -ne 'PASS'){throw 'Windows identity repair failed.'}

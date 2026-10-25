$ErrorActionPreference='Stop'
$base='C:\ProgramData\coord\g7'
$path=Join-Path $base 'private/canary-batches.json'
if(Test-Path $path){throw 'Canary registry already exists; inspect instead of recreating.'}
$key=[IO.File]::ReadAllText((Join-Path $base 'daemon-private/management.key')).Trim()
function Rpc([string]$Method,$Params){
 $r=Invoke-RestMethod 'http://127.0.0.1:7777/' -Method Post -ContentType application/json -Headers @{Authorization=('Bearer '+$key)} -Body (@{jsonrpc='2.0';id=1;method=$Method;params=$Params}|ConvertTo-Json -Depth 30 -Compress)
 if($r.error){throw ('RPC failed: '+$Method)};return $r.result
}
$deployment=Get-Content -Raw (Join-Path $base 'private/production-deployment.json')|ConvertFrom-Json
if($deployment.state -ne 'PASS'){throw 'Production gates required.'}
$operator=Rpc 'batches/create' @{name='G7 synthetic operator six-worker canary';config=@{action_profile='g7-canary-v1';require_progress=$true;agent_lease_budget_seconds=3600;max_attempts=3}}
$browser=Rpc 'batches/create' @{name='G7 real ChatGPT two-conversation canary - awaiting tool recovery';config=@{action_profile='g7-browser-v1';require_progress=$true;agent_lease_budget_seconds=3600;max_attempts=3}}
$now=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$policy=Get-Content -Raw (Join-Path $base 'relay-private/identity.json')|ConvertFrom-Json
$policy.bindings=@(
 @{id='g7-operator';batch_id=$operator.id;client_sha256=$policy.legacy_client_sha256[0];worker_ids=@('0000','0001','0002','0003','0004','0005');scopes=@('coord:read','coord:claim','coord:progress','coord:extend','coord:submit','coord:failure');not_before=($now-1000);expires_at=($now+86300000)},
 @{id='g7-browser';batch_id=$browser.id;client_sha256=$policy.legacy_client_sha256[0];worker_ids=@('0000','0001','0002','0003','0004','0005');scopes=@('coord:read','coord:claim','coord:progress','coord:extend','coord:submit','coord:failure');not_before=($now-1000);expires_at=($now+86300000)}
)
$policy|ConvertTo-Json -Depth 12|Set-Content (Join-Path $base 'relay-private/identity.json') -Encoding utf8NoBOM
Restart-Service CoordRelayAgent
$result=@{state='EMPTY_BATCHES_BOUND';operator_batch_id=$operator.id;browser_batch_id=$browser.id;bindings=$policy.bindings;task_count=0;signing_secret_emitted=$false}
$result|ConvertTo-Json -Depth 15|Set-Content $path -Encoding utf8NoBOM
$result|ConvertTo-Json -Depth 15

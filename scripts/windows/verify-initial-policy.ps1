$ErrorActionPreference='Stop'
$base='C:\ProgramData\coord\g7'
$state=Get-Content -Raw (Join-Path $base 'private/initial-operation.json')|ConvertFrom-Json
$node=(Get-Content -Raw (Join-Path $state.backup_root 'CoordRelayAgent.json')|ConvertFrom-Json).ExecutablePath
$check=@'
const assert=require('node:assert/strict');
const fs=require('node:fs/promises');
const {randomUUID}=require('node:crypto');
const identity=require('C:/ProgramData/coord-relay-agent/identity.cjs');
(async()=>{
 const policy=await identity.loadPolicy('C:/ProgramData/coord/g7/relay-private/identity.json');
 const binding=policy.bindings.find(b=>b.id==='emote-initial-v1');assert.ok(binding);
 const workers=[];
 for(const worker of binding.worker_ids){
  const now=Date.now();
  const principal={binding_id:binding.id,agent_id:`emote-${worker}`,worker_id:worker,batch_id:binding.batch_id,scopes:binding.scopes,issued_at:now,expires_at:Math.min(now+60000,binding.expires_at)};
  for(const method of ['scheduler/agent_status','scheduler/claim']){
   const params={batch_id:binding.batch_id,agent_id:principal.agent_id};if(method==='scheduler/claim')params.lease_seconds=600;
   const req=identity.signRequest({requestId:randomUUID(),method,params},principal,policy);
   assert.equal(identity.verifyRequest(req,policy).ok,true);
  }
  workers.push(worker);
 }
 console.log(JSON.stringify({state:'PASS',batch_id:binding.batch_id,binding,workers,windows_signature_scope_validation:'PASS',network_claim_calls:0}));
})().catch(()=>{console.error('Windows policy gate failed');process.exit(1)});
'@
& $node -e $check
if($LASTEXITCODE -ne 0){throw 'Windows policy check failed.'}

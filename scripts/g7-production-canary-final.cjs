// Read-only reconciliation after the interrupted operator fixture; never replay.
const assert=require('node:assert/strict');
const fs=require('node:fs/promises');
const {randomUUID}=require('node:crypto');
const identity=require('/opt/coord-relay/identity.cjs');
(async()=>{
 const p=await identity.loadPolicy('/etc/coord-relay/identity.json');
 const b=p.bindings.find(x=>x.id==='g7-operator');assert.ok(b);
 const c=JSON.parse(await fs.readFile('/etc/coord-relay/config.json','utf8'));
 const secret=(await fs.readFile(c.clientSecretFile,'utf8')).trim();
 const make=()=>{const now=Date.now();return identity.signRequest({requestId:randomUUID(),method:'scheduler/agent_status',params:{agent_id:'emote-0000',batch_id:b.batch_id}}, {binding_id:b.id,agent_id:'emote-0000',worker_id:'0000',batch_id:b.batch_id,scopes:b.scopes,issued_at:now,expires_at:Math.min(now+60000,b.expires_at)},p)};
 const send=async(req,key=secret)=>{const r=await fetch('http://127.0.0.1:'+c.clientPort+'/v1/rpc',{method:'POST',headers:{'content-type':'application/json',authorization:'Bearer '+key},body:JSON.stringify(req)});return{status:r.status,body:await r.json()}};
 const good=await send(make());assert.equal(good.body.state,'ok');
 const progress=good.body.result.batch_progress;
 assert.equal(progress.total,10);assert.equal(progress.states.completed,10);assert.equal(progress.active,0);
 assert.doesNotMatch(JSON.stringify(good.body),/lease_token|leaseToken/);
 const tamper=make();tamper.identity.worker_id='0001';
 const bad=await send(tamper);assert.equal(bad.status,400);assert.equal(bad.body.state,'error');
 const wrongKey=await send(make(),'wrong');assert.equal(wrongKey.status,401);
 const unsigned=make();delete unsigned.identity;unsigned.params.agent_id='coord-relay-agent';
 const denied=await send(unsigned);assert.equal(denied.body.state,'not_dispatched');
 const journal=await fs.readFile(c.journalFile,'utf8');
 assert.doesNotMatch(journal,/lease_token|leaseToken|"params"|"checkpoint"|"characters"/);
 const keys=[...new Set(journal.split('\n').filter(Boolean).flatMap(s=>Object.keys(JSON.parse(s))))].sort();
 console.log(JSON.stringify({state:'PASS',scope:'read-only production reconciliation; prior lifecycle trace retained separately',batch_id:b.batch_id,progress,tampered_identity:{status:bad.status,state:bad.body.state},wrong_transport_key:{status:wrongKey.status},unsigned_protected_batch:{state:denied.body.state},journal:{no_task_or_lease_fields:true,record_keys:keys},real_chatgpt_lifecycle:'NEEDS_USER_ACTION'},null,2));
})().catch(e=>{console.error(JSON.stringify({state:'FAIL',failure:e.message}));process.exitCode=1});

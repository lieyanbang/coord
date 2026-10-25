"""Read-only six-worker end-to-end queries; claim dispatch is intercepted."""
import json
import subprocess

code = r'''
const assert=require('node:assert/strict');
const fs=require('node:fs/promises');
const {randomUUID}=require('node:crypto');
const identity=require('/opt/coord-relay/identity.cjs');
const mcp=require('/opt/app/feathers/realtimetext/lib/coord-mcp/index.cjs');
let phase='policy';
(async()=>{
 const policy=await identity.loadPolicy('/etc/coord-relay/identity.json');
 const binding=policy.bindings.find(b=>b.id==='emote-initial-v1');assert.ok(binding);
 const store=JSON.parse(await fs.readFile('/var/lib/coord-mcp/oauth.json','utf8'));
 const clientId=Object.keys(store.clients).find(c=>identity.hash(c)===binding.client_sha256);assert.ok(clientId);
 phase='live-oauth-metadata';
 const live=Object.entries(store.accessTokens).find(([h,v])=>v.clientId===clientId && v.expiresAt>Date.now());assert.ok(live,'Unexpired approved OAuth record required');
 const auth={clientId,expiresAt:live[1].expiresAt,tokenSha256:live[0]};
 const config=JSON.parse(await fs.readFile('/etc/coord-relay/config.json','utf8'));
 const secret=(await fs.readFile(config.clientSecretFile,'utf8')).trim();
 let claimDispatches=0, interceptedClaims=0;
 const sidecar={call:async(req)=>{
  const valid=identity.verifyRequest(req,policy);assert.equal(valid.ok,true);
  if(req.method==='scheduler/claim'){
   // Never send a claim to the live Sidecar/queue, including to an empty batch.
   interceptedClaims++;assert.equal(req.params.lease_seconds,600);return {authorization_only:true,not_dispatched:true};
  }
  assert.equal(req.method,'scheduler/agent_status');
  const res=await fetch(`http://127.0.0.1:${config.clientPort}/v1/rpc`,{method:'POST',headers:{'content-type':'application/json',authorization:`Bearer ${secret}`},body:JSON.stringify(req),signal:AbortSignal.timeout(15000)});
  assert.equal(res.status,200);const body=await res.json();assert.equal(body.state,'ok');return body.result;
 }};
 class Server{constructor(){this.handlers=new Map()}setRequestHandler(type,handler){this.handlers.set(type,handler)}}
 const types={InitializeRequestSchema:'init',ListToolsRequestSchema:'list',CallToolRequestSchema:'call',SUPPORTED_PROTOCOL_VERSIONS:['2025-03-26'],LATEST_PROTOCOL_VERSION:'2025-03-26'};
 const server=await mcp.createCoordMcpServer(auth,sidecar,async()=>({server:{Server},types}),policy);
 const workers=[];
 for(let i=0;i<6;i++){
  const worker=i.toString(16).padStart(4,'0').toUpperCase();
  phase='query-'+worker;
  const query=await server.handlers.get('call')({params:{name:'coord_agent_status',arguments:{worker_id:worker,batch_id:binding.batch_id}}});
  assert.ok(!query.isError);const result=JSON.parse(query.content[0].text);
  assert.equal(result.batch_progress.total,121);assert.equal(result.batch_progress.states.pending,121);assert.equal(result.batch_progress.states.claimed,0);assert.equal(result.batch_progress.states.submitted,0);
  phase='intercepted-claim-'+worker;
  const claim=await server.handlers.get('call')({params:{name:'coord_claim_next',arguments:{worker_id:worker,batch_id:binding.batch_id,request_id:randomUUID(),lease_seconds:600}}});
  assert.ok(!claim.isError);assert.equal(JSON.parse(claim.content[0].text).not_dispatched,true);
  workers.push({worker_id:worker,real_two_hop_status:'PASS',mcp_claim_authorization_only:'PASS',claim_dispatched:false});
 }
 assert.equal(interceptedClaims,6);assert.equal(claimDispatches,0);
 phase='negative-checks';
 assert.throws(()=>identity.resolvePrincipal(policy,auth,Date.now(),{worker_id:'0006',batch_id:binding.batch_id}));
 assert.throws(()=>identity.resolvePrincipal(policy,auth,Date.now(),{worker_id:'0000',batch_id:randomUUID()}));
 assert.throws(()=>identity.resolvePrincipal(policy,{...auth,clientId:'not-authorized'},Date.now(),{worker_id:'0000',batch_id:binding.batch_id}));
 assert.throws(()=>identity.resolvePrincipal(policy,{...auth,expiresAt:binding.expires_at+60000},binding.expires_at,{worker_id:'0000',batch_id:binding.batch_id}));
 console.log(JSON.stringify({state:'PASS',batch_id:binding.batch_id,binding,workers,negative_checks:['unlisted worker','wrong batch','unlisted client','exact policy expiry'],claim_rpc_invocations:0,claim_dispatches:0,oauth_context:'existing unexpired registered client metadata; operator verifier, not a ChatGPT conversation',tokens_exported:false}));
})().catch(e=>{console.error('Read-only six-worker gate failed; phase='+phase+'; type='+e.name+'; code='+(e.code||'')+'; no claims dispatched');process.exit(1)});
'''
r = subprocess.run(['node','-e',code],capture_output=True,text=True)
if r.returncode:
    raise RuntimeError(r.stderr.strip()) from None
print(json.dumps(json.loads(r.stdout)))

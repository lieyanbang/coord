// Operator synthetic acceptance through the deployed Sidecar and Windows Relay.
// This does not impersonate an OAuth exchange or real ChatGPT conversation.
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const { randomUUID } = require('node:crypto');
const identity = require('/opt/coord-relay/identity.cjs');
const mcp = require('/opt/app/feathers/realtimetext/lib/coord-mcp/index.cjs');
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));

(async () => {
  const policy = await identity.loadPolicy('/etc/coord-relay/identity.json');
  const binding = policy.bindings.find(b => b.id === 'g7-operator');
  assert.ok(binding);
  const config = JSON.parse(await fs.readFile('/etc/coord-relay/config.json', 'utf8'));
  const secret = (await fs.readFile(config.clientSecretFile, 'utf8')).trim();
  const endpoint = `http://127.0.0.1:${config.clientPort}/v1/rpc`;
  const send = async (request, credential = secret) => {
    const r = await fetch(endpoint, { method: 'POST', headers: { 'content-type': 'application/json', authorization: `Bearer ${credential}` }, body: JSON.stringify(request), signal: AbortSignal.timeout(15000) });
    return { status: r.status, body: await r.json(), request };
  };
  const call = async (name, worker, args = {}) => {
    const c = mcp.validateToolInput(name, { worker_id: worker, batch_id: binding.batch_id, ...args });
    const now = Date.now();
    const p = { binding_id: binding.id, agent_id: `emote-${worker}`, worker_id: worker, batch_id: binding.batch_id, scopes: binding.scopes, issued_at: now, expires_at: Math.min(now + 60000, binding.expires_at) };
    if (Object.hasOwn(c.params, 'agent_id')) c.params.agent_id = p.agent_id;
    if (Object.hasOwn(c.params, 'agentId')) c.params.agentId = p.agent_id;
    return send(identity.signRequest({ requestId: c.requestId, method: c.method, params: c.params }, p, policy));
  };
  const ok = r => { assert.equal(r.body.state, 'ok', `Expected success, got ${r.body.state}/${r.body.code || r.body.error?.code || ''}`); return r.body.result; };
  const resultFor = (t, worker) => ({ schema_version: 1, group_id: t.payload.group_id, agent_id: `emote-${worker}`, attempt_no: t.attempt_no, source_image_ids: t.payload.source_image_ids, characters: [],
    uncertain: t.payload.source_image_ids.map(image_id => ({ image_id, visible_features: 'Synthetic single-pixel marker; no game character is depicted', reference_candidates: [], checked_paths: ['refs/arknights/icons/marker.png'], check_conclusion: 'Inspected synthetic reference; no character classification', reason: 'G7 deployment acceptance fixture', review_priority: 'low' })), general_notes: 'Operator synthetic fixture; not a real ChatGPT visual acceptance' });
  const submit = async (t, worker) => {
    const r = await call('coord_submit_result', worker, { id: t.id, lease_token: t.lease_token, request_id: randomUUID(), result: resultFor(t, worker) });
    ok(r); return r;
  };
  const report = { state: 'FAIL', environment: 'actual production daemon/Sidecar/Windows Relay/dedicated service Runner', client_kind: 'operator synthetic; no OAuth/ChatGPT claim', batch_id: binding.batch_id, started_at: new Date().toISOString(), checks: [] };
  const claims = [];
  try {
    const initial = ok(await call('coord_agent_status', '0000')).batch_progress;
    assert.equal(initial.active, 8, 'Run only with exactly eight outstanding synthetic fixtures');
    assert.equal(initial.states.claimed, 0, 'Do not race another canary operator');
    report.initial_progress = initial;
    for (const wave of [[0, 1, 2], [3, 4, 5]]) {
      claims.push(...await Promise.all(wave.map(i => call('coord_claim_next', `000${i}`, { request_id: randomUUID(), lease_seconds: 60 }))));
    }
    const tasks = claims.map(c => ok(c).task);
    assert.ok(tasks.every(Boolean));assert.equal(new Set(tasks.map(t => t.id)).size, 6);
    assert.deepEqual(tasks.map(t => t.claimed_by), ['emote-0000', 'emote-0001', 'emote-0002', 'emote-0003', 'emote-0004', 'emote-0005']);
    report.claims = tasks.map(t => ({ id: t.id, worker: t.claimed_by, attempt_no: t.attempt_no }));
    report.checks.push('six unique concurrent claims');
    const first = tasks[0];
    assert.equal((await call('coord_report_progress', '0001', { id: first.id, lease_token: first.lease_token, attempt_no: first.attempt_no, sequence: 1, checkpoint: 'foreign worker negative', request_id: randomUUID() })).body.state, 'rpc_error');
    assert.equal(ok(await call('coord_get_task', '0001', { id: first.id })).lease_token, undefined);
    report.checks.push('foreign live token denied; reads redact tokens');
    const seq = { id: first.id, lease_token: first.lease_token, attempt_no: first.attempt_no, request_id: randomUUID(), sequence: 1, checkpoint: 'Synthetic marker and synthetic reference inspected' };
    ok(await call('coord_report_progress', '0000', seq));
    const ext = { id: first.id, lease_token: first.lease_token, attempt_no: first.attempt_no, request_id: randomUUID(), progress_sequence: 1, lease_seconds: 60 };
    ok(await call('coord_extend_lease_progress', '0000', ext));
    assert.equal((await call('coord_extend_lease_progress', '0000', { ...ext, request_id: randomUUID() })).body.state, 'rpc_error');
    report.checks.push('progress renewal; consumed sequence denied');
    const done = await submit(first, '0000');
    assert.equal((await send(done.request)).body.state, 'uncertain');
    for (let i = 1; i < tasks.length; i++) await submit(tasks[i], `000${i}`);
    report.checks.push('six fenced submissions; request replay uncertain');
    const expiring = ok(await call('coord_claim_next', '0000', { request_id: randomUUID(), lease_seconds: 1 })).task;
    const failing = ok(await call('coord_claim_next', '0001', { request_id: randomUUID(), lease_seconds: 60 })).task;
    assert.ok(expiring && failing && expiring.id !== failing.id);
    ok(await call('coord_report_failure', '0001', { id: failing.id, lease_token: failing.lease_token, request_id: randomUUID(), error: 'Intentional bounded synthetic failure' }));
    await delay(1250);
    const recoveries = [];
    for (let i = 0; i < 50 && recoveries.length < 2; i++) {
      const t = ok(await call('coord_claim_next', '0004', { request_id: randomUUID(), lease_seconds: 60 })).task;
      if (!t) { await delay(200); continue; }
      assert.ok([expiring.id, failing.id].includes(t.id));assert.equal(t.attempt_no, 2);
      const stale = t.id === expiring.id ? expiring : failing;
      const oldWorker = t.id === expiring.id ? '0000' : '0001';
      assert.equal((await call('coord_submit_result', oldWorker, { id: stale.id, lease_token: stale.lease_token, request_id: randomUUID(), result: resultFor(stale, oldWorker) })).body.state, 'rpc_error');
      await submit(t, '0004');recoveries.push({ id: t.id, attempt_no: t.attempt_no, worker: t.claimed_by });
    }
    assert.equal(recoveries.length, 2);report.recoveries = recoveries;
    report.checks.push('next claim; natural expiry and failure reclaimed at attempt 2; stale submissions denied');
    const unsigned = { requestId: randomUUID(), method: 'scheduler/claim', params: { agent_id: 'coord-relay-agent', batch_id: binding.batch_id } };
    assert.equal((await send(unsigned)).body.state, 'not_dispatched');
    assert.notEqual((await send(unsigned, 'wrong')).status, 200);
    const tamper = structuredClone(done.request);tamper.requestId=randomUUID();tamper.identity.worker_id='0001';
    assert.equal((await send(tamper)).body.state, 'error');
    report.checks.push('unsigned protected batch, bad transport key, tampered identity denied');
    const status = ok(await call('coord_agent_status', '0000'));
    assert.equal(status.batch_progress.total, initial.total);assert.equal(status.batch_progress.terminal, initial.total);assert.equal(status.batch_progress.states.completed, initial.total);assert.equal(status.batch_progress.active, 0);
    assert.doesNotMatch(JSON.stringify(status), /lease_token/);report.progress=status.batch_progress;
    const journal = await fs.readFile(config.journalFile, 'utf8');
    for (const t of [...tasks, expiring, failing]) assert.equal(journal.includes(t.lease_token), false);
    report.checks.push('authoritative terminal snapshot; journal excludes lease tokens');
    report.state = 'PASS';
  } catch (error) { report.failure = error.message; process.exitCode = 1; }
  report.finished_at=new Date().toISOString();
  console.log(JSON.stringify(report, null, 2));
})().catch(error => { console.error(JSON.stringify({state:'FAIL',failure:error.message}));process.exitCode=1; });

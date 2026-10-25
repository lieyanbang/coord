import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { createSidecar } from './server.mjs';
import { createAgent } from './agent.mjs';
import { reservePort } from './test-ports.mjs';
import identity from './identity.cjs';
import mcp from './coord-mcp/index.cjs';
import { validateRpcRequest } from './protocol.mjs';

const batch = '123e4567-e89b-42d3-a456-426614174002';
const task = '123e4567-e89b-42d3-a456-426614174003';
const now = Date.now();
const auth = { clientId: 'synthetic-shared-owner', tokenSha256: identity.hash('synthetic-token'), expiresAt: now + 300_000 };

export function logicalPolicy(batchId = batch, time = Date.now()) {
  return identity.validatePolicy({ version: 2, mode: 'logical', enabled: true,
    signing_key_file: fileURLToPath(new URL('./synthetic-key', import.meta.url)),
    legacy_client_sha256: [identity.hash(auth.clientId)],
    bindings: [{ id: 'logical-canary', client_sha256: identity.hash(auth.clientId),
      worker_ids: ['0000', '0001', '0002', '0003', '0004', '0005', 'ABCD'], batch_id: batchId,
      scopes: ['coord:read', 'coord:claim', 'coord:progress', 'coord:extend', 'coord:submit', 'coord:failure'],
      not_before: time - 1_000, expires_at: time + 600_000 }],
  }, 'ac'.repeat(32));
}

test('logical worker input is explicit, canonical, batch-scoped and legacy-compatible', () => {
  const call = mcp.validateToolInput('coord_claim_next', { worker_id: 'aBcD', batch_id: batch, request_id: randomUUID() });
  assert.deepEqual(call.logical, { worker_id: 'ABCD', batch_id: batch });
  assert.equal(call.params.agent_id, 'coord-relay-agent', 'principal must be assigned only after OAuth authorization');
  for (const worker_id of ['', '000', '00000', 'GGGG', '../0', 0]) {
    assert.throws(() => mcp.validateToolInput('coord_claim_next', { worker_id, batch_id: batch, request_id: randomUUID() }));
  }
  assert.throws(() => mcp.validateToolInput('coord_get_task', { worker_id: '0000', id: task }));
  assert.throws(() => mcp.validateToolInput('coord_get_task', { worker_id: '0000', batch_id: batch, id: task, agent_id: 'emote-0001' }));
  assert.equal(mcp.validateToolInput('coord_claim_next', { batch_id: batch, request_id: randomUUID() }).logical, undefined);
  assert.match(mcp.TOOLS.find(t => t.name === 'coord_claim_next').inputSchema.properties.worker_id.pattern, /0-9/);
});

test('one authenticated owner can declare different logical workers and rotate tokens', () => {
  const policy = logicalPolicy(batch, now);
  const principals = Array.from({ length: 6 }, (_, i) => identity.resolvePrincipal(policy, auth, now, { worker_id: `000${i}`, batch_id: batch }));
  assert.deepEqual(principals.map(p => p.agent_id), Array.from({ length: 6 }, (_, i) => `emote-000${i}`));
  assert.equal(identity.resolvePrincipal(policy, { ...auth, tokenSha256: identity.hash('rotated') }, now, { worker_id: '0000', batch_id: batch }).agent_id, 'emote-0000');
  assert.throws(() => identity.resolvePrincipal(policy, null, now, { worker_id: '0000', batch_id: batch }));
  assert.throws(() => identity.resolvePrincipal(policy, { ...auth, clientId: 'other-owner-client' }, now, { worker_id: '0000', batch_id: batch }));
  assert.throws(() => identity.resolvePrincipal(policy, { ...auth, expiresAt: now }, now, { worker_id: '0000', batch_id: batch }));
  for (const declaration of [{ worker_id: '0006', batch_id: batch }, { worker_id: '0000', batch_id: randomUUID() }]) {
    assert.throws(() => identity.resolvePrincipal(policy, auth, now, declaration));
  }
  assert.throws(() => identity.resolvePrincipal(null, auth, now, { worker_id: '0000', batch_id: batch }));
});

test('logical worker, agent, batch, scope and expiration are signed and reverified at both hops', () => {
  const policy = logicalPolicy(batch, now);
  const principal = identity.resolvePrincipal(policy, auth, now, { worker_id: '0000', batch_id: batch });
  const request = identity.signRequest({ requestId: randomUUID(), method: 'scheduler/claim', params: { agent_id: principal.agent_id, batch_id: batch } }, principal, policy);
  assert.equal(validateRpcRequest(request, policy).ok, true);
  for (const mutate of [r => { r.identity.worker_id = '0001'; }, r => { r.params.agent_id = 'emote-0001'; }, r => { r.params.batch_id = randomUUID(); }, r => { r.requestId = randomUUID(); }]) {
    const changed = structuredClone(request); mutate(changed);
    assert.equal(validateRpcRequest(changed, policy).ok, false);
  }
  assert.equal(identity.verifyRequest(request, policy, now + 60_001).ok, false);
  const denied = { ...principal, scopes: ['coord:read'] };
  assert.equal(identity.verifyRequest(identity.signRequest({ ...request, identity: undefined }, denied, policy), policy, now).ok, false);
  assert.equal(identity.protectedBatch(policy, batch), true);
});

test('six synthetic logical clients run fenced lifecycles through both relay hops and a real daemon', { timeout: 30_000 }, async () => {
  const folder = await mkdtemp(join(tmpdir(), 'coord-g7-logical-'));
  const port = await reservePort();
  const coordUrl = `http://127.0.0.1:${port}/`;
  const executable = resolve(process.env.COORD_TEST_BIN || 'target/release/coord.exe');
  const daemon = spawn(executable, ['serve', '--addr', `127.0.0.1:${port}`, '--db', join(folder, 'state.db')], { stdio: 'ignore' });
  let sidecar; let agent;
  const direct = async (method, params) => {
    const result = await (await fetch(coordUrl, { method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', id: randomUUID(), method, params }) })).json();
    assert.equal(result.error, undefined);
    return result.result;
  };
  try {
    let ready = false;
    for (let i = 0; i < 100; i++) {
      if (daemon.exitCode !== null) throw new Error('isolated G7 daemon exited');
      try { await direct('tasks/list', { limit: 1, skip_reclaim: true }); ready = true; break; } catch { await delay(50); }
    }
    assert.equal(ready, true);
    const b = await direct('batches/create', { name: 'G7 synthetic logical lifecycle', config: { require_progress: true, max_attempts: 3, agent_lease_budget_seconds: 3600 } });
    await direct('batches/add_tasks', { batch_id: b.id, tasks: Array.from({ length: 8 }, (_, i) => ({ key: `synthetic-${i}`, name: 'synthetic fixture', payload: { synthetic: true } })) });
    const policy = logicalPolicy(b.id);
    const agentPort = await reservePort(); const clientPort = await reservePort();
    sidecar = await createSidecar({ agentHost: '127.0.0.1', agentPort, clientHost: '127.0.0.1', clientPort,
      allowedPeerIp: '127.0.0.1', agentSecret: '35'.repeat(32), clientSecret: '46'.repeat(32),
      identityPolicy: policy, journalFile: join(folder, 'requests.ndjson'), requestTimeoutMs: 4000 });
    await sidecar.start();
    agent = await createAgent({ serverHost: '127.0.0.1', serverPort: agentPort, coordUrl,
      secret: '35'.repeat(32), identityPolicy: policy, heartbeatMs: 100 });
    agent.start();
    assert.equal(await agent.waitUntilConnected(3000), true);
    const send = async (request, secret = '46'.repeat(32)) => {
      const response = await fetch(`http://127.0.0.1:${clientPort}/v1/rpc`, { method: 'POST',
        headers: { 'content-type': 'application/json', authorization: `Bearer ${secret}` }, body: JSON.stringify(request) });
      return { status: response.status, body: await response.json(), request };
    };
    const call = async (name, worker, args = {}) => {
      const c = mcp.validateToolInput(name, { worker_id: worker, batch_id: b.id, ...args });
      const p = identity.resolvePrincipal(policy, auth, Date.now(), c.logical);
      if (Object.hasOwn(c.params, 'agent_id')) c.params.agent_id = p.agent_id;
      if (Object.hasOwn(c.params, 'agentId')) c.params.agentId = p.agent_id;
      return send(identity.signRequest({ requestId: c.requestId, method: c.method, params: c.params }, p, policy));
    };
    const claims = [];
    // The transport admits at most four pending calls. Two waves retain real
    // concurrent database claims without treating backpressure as a lost task.
    for (const wave of [[0, 1, 2], [3, 4, 5]]) {
      claims.push(...await Promise.all(wave.map(i => call('coord_claim_next', `000${i}`, { request_id: randomUUID(), lease_seconds: 30 }))));
    }
    assert.ok(claims.every(c => c.body.state === 'ok' && c.body.result.task));
    const tasks = claims.map(c => c.body.result.task);
    assert.equal(new Set(tasks.map(t => t.id)).size, 6);
    assert.deepEqual(tasks.map(t => t.claimed_by), Array.from({ length: 6 }, (_, i) => `emote-000${i}`));
    const first = tasks[0];
    const foreign = await call('coord_report_progress', '0001', { id: first.id, lease_token: first.lease_token, attempt_no: first.attempt_no, sequence: 1, checkpoint: 'wrong-worker test', request_id: randomUUID() });
    assert.equal(foreign.body.state, 'rpc_error');
    const read = await call('coord_get_task', '0001', { id: first.id });
    assert.equal(read.body.state, 'ok'); assert.equal(read.body.result.lease_token, undefined);
    const status = await call('coord_agent_status', '0000');
    assert.equal(status.body.state, 'ok'); assert.doesNotMatch(JSON.stringify(status.body), /lease_token/);
    assert.equal(status.body.result.batch_progress.total, 8);
    assert.equal(status.body.result.batch_progress.status, 'active');
    const progress = await call('coord_report_progress', '0000', { id: first.id, lease_token: first.lease_token, attempt_no: first.attempt_no, sequence: 1, checkpoint: 'Synthetic work completed', request_id: randomUUID() });
    assert.equal(progress.body.state, 'ok');
    const extension = { id: first.id, lease_token: first.lease_token, attempt_no: first.attempt_no, progress_sequence: 1, lease_seconds: 30, request_id: randomUUID() };
    assert.equal((await call('coord_extend_lease_progress', '0000', extension)).body.state, 'ok');
    assert.equal((await call('coord_extend_lease_progress', '0000', { ...extension, request_id: randomUUID() })).body.state, 'rpc_error');
    const submitted = await call('coord_submit_result', '0000', { id: first.id, lease_token: first.lease_token, result: { synthetic: true }, request_id: randomUUID() });
    assert.equal(submitted.body.state, 'ok');
    assert.equal((await send(submitted.request)).body.state, 'uncertain');
    const next = (await call('coord_claim_next', '0000', { request_id: randomUUID(), lease_seconds: 1 })).body.result.task;
    assert.notEqual(next.id, first.id);
    for (let i = 1; i < tasks.length; i++) {
      const task = tasks[i];
      const name = i === 1 ? 'coord_report_failure' : 'coord_submit_result';
      const value = i === 1 ? { error: 'Synthetic bounded failure' } : { result: { synthetic: true } };
      assert.equal((await call(name, `000${i}`, { id: task.id, lease_token: task.lease_token, request_id: randomUUID(), ...value })).body.state, 'ok');
    }
    await delay(1150);
    await direct('tasks/list', { skip_reclaim: false, limit: 100 });
    const pending = await direct('tasks/get', { id: next.id });
    assert.equal(pending.state, 'pending');
    await delay(Math.max(0, Date.parse(pending.available_at) - Date.now() + 30));
    const recovered = await call('coord_claim_task', '0004', { id: next.id, request_id: randomUUID(), lease_seconds: 30 });
    assert.equal(recovered.body.state, 'ok');
    assert.ok(recovered.body.result.attempt_no > next.attempt_no);
    assert.equal((await call('coord_submit_result', '0000', { id: next.id, lease_token: next.lease_token, result: { stale: true }, request_id: randomUUID() })).body.state, 'rpc_error');
    const unsigned = { requestId: randomUUID(), method: 'scheduler/claim', params: { agent_id: 'coord-relay-agent', batch_id: b.id } };
    assert.equal((await send(unsigned)).body.state, 'not_dispatched');
    assert.notEqual((await send(unsigned, 'wrong')).status, 200);
    const journal = await readFile(join(folder, 'requests.ndjson'), 'utf8');
    for (const task of tasks) assert.equal(journal.includes(task.lease_token), false);
  } finally {
    await agent?.stop(); await sidecar?.stop();
    if (daemon.exitCode === null) { daemon.kill(); await new Promise(done => daemon.once('exit', done)); }
    await rm(folder, { recursive: true, force: true });
  }
});

import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createServer as createHttpServer } from 'node:http';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { randomUUID } from 'node:crypto';
import { setTimeout as delay } from 'node:timers/promises';
import { test } from 'node:test';
import { createAgent } from './agent.mjs';
import { createSidecar } from './server.mjs';
import { reservePort } from './test-ports.mjs';
import { COORD_AGENT_ID, RELAY_AGENT_ID } from './protocol.mjs';
import identity from './identity.cjs';
import identityFixture from './identity-test-fixture.cjs';

const agentSecret = '55'.repeat(32);
const clientSecret = '66'.repeat(32);
const coordExe = process.env.COORD_TEST_BIN
  ? resolve(process.env.COORD_TEST_BIN)
  : resolve('target/release/coord.exe');

async function directRpc(url, method, params) {
  const response = await fetch(url, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ jsonrpc: '2.0', id: randomUUID(), method, params }),
  });
  const envelope = await response.json();
  if (envelope.error) throw new Error(`coord RPC ${method} returned ${envelope.error.code}: ${envelope.error.message}`);
  return envelope.result;
}

async function relayRpc(url, requestId, method, params) {
  const response = await fetch(`${url}/v1/rpc`, {
    method: 'POST',
    headers: { authorization: `Bearer ${clientSecret}`, 'content-type': 'application/json' },
    body: JSON.stringify({ requestId, method, params }),
  });
  return { status: response.status, body: await response.json() };
}

test('isolated coord lease fencing and submit replay across the authenticated relay', async t => {
  assert.ok(coordExe.endsWith('.exe') || process.platform !== 'win32', 'COORD_TEST_BIN must point to the release coord executable');
  const folder = await mkdtemp(join(tmpdir(), 'coord-relay-live-'));
  const coordPort = await reservePort();
  const agentServer = createHttpServer();
  const agentPort = await reservePort();
  const clientServer = createHttpServer();
  const clientPort = await reservePort();
  await Promise.all([
    new Promise(resolveListen => agentServer.listen(agentPort, '127.0.0.1', resolveListen)),
    new Promise(resolveListen => clientServer.listen(clientPort, '127.0.0.1', resolveListen)),
  ]);
  await Promise.all([
    new Promise(resolveClose => agentServer.close(resolveClose)),
    new Promise(resolveClose => clientServer.close(resolveClose)),
  ]);

  const coord = spawn(coordExe, ['serve', '--addr', `127.0.0.1:${coordPort}`, '--db', join(folder, 'coord.db')], { stdio: 'ignore' });
  const coordUrl = `http://127.0.0.1:${coordPort}/`;
  let sidecar;
  let agent;
  let succeeded = false;
  try {
    const deadline = Date.now() + 10_000;
    while (Date.now() < deadline) {
      if (coord.exitCode !== null) throw new Error(`isolated coord exited with ${coord.exitCode}`);
      try {
        await directRpc(coordUrl, 'tasks/list', { limit: 1, skip_reclaim: true });
        succeeded = true;
        break;
      } catch {
        await delay(100);
      }
    }
    assert.equal(succeeded, true, 'isolated coord did not become ready');

    sidecar = await createSidecar({
      agentHost: '127.0.0.1', agentPort,
      clientHost: '127.0.0.1', clientPort,
      allowedPeerIp: '127.0.0.1', agentId: RELAY_AGENT_ID,
      agentSecret, clientSecret,
      journalFile: join(folder, 'relay', 'requests.ndjson'),
      requestTimeoutMs: 4_000, heartbeatTimeoutMs: 2_000,
    });
    await sidecar.start();
    agent = await createAgent({
      serverHost: '127.0.0.1', serverPort: agentPort,
      agentId: RELAY_AGENT_ID, coordUrl, secret: agentSecret,
      heartbeatMs: 50, reconnectMinMs: 50, reconnectMaxMs: 100, coordTimeoutMs: 3_000,
    });
    agent.start();
    assert.equal(await agent.waitUntilConnected(2_000), true);
    const relayUrl = `http://127.0.0.1:${clientPort}`;

    const staleTask = await directRpc(coordUrl, 'tasks/send', { name: 'F2 stale lease fixture', kind: 'task', priority: 'normal', payload: { fixture: true } });
    const staleClaimId = randomUUID();
    const firstClaim = await relayRpc(relayUrl, staleClaimId, 'tasks/claim', { id: staleTask.id, agentId: COORD_AGENT_ID, leaseSeconds: 1 });
    assert.equal(firstClaim.status, 200);
    const oldToken = firstClaim.body.result.lease_token;
    assert.equal(typeof oldToken, 'string');
    await delay(1_150);
    await directRpc(coordUrl, 'tasks/list', { state: 'pending', limit: 100, skip_reclaim: false });
    const reclaimed = await directRpc(coordUrl, 'tasks/get', { id: staleTask.id });
    assert.equal(reclaimed.state, 'pending');
    if (reclaimed.available_at) {
      await delay(Math.max(0, Date.parse(reclaimed.available_at) - Date.now()) + 25);
    }
    const newClaim = await directRpc(coordUrl, 'tasks/claim', { id: staleTask.id, agentId: 'f2-test-new', leaseSeconds: 600 });
    const staleSubmitId = randomUUID();
    const staleSubmit = await relayRpc(relayUrl, staleSubmitId, 'tasks/complete', {
      id: staleTask.id, agentId: COORD_AGENT_ID, leaseToken: oldToken,
      result: { should_not_complete: true },
    });
    assert.equal(staleSubmit.status, 409);
    assert.equal(staleSubmit.body.state, 'rpc_error');
    assert.equal(staleSubmit.body.error.code, -32001);
    const staleReplay = await relayRpc(relayUrl, staleSubmitId, 'tasks/complete', {
      id: staleTask.id, agentId: COORD_AGENT_ID, leaseToken: oldToken,
      result: { should_not_complete: true },
    });
    assert.equal(staleReplay.status, 504);
    assert.equal(staleReplay.body.state, 'uncertain');
    const afterStale = await directRpc(coordUrl, 'tasks/get', { id: staleTask.id });
    assert.equal(afterStale.state, 'claimed');
    assert.equal(afterStale.claimed_by, 'f2-test-new');
    assert.equal(afterStale.lease_token, newClaim.lease_token);

    const completeTask = await directRpc(coordUrl, 'tasks/send', { name: 'F2 relay completion fixture', kind: 'task', priority: 'normal', payload: { fixture: true } });
    const goodClaim = await relayRpc(relayUrl, randomUUID(), 'tasks/claim', { id: completeTask.id, agentId: COORD_AGENT_ID, leaseSeconds: 600 });
    assert.equal(goodClaim.status, 200);
    const completionId = randomUUID();
    const completion = await relayRpc(relayUrl, completionId, 'tasks/complete', {
      id: completeTask.id, agentId: COORD_AGENT_ID,
      leaseToken: goodClaim.body.result.lease_token,
      result: { accepted_once: true }, postAck: true, ackName: 'F2 isolated relay receipt',
    });
    assert.equal(completion.status, 200);
    assert.equal(completion.body.state, 'ok');
    const duplicateCompletion = await relayRpc(relayUrl, completionId, 'tasks/complete', {
      id: completeTask.id, agentId: COORD_AGENT_ID,
      leaseToken: goodClaim.body.result.lease_token,
      result: { accepted_once: true }, postAck: true, ackName: 'F2 isolated relay receipt',
    });
    assert.equal(duplicateCompletion.status, 504);
    const finalTask = await directRpc(coordUrl, 'tasks/get', { id: completeTask.id });
    assert.equal(finalTask.state, 'completed');
    assert.equal(finalTask.result.accepted_once, true);
    const acks = await directRpc(coordUrl, 'tasks/list', { kind: 'ack', limit: 100, skip_reclaim: true });
    assert.equal(acks.filter(task => task.name === 'F2 isolated relay receipt').length, 1);
    const journal = await readFile(join(folder, 'relay', 'requests.ndjson'), 'utf8');
    assert.equal(journal.includes(oldToken), false);
    assert.equal(journal.includes(goodClaim.body.result.lease_token), false);
    assert.equal(journal.includes('should_not_complete'), false);
    assert.equal(journal.includes('accepted_once'), false);
  } finally {
    await agent?.stop();
    await sidecar?.stop();
    if (coord.exitCode === null) {
      coord.kill();
      await new Promise(resolveExit => coord.once('exit', resolveExit));
    }
    await rm(folder, { recursive: true, force: true });
  }
});

test('synthetic OAuth-bound relay exercises progress, extend, submit, next claim and disconnect reclaim against a real isolated coord', async () => {
  const folder = await mkdtemp(join(tmpdir(), 'coord-g6-bound-live-'));
  const coordPort = await reservePort(); const agentPort = await reservePort(); const clientPort = await reservePort();
  const coordUrl = `http://127.0.0.1:${coordPort}/`;
  const child = spawn(coordExe, ['serve', '--addr', `127.0.0.1:${coordPort}`, '--db', join(folder, 'coord.db')], { stdio: 'ignore' });
  let sidecar; let agent;
  try {
    let ready = false;
    for (let i = 0; i < 100; i++) {
      if (child.exitCode !== null) throw new Error('isolated G6 daemon exited');
      try { await directRpc(coordUrl, 'tasks/list', { skip_reclaim: true, limit: 1 }); ready = true; break; } catch { await delay(100); }
    }
    assert.equal(ready, true);
    const batch = await directRpc(coordUrl, 'batches/create', { name: 'G6 synthetic identity lease test only', config: { require_progress: true, max_attempts: 3 } });
    await directRpc(coordUrl, 'batches/add_tasks', { batch_id: batch.id, tasks: [0, 1, 2].map(i => ({ key: `isolated-${i}`, name: `isolated-${i}`, payload: { synthetic: true } })) });
    const policy = identityFixture.fixturePolicy(batch.id);
    sidecar = await createSidecar({ agentHost: '127.0.0.1', agentPort, clientHost: '127.0.0.1', clientPort,
      allowedPeerIp: '127.0.0.1', agentSecret, clientSecret, journalFile: join(folder, 'requests.ndjson'), identityPolicy: policy });
    await sidecar.start();
    agent = await createAgent({ serverHost: '127.0.0.1', serverPort: agentPort, coordUrl, secret: agentSecret,
      heartbeatMs: 50, reconnectMinMs: 50, reconnectMaxMs: 100, identityPolicy: policy });
    agent.start(); assert.equal(await agent.waitUntilConnected(2_000), true);
    const invoke = async (index, method, params, requestId = randomUUID()) => {
      const principal = identity.resolvePrincipal(policy, identityFixture.fixtureAuth(index));
      const request = identity.signRequest({ requestId, method, params }, principal, policy);
      const response = await fetch(`http://127.0.0.1:${clientPort}/v1/rpc`, { method: 'POST', headers: { authorization: `Bearer ${clientSecret}`, 'content-type': 'application/json' }, body: JSON.stringify(request) });
      return { status: response.status, body: await response.json() };
    };
    const claim = async (index, seconds = 600) => {
      const response = await invoke(index, 'scheduler/claim', { agent_id: `A${index}`, batch_id: batch.id, lease_seconds: seconds });
      assert.equal(response.status, 200); return response.body.result.task;
    };
    const globalLegacy = await relayRpc(`http://127.0.0.1:${clientPort}`, randomUUID(), 'scheduler/claim', { agent_id: COORD_AGENT_ID });
    assert.equal(globalLegacy.body.code, 'identity_batch_required');
    assert.equal(globalLegacy.body.state, 'not_dispatched');
    assert.doesNotMatch(JSON.stringify(globalLegacy.body), /lease_token|payload|synthetic/);
    const untouched = await directRpc(coordUrl, 'tasks/list', { skip_reclaim: true, limit: 100 });
    assert.equal(untouched.length, 3);
    assert.ok(untouched.every(task => task.state === 'pending' && task.lease_token === null && task.claimed_by === null));
    const old = await claim(0, 1);
    const progressId = randomUUID();
    const checkpoint = await invoke(0, 'scheduler/progress', { id: old.id, agent_id: 'A0', lease_token: old.lease_token, attempt_no: old.attempt_no, sequence: 1, request_id: progressId, checkpoint: 'Synthetic checkpoint; not liveness evidence.' }, progressId);
    assert.equal(checkpoint.status, 200);
    const beforeExpiry = await directRpc(coordUrl, 'tasks/get', { id: old.id });
    assert.equal(beforeExpiry.lease_until, old.lease_until, 'progress must not extend a lease by itself');
    await delay(1_150);
    await directRpc(coordUrl, 'tasks/list', { skip_reclaim: false, limit: 100 });
    const reclaimed = await directRpc(coordUrl, 'tasks/get', { id: old.id });
    assert.equal(reclaimed.state, 'pending');
    if (reclaimed.available_at) await delay(Math.max(0, Date.parse(reclaimed.available_at) - Date.now()) + 25);
    // Exact-ID claim keeps this assertion independent of queue ordering.
    const claimed = await invoke(1, 'tasks/claim', { id: old.id, agentId: 'A1', leaseSeconds: 600 });
    assert.equal(claimed.status, 200); const current = claimed.body.result;
    assert.equal(current.attempt_no, old.attempt_no + 1);
    const staleId = randomUUID();
    const stale = await invoke(0, 'scheduler/progress', { id: old.id, agent_id: 'A0', lease_token: old.lease_token, attempt_no: old.attempt_no, sequence: 2, request_id: staleId, checkpoint: 'Old attempt must fail.' }, staleId);
    assert.equal(stale.body.error.code, -32001);
    const cross = await invoke(0, 'tasks/complete', { id: current.id, agentId: 'A0', leaseToken: current.lease_token, result: { cross_identity: true } });
    assert.equal(cross.body.error.code, -32001);
    const checkpointId = randomUUID();
    assert.equal((await invoke(1, 'scheduler/progress', { id: current.id, agent_id: 'A1', lease_token: current.lease_token, attempt_no: current.attempt_no, sequence: 1, request_id: checkpointId, checkpoint: 'Synthetic image adapter step completed.' }, checkpointId)).status, 200);
    const extensionId = randomUUID();
    const extensionParams = { id: current.id, agent_id: 'A1', lease_token: current.lease_token, attempt_no: current.attempt_no, progress_sequence: 1, request_id: extensionId, lease_seconds: 600 };
    assert.equal((await invoke(1, 'scheduler/extend_progress', extensionParams, extensionId)).status, 200);
    const reuseId = randomUUID();
    const reused = await invoke(1, 'scheduler/extend_progress', { ...extensionParams, request_id: reuseId }, reuseId);
    assert.equal(reused.body.error.code, -32001);
    const submitParams = { id: current.id, agentId: 'A1', leaseToken: current.lease_token, result: { synthetic: true, completed: 1 }, postAck: false };
    const submitId = randomUUID();
    const submit = await invoke(1, 'tasks/complete', submitParams, submitId);
    assert.equal(submit.status, 200);
    const exactReadback = await invoke(1, 'tasks/get', { id: current.id });
    assert.equal(exactReadback.body.result.state, 'completed');
    assert.equal(JSON.stringify(exactReadback.body.result).includes('lease_token'), false);
    assert.equal((await invoke(1, 'tasks/complete', submitParams, submitId)).status, 504, 'relay request-id replay must never dispatch again');
    assert.equal((await invoke(1, 'tasks/complete', submitParams)).status, 200, 'same fenced receipt can reconcile an accepted result without changing it');
    const next = await claim(1, 1);
    assert.notEqual(next.id, current.id, 'successful submit must allow another claim');
    const failing = await claim(0, 600);
    const failed = await invoke(0, 'scheduler/failure', { id: failing.id, agent_id: 'A0', lease_token: failing.lease_token, error: 'Synthetic bounded retry test.' });
    assert.equal(failed.status, 200);
    const staleFailureSubmit = await invoke(0, 'tasks/complete', { id: failing.id, agentId: 'A0', leaseToken: failing.lease_token, result: { invalid_after_failure: true } });
    assert.equal(staleFailureSubmit.body.error.code, -32001);
    await agent.stop();
    const unavailable = await invoke(1, 'tasks/get', { id: next.id });
    assert.equal(unavailable.body.state, 'not_dispatched');
    await delay(1_200);
    await directRpc(coordUrl, 'tasks/list', { skip_reclaim: false, limit: 100 });
    const disconnected = await directRpc(coordUrl, 'tasks/get', { id: next.id });
    assert.equal(disconnected.state, 'pending');
    assert.equal(disconnected.lease_token, null);
    const final = await directRpc(coordUrl, 'tasks/get', { id: current.id });
    assert.equal(final.result.completed, 1);
    assert.equal(final.state, 'completed');
  } finally {
    await agent?.stop(); await sidecar?.stop();
    if (child.exitCode === null) { child.kill(); await new Promise(done => child.once('exit', done)); }
    await rm(folder, { recursive: true, force: true });
  }
});

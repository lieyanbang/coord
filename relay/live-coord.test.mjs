import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createServer as createHttpServer } from 'node:http';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
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

const coordMcpRequire = createRequire(new URL('./coord-mcp/package.json', import.meta.url));
const express = coordMcpRequire('express');
const { registerCoordMcp, SidecarClient } = coordMcpRequire('./index.cjs');

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

test('Phase R trusted lifecycle survives stateless MCP calls and Core plus Relay restart', async () => {
  const folder = await mkdtemp(join(tmpdir(), 'coord-phase-r-live-'));
  const dbPath = join(folder, 'coord.db');
  const relayJournal = join(folder, 'relay', 'requests.ndjson');
  const secretFile = join(folder, 'client.secret');
  const mcpToken = 'phase-r-isolated-owner-token';
  await writeFile(secretFile, `${clientSecret}\n`, { mode: 0o600 });

  let coord;
  let coordUrl;
  let sidecar;
  let agent;
  const sidecarClient = new SidecarClient({
    url: 'http://127.0.0.1:1/v1/rpc',
    secretFile,
    timeoutMs: 6_000,
  });
  const oauth = {
    setCors(response) { response.setHeader('Access-Control-Allow-Origin', '*'); },
    challenge(response) { response.status(401).json({ error: 'unauthorized' }); },
    async authenticateRequest(request) {
      return request.get('authorization') === `Bearer ${mcpToken}`
        ? { clientId: 'phase-r-isolated-owner', expiresAt: Date.now() + 600_000 }
        : null;
    },
  };
  const mcpApp = express();
  const mcpHandle = await registerCoordMcp(mcpApp, {
    express,
    oauth,
    oauthModule: { installOAuthRoutes() {} },
    sidecar: sidecarClient,
    skipOAuthInit: true,
    identityPolicy: false,
  });
  const mcpPort = await reservePort();
  const mcpServer = createHttpServer(mcpApp);
  await new Promise((resolveListen, reject) => {
    mcpServer.once('error', reject);
    mcpServer.listen(mcpPort, '127.0.0.1', resolveListen);
  });
  const mcpUrl = `http://127.0.0.1:${mcpPort}/coord/mcp`;

  async function startCoord() {
    const port = await reservePort();
    const child = spawn(coordExe, ['serve', '--addr', `127.0.0.1:${port}`, '--db', dbPath], { stdio: 'ignore' });
    const url = `http://127.0.0.1:${port}/`;
    let ready = false;
    const deadline = Date.now() + 10_000;
    while (Date.now() < deadline) {
      if (child.exitCode !== null) throw new Error(`isolated Phase R Core exited with ${child.exitCode}`);
      try {
        await directRpc(url, 'tasks/list', { limit: 1, skip_reclaim: true });
        ready = true;
        break;
      } catch {
        await delay(100);
      }
    }
    if (!ready) {
      const exited = new Promise(resolveExit => child.once('exit', resolveExit));
      child.kill();
      await exited;
      throw new Error('isolated Phase R Core did not become ready');
    }
    return { child, url };
  }

  async function startRelay(coordUrl) {
    const currentSidecar = await createSidecar({
      agentHost: '127.0.0.1', agentPort: 0,
      clientHost: '127.0.0.1', clientPort: 0,
      allowedPeerIp: '127.0.0.1', agentId: RELAY_AGENT_ID,
      agentSecret, clientSecret,
      journalFile: relayJournal,
      requestTimeoutMs: 4_000, heartbeatTimeoutMs: 2_000,
    });
    const addresses = await currentSidecar.start();
    let currentAgent;
    try {
      currentAgent = await createAgent({
      serverHost: '127.0.0.1', serverPort: addresses.agentAddress.port,
      agentId: RELAY_AGENT_ID, coordUrl, secret: agentSecret,
      heartbeatMs: 50, reconnectMinMs: 50, reconnectMaxMs: 100, coordTimeoutMs: 3_000,
      });
      currentAgent.start();
      assert.equal(await currentAgent.waitUntilConnected(2_000), true);
    } catch (error) {
      await currentAgent?.stop();
      await currentSidecar.stop();
      throw error;
    }
    const url = `http://127.0.0.1:${addresses.clientAddress.port}/v1/rpc`;
    sidecarClient.url = url;
    return { sidecar: currentSidecar, agent: currentAgent, url };
  }

  async function stopRelay() {
    await agent?.stop();
    await sidecar?.stop();
    agent = undefined;
    sidecar = undefined;
  }

  async function mcpTool(name, args) {
    const response = await fetch(mcpUrl, {
      method: 'POST',
      headers: {
        authorization: `Bearer ${mcpToken}`,
        accept: 'application/json, text/event-stream',
        'content-type': 'application/json',
        'mcp-protocol-version': '2025-03-26',
      },
      body: JSON.stringify({
        jsonrpc: '2.0', id: randomUUID(), method: 'tools/call',
        params: { name, arguments: args },
      }),
    });
    const text = await response.text();
    const dataLine = text.split(/\r?\n/).find(line => line.startsWith('data:'));
    const payload = dataLine ? JSON.parse(dataLine.slice(5).trim()) : JSON.parse(text);
    assert.equal(response.status, 200, `${name}: ${text}`);
    assert.equal(payload.error, undefined, `${name}: ${text}`);
    return payload.result;
  }

  async function toolJson(name, args) {
    const result = await mcpTool(name, args);
    assert.equal(result.isError, undefined, `${name}: ${JSON.stringify(result)}`);
    return JSON.parse(result.content[0].text);
  }

  async function stopCoord() {
    if (coord?.exitCode === null) {
      const exited = new Promise(resolveExit => coord.once('exit', resolveExit));
      coord.kill();
      await exited;
    }
    coord = undefined;
  }

  try {
    ({ child: coord, url: coordUrl } = await startCoord());
    ({ sidecar, agent } = await startRelay(coordUrl));

    const batch = await directRpc(coordUrl, 'batches/create', {
      name: 'Phase R isolated lifecycle',
      config: { max_attempts: 3, retry_backoff_ms: 1, max_backoff_ms: 2 },
    });
    await directRpc(coordUrl, 'batches/add_tasks', {
      batch_id: batch.id,
      tasks: [0, 1, 2].map(index => ({ key: `phase-r-${index}`, name: `Phase R ${index}`, payload: { fixture: true } })),
    });

    // Each call is a separate stateless MCP HTTP request. The OAuth identity is
    // synthetic, but the production MCP/sidecar/Core code paths are real.
    const first = await toolJson('coord_claim', { batch_id: batch.id, worker_id: '0001' });
    const firstTaskId = first.id;
    assert.equal(first.attempt_no, 1);
    assert.doesNotMatch(JSON.stringify(first), /lease_token|leaseToken/);
    const internalClaim = await directRpc(coordUrl, 'tasks/get', { id: firstTaskId });
    assert.equal(typeof internalClaim.lease_token, 'string');
    await toolJson('coord_progress', {
      batch_id: batch.id, worker_id: '0001', task_id: firstTaskId,
      attempt_no: first.attempt_no, checkpoint: 'Core and Relay integration checkpoint',
    });

    // Restart both persistent services after saving a checkpoint. The same DB
    // and sidecar journal are reopened; the MCP HTTP listener remains stateless.
    await stopRelay();
    await stopCoord();
    ({ child: coord, url: coordUrl } = await startCoord());
    ({ sidecar, agent } = await startRelay(coordUrl));
    const resumed = await toolJson('coord_resume', { batch_id: batch.id, worker_id: '0001' });
    assert.equal(resumed.state, 'active');
    assert.equal(resumed.checkpoint, 'Core and Relay integration checkpoint');
    assert.equal(resumed.task.id, firstTaskId);
    assert.doesNotMatch(JSON.stringify(resumed), /lease_token|leaseToken/);
    const repeatedClaim = await toolJson('coord_claim', { batch_id: batch.id, worker_id: '0001' });
    assert.equal(repeatedClaim.id, firstTaskId);
    assert.equal(repeatedClaim.attempt_no, first.attempt_no);

    const firstResult = { accepted_after_restart: true };
    const completionArgs = {
      batch_id: batch.id, worker_id: '0001', task_id: firstTaskId,
      attempt_no: first.attempt_no, result: firstResult,
      ack: { name: 'Phase R isolated lifecycle acknowledgement' },
    };
    const completed = await toolJson('coord_complete', completionArgs);
    assert.equal(completed.state, 'completed');
    const replayed = await toolJson('coord_complete', completionArgs);
    assert.equal(replayed.state, 'completed');
    assert.equal(replayed.replayed, true);
    const storedFirst = await directRpc(coordUrl, 'tasks/get', { id: firstTaskId });
    assert.equal(storedFirst.result.accepted_after_restart, true);
    const acks = await directRpc(coordUrl, 'tasks/list', { kind: 'ack', limit: 100, skip_reclaim: true });
    assert.equal(acks.filter(task => task.name === 'Phase R isolated lifecycle acknowledgement').length, 1);

    const released = await toolJson('coord_claim', { batch_id: batch.id, worker_id: '0002' });
    await toolJson('coord_progress', {
      batch_id: batch.id, worker_id: '0002', task_id: released.id,
      attempt_no: released.attempt_no, checkpoint: 'Checkpoint before worker handoff',
    });
    const releaseResult = await toolJson('coord_release', {
      batch_id: batch.id, worker_id: '0002', task_id: released.id,
      attempt_no: released.attempt_no, reason: 'synthetic context interruption',
    });
    assert.equal(releaseResult.state, 'pending');
    const releasedResume = await toolJson('coord_resume', { batch_id: batch.id, worker_id: '0002' });
    assert.equal(releasedResume.state, 'released');
    assert.equal(releasedResume.checkpoint, 'Checkpoint before worker handoff');
    await delay(10);
    const handoff = await toolJson('coord_claim', { batch_id: batch.id, worker_id: '0003' });
    assert.equal(handoff.id, released.id);
    assert.equal(handoff.attempt_no, released.attempt_no + 1);
    const handoffResume = await toolJson('coord_resume', { batch_id: batch.id, worker_id: '0003' });
    assert.equal(handoffResume.state, 'active');
    assert.equal(handoffResume.checkpoint, 'Checkpoint before worker handoff');
    const staleCompletion = await mcpTool('coord_complete', {
      batch_id: batch.id, worker_id: '0002', task_id: released.id,
      attempt_no: released.attempt_no, result: { stale: true },
    });
    assert.equal(staleCompletion.isError, true);
    const currentHandoffTask = await directRpc(coordUrl, 'tasks/get', { id: released.id });
    assert.equal(currentHandoffTask.claimed_by, '0003');
    assert.equal(currentHandoffTask.result, null);
    await toolJson('coord_complete', {
      batch_id: batch.id, worker_id: '0003', task_id: released.id,
      attempt_no: handoff.attempt_no, result: { handoff_completed: true },
    });

    const expiring = await toolJson('coord_claim', {
      batch_id: batch.id, worker_id: '0004', lease_seconds: 1,
    });
    await delay(1_150);
    const expiredResume = await toolJson('coord_resume', { batch_id: batch.id, worker_id: '0004' });
    assert.equal(expiredResume.state, 'expired');
    let recovered;
    const recoveryDeadline = Date.now() + 2_000;
    while (Date.now() < recoveryDeadline) {
      recovered = await toolJson('coord_claim', { batch_id: batch.id, worker_id: '0005' });
      if (recovered.id) break;
      await delay(50);
    }
    assert.ok(recovered?.id, 'recovered task should become claimable after interruption backoff');
    assert.equal(recovered.id, expiring.id);
    assert.equal(recovered.attempt_no, expiring.attempt_no + 1);
    const recoveredStored = await directRpc(coordUrl, 'tasks/get', { id: recovered.id });
    assert.equal(recoveredStored.failure_count, 0);
    await toolJson('coord_complete', {
      batch_id: batch.id, worker_id: '0005', task_id: recovered.id,
      attempt_no: recovered.attempt_no, result: { recovered_after_expiry: true },
    });
    const status = await toolJson('coord_status', { batch_id: batch.id, worker_id: '0001' });
    assert.equal(status.active_claims, 0);
    assert.equal(status.failure_count, 0);

    const journal = await readFile(relayJournal, 'utf8');
    assert.equal(journal.includes(internalClaim.lease_token), false);
    assert.equal(journal.includes(JSON.stringify(firstResult)), false);
  } finally {
    await stopRelay();
    await stopCoord();
    await new Promise(resolveClose => {
      if (!mcpServer.listening) return resolveClose();
      mcpServer.close(resolveClose);
      mcpServer.closeAllConnections();
    });
    await mcpHandle.shutdown();
    await rm(folder, { recursive: true, force: true });
  }
});

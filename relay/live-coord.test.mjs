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
import { COORD_AGENT_ID, RELAY_AGENT_ID } from './protocol.mjs';

const agentSecret = '55'.repeat(32);
const clientSecret = '66'.repeat(32);
const coordExe = process.env.COORD_TEST_BIN
  ? resolve(process.env.COORD_TEST_BIN)
  : resolve('target/release/coord.exe');

function reservePort() {
  return new Promise((resolvePort, reject) => {
    const server = createHttpServer();
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const port = server.address().port;
      server.close(error => error ? reject(error) : resolvePort(port));
    });
  });
}

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

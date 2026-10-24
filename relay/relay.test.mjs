import assert from 'node:assert/strict';
import { createServer as createHttpServer } from 'node:http';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHmac, randomUUID } from 'node:crypto';
import { test } from 'node:test';
import { createAgent } from './agent.mjs';
import { createSidecar } from './server.mjs';
import { isFetchBlockedPort, listen, reservePort } from './test-ports.mjs';
import { COORD_AGENT_ID, makeAgentProof, validateRpcRequest } from './protocol.mjs';
import identity from './identity.cjs';
import identityFixture from './identity-test-fixture.cjs';

const agentSecret = '11'.repeat(32);
const clientSecret = '22'.repeat(32);

async function startCoord(handler) {
  const state = { count: 0 };
  const server = createHttpServer(async (request, response) => {
    let raw = '';
    for await (const chunk of request) raw += chunk.toString('utf8');
    const rpc = JSON.parse(raw);
    state.count += 1;
    const result = await handler(rpc, request.headers);
    if (result.httpRedirect) {
      response.writeHead(302, { location: result.httpRedirect });
      response.end();
      return;
    }
    response.writeHead(200, { 'content-type': 'application/json' });
    response.end(JSON.stringify({ jsonrpc: '2.0', id: rpc.id, ...result }));
  });
  const address = await listen(server);
  return {
    state,
    url: `http://127.0.0.1:${address.port}/`,
    close: () => new Promise(resolve => {
      server.closeAllConnections?.();
      server.close(resolve);
    }),
  };
}

async function startRig({ handler, timeoutMs = 1_000, maxPending = 4, allowedPeerIp = '127.0.0.1', agentSecretForClient = agentSecret, expectAgentConnected = true, identityPolicy = null, agentIdentityPolicy = identityPolicy, coordAuthKey = null } = {}) {
  const folder = await mkdtemp(join(tmpdir(), 'coord-relay-test-'));
  const journalFile = join(folder, 'requests.ndjson');
  const coord = await startCoord(handler ?? (async () => ({ result: [] })));
  const clientPort = await reservePort();
  const sidecar = await createSidecar({
    agentHost: '127.0.0.1',
    agentPort: 0,
    clientHost: '127.0.0.1',
    clientPort,
    allowedPeerIp,
    agentId: 'test-agent',
    agentSecret,
    clientSecret,
    journalFile,
    requestTimeoutMs: timeoutMs,
    heartbeatTimeoutMs: 2_000,
    maxPending,
    identityPolicy,
  });
  const address = await sidecar.start();
  let coordAuthKeyFile;
  if (coordAuthKey) {
    coordAuthKeyFile = join(folder, 'coord-rpc.key');
    await writeFile(coordAuthKeyFile, `${coordAuthKey}\n`, { mode: 0o600 });
  }
  const agent = await createAgent({
    serverHost: '127.0.0.1',
    serverPort: address.agentAddress.port,
    agentId: 'test-agent',
    coordUrl: coord.url,
    secret: agentSecretForClient,
    heartbeatMs: 50,
    reconnectMinMs: 300,
    reconnectMaxMs: 300,
    coordTimeoutMs: 1_000,
    maxInflight: 4,
    identityPolicy: agentIdentityPolicy,
    ...(coordAuthKeyFile ? { coordAuthKeyFile } : {}),
  });
  agent.start();
  assert.equal(await agent.waitUntilConnected(1_000), expectAgentConnected && allowedPeerIp === '127.0.0.1');
  return {
    agent,
    coord,
    sidecar,
    agentPort: address.agentAddress.port,
    journalFile,
    clientUrl: `http://127.0.0.1:${address.clientAddress.port}`,
    async close() {
      await this.agent.stop();
      await this.sidecar.stop();
      await coord.close();
      await rm(folder, { recursive: true, force: true });
    },
  };
}

async function rpc(clientUrl, requestId = randomUUID(), method = 'tasks/list', params = { limit: 10, skip_reclaim: true }, token = clientSecret) {
  const response = await fetch(`${clientUrl}/v1/rpc`, {
    method: 'POST',
    headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
    body: JSON.stringify({ requestId, method, params }),
  });
  return { status: response.status, body: await response.json() };
}

test('whitelist and strict schemas reject side effects and reclaiming list calls', () => {
  const base = { requestId: randomUUID(), method: 'tasks/list', params: { limit: 10, skip_reclaim: true } };
  assert.equal(validateRpcRequest(base).ok, true);
  assert.equal(validateRpcRequest({ ...base, method: 'tasks/send', params: { name: 'x' } }).ok, false);
  assert.equal(validateRpcRequest({ ...base, method: 'actions/status', params: { batch_id: randomUUID() } }).ok, false);
  assert.equal(validateRpcRequest({ ...base, params: { limit: 10, skip_reclaim: false } }).ok, false);
  assert.equal(validateRpcRequest({ ...base, params: { limit: 10, skip_reclaim: true, wait_ms: 500 } }).ok, false);
  assert.equal(validateRpcRequest({ ...base, params: { limit: 10, skip_reclaim: true, arbitrary: true } }).ok, false);
  assert.equal(validateRpcRequest({ ...base, requestId: 'not-a-uuid' }).ok, false);
  const progressId = randomUUID();
  const progressParams = {
    id: randomUUID(), agent_id: COORD_AGENT_ID, lease_token: randomUUID(),
    attempt_no: 1, sequence: 1, request_id: progressId, checkpoint: 'checked refs',
  };
  assert.equal(validateRpcRequest({ requestId: progressId, method: 'scheduler/progress', params: progressParams }).ok, true);
  assert.equal(validateRpcRequest({ requestId: randomUUID(), method: 'scheduler/progress', params: progressParams }).code, 'request_id_mismatch');
  assert.equal(validateRpcRequest({ requestId: progressId, method: 'scheduler/progress', params: { ...progressParams, agent_id: 'attacker' } }).ok, false);
  assert.equal(validateRpcRequest({ requestId: randomUUID(), method: 'scheduler/agent_status', params: { agent_id: COORD_AGENT_ID, batch_id: randomUUID() } }).ok, true);
  assert.ok(makeAgentProof(agentSecret, 'test-agent', 'nonce').length === 64);
  assert.equal(typeof COORD_AGENT_ID, 'string');
});

test('HTTP test listeners avoid Fetch-blocked ports', async t => {
  assert.equal(isFetchBlockedPort(10080), true);
  const server = createHttpServer((request, response) => response.end('ok'));
  const address = await listen(server);
  t.after(() => new Promise(resolve => server.close(resolve)));
  assert.equal(isFetchBlockedPort(address.port), false);
  const response = await fetch(`http://127.0.0.1:${address.port}/`);
  assert.equal(response.status, 200);
});

test('authenticated Arasaka agent round-trips through sidecar to coord; denied calls never dispatch', async t => {
  const rig = await startRig({
    handler: async request => ({ result: [
      { id: 'foreign', claimed_by: 'other-agent', lease_token: 'must-not-leak' },
      { id: 'owned', claimed_by: COORD_AGENT_ID, lease_token: 'owned-token' },
    ] }),
  });
  t.after(() => rig.close());

  const unauthorized = await rpc(rig.clientUrl, randomUUID(), 'tasks/list', { limit: 1, skip_reclaim: true }, '33'.repeat(32));
  assert.equal(unauthorized.status, 401);
  const deniedMethod = await rpc(rig.clientUrl, randomUUID(), 'tasks/send', { name: 'must not dispatch' });
  assert.equal(deniedMethod.status, 400);
  const malformed = await rpc(rig.clientUrl, randomUUID(), 'tasks/list', { limit: 1, skip_reclaim: false });
  assert.equal(malformed.status, 400);
  assert.equal(rig.coord.state.count, 0);

  const requestId = randomUUID();
  const accepted = await rpc(rig.clientUrl, requestId);
  assert.equal(accepted.status, 200);
  assert.equal(accepted.body.state, 'ok');
  assert.equal(Object.hasOwn(accepted.body.result[0], 'lease_token'), false);
  assert.equal(accepted.body.result[1].lease_token, 'owned-token');
  assert.equal(rig.coord.state.count, 1);

  const duplicate = await rpc(rig.clientUrl, requestId);
  assert.equal(duplicate.status, 504);
  assert.equal(duplicate.body.state, 'uncertain');
  assert.equal(rig.coord.state.count, 1);
});

test('concurrent duplicate request ids are dispatched at most once', async t => {
  const rig = await startRig({
    handler: async () => {
      await new Promise(resolve => setTimeout(resolve, 100));
      return { result: [] };
    },
  });
  t.after(() => rig.close());

  const requestId = randomUUID();
  const calls = Array.from({ length: 8 }, () => rpc(rig.clientUrl, requestId).catch(error => ({ error })));
  await new Promise(resolve => setTimeout(resolve, 50));
  const journal = (await readFile(rig.journalFile, 'utf8'))
    .trim().split('\n').filter(Boolean).map(line => JSON.parse(line));
  assert.equal(journal.filter(record => record.requestId === requestId && record.state === 'dispatched').length, 1);
  const responses = await Promise.all(calls);
  assert.equal(responses.filter(response => response.status === 200).length, 1);
  assert.equal(responses.filter(response => response.status === 504 && response.body.state === 'uncertain').length, 7);
  assert.equal(rig.coord.state.count, 1);
});

test('journal fingerprints are keyed HMACs and omit request parameters', async t => {
  const rig = await startRig();
  t.after(() => rig.close());

  const requestId = randomUUID();
  const params = { limit: 10, skip_reclaim: true };
  const response = await rpc(rig.clientUrl, requestId, 'tasks/list', params);
  assert.equal(response.status, 200);

  const journal = await readFile(rig.journalFile, 'utf8');
  const record = journal.trim().split('\n').map(line => JSON.parse(line))
    .find(entry => entry.requestId === requestId && entry.state === 'dispatched');
  const expected = createHmac('sha256', Buffer.from(clientSecret, 'hex'))
    .update(JSON.stringify({ method: 'tasks/list', params }))
    .digest('hex');
  assert.equal(record.requestHash, expected);
  assert.equal(journal.includes('skip_reclaim'), false);
});

test('over-limit body and concurrency are rejected before dispatch', async t => {
  const rig = await startRig({
    timeoutMs: 1_000,
    maxPending: 1,
    handler: async () => {
      await new Promise(resolve => setTimeout(resolve, 250));
      return { result: [] };
    },
  });
  t.after(() => rig.close());

  const oversized = await fetch(`${rig.clientUrl}/v1/rpc`, {
    method: 'POST',
    headers: { authorization: `Bearer ${clientSecret}`, 'content-type': 'application/json' },
    body: JSON.stringify({ requestId: randomUUID(), method: 'tasks/list', params: { limit: 1, skip_reclaim: true, filler: 'x'.repeat(70_000) } }),
  });
  assert.equal(oversized.status, 413);
  assert.equal(rig.coord.state.count, 0);

  const first = rpc(rig.clientUrl);
  await new Promise(resolve => setTimeout(resolve, 30));
  const second = await rpc(rig.clientUrl);
  assert.equal(second.status, 429);
  assert.equal(second.body.state, 'not_dispatched');
  assert.equal((await first).status, 200);
  assert.equal(rig.coord.state.count, 1);
});

test('timeout and late coord response stay uncertain and are never replayed', async t => {
  const rig = await startRig({
    timeoutMs: 100,
    handler: async () => {
      await new Promise(resolve => setTimeout(resolve, 250));
      return { result: { ok: true } };
    },
  });
  t.after(() => rig.close());
  const requestId = randomUUID();
  const first = await rpc(rig.clientUrl, requestId);
  assert.equal(first.status, 504);
  assert.equal(first.body.state, 'uncertain');
  await new Promise(resolve => setTimeout(resolve, 300));
  const retry = await rpc(rig.clientUrl, requestId);
  assert.equal(retry.status, 504);
  assert.equal(retry.body.state, 'uncertain');
  assert.equal(rig.coord.state.count, 1);
});

test('oversized coord results are never returned as successful RPC responses', async t => {
  const rig = await startRig({ handler: async () => ({ result: { oversized: 'x'.repeat(70_000) } }) });
  t.after(() => rig.close());
  const result = await rpc(rig.clientUrl);
  assert.equal(result.status, 504);
  assert.equal(result.body.state, 'uncertain');
  assert.equal(result.body.code, 'coord_response_too_large');
  assert.equal(rig.coord.state.count, 1);
});

test('unauthorized network peer is rejected before an agent session is established', async t => {
  const denied = await startRig({ allowedPeerIp: '192.0.2.55' });
  t.after(() => denied.close());
  const health = await fetch(`${denied.clientUrl}/healthz`);
  assert.equal(health.status, 503);
  assert.equal((await health.json()).agentConnected, false);
  assert.equal(denied.coord.state.count, 0);
});

test('incorrect shared secret never authenticates', async t => {
  const denied = await startRig({ agentSecretForClient: '44'.repeat(32), expectAgentConnected: false });
  t.after(() => denied.close());
  const health = await fetch(`${denied.clientUrl}/healthz`);
  assert.equal(health.status, 503);
  assert.equal((await health.json()).agentConnected, false);
  assert.equal(denied.coord.state.count, 0);
});

test('agent reconnects after its connection drops and resumes bounded RPC', async t => {
  const rig = await startRig();
  t.after(() => rig.close());
  await rig.agent.stop();
  const replacement = await createAgent({
    serverHost: '127.0.0.1', serverPort: rig.agentPort,
    agentId: 'test-agent', coordUrl: rig.coord.url, secret: agentSecret,
    heartbeatMs: 50, reconnectMinMs: 50, reconnectMaxMs: 100,
  });
  rig.agent = replacement;
  replacement.start();
  assert.equal(await replacement.waitUntilConnected(1_000), true);
  const result = await rpc(rig.clientUrl);
  assert.equal(result.status, 200);
  assert.equal(result.body.state, 'ok');
  assert.equal(rig.coord.state.count, 1);
  await replacement.stop();
});

test('sidecar restart marks dispatched requests uncertain and never replays them', async t => {
  const rig = await startRig({
    timeoutMs: 2_000,
    handler: async () => {
      await new Promise(resolve => setTimeout(resolve, 300));
      return { result: { ok: true } };
    },
  });
  t.after(() => rig.close());

  const requestId = randomUUID();
  const pendingCall = rpc(rig.clientUrl, requestId).catch(() => ({ transportInterrupted: true }));
  const deadline = Date.now() + 1_000;
  while (rig.coord.state.count === 0 && Date.now() < deadline) await new Promise(resolve => setTimeout(resolve, 10));
  assert.equal(rig.coord.state.count, 1);

  await rig.sidecar.stop();
  const clientPort = await reservePort();
  const recovered = await createSidecar({
    agentHost: '127.0.0.1', agentPort: rig.agentPort,
    clientHost: '127.0.0.1', clientPort,
    allowedPeerIp: '127.0.0.1', agentId: 'test-agent',
    agentSecret, clientSecret, journalFile: rig.journalFile,
    requestTimeoutMs: 2_000, heartbeatTimeoutMs: 2_000,
  });
  const addresses = await recovered.start();
  rig.sidecar = recovered;
  rig.clientUrl = `http://127.0.0.1:${addresses.clientAddress.port}`;
  let ready = false;
  const reconnectDeadline = Date.now() + 1_500;
  while (!ready && Date.now() < reconnectDeadline) {
    const health = await fetch(`${rig.clientUrl}/healthz`);
    ready = health.status === 200;
    if (!ready) await new Promise(resolve => setTimeout(resolve, 20));
  }
  assert.equal(ready, true);
  const replay = await rpc(rig.clientUrl, requestId);
  assert.equal(replay.status, 504);
  assert.equal(replay.body.state, 'uncertain');
  await new Promise(resolve => setTimeout(resolve, 350));
  assert.equal(rig.coord.state.count, 1);
  await pendingCall;
});

test('identity registry rejects shared clients, token substitution, expiry and cross-batch upgrades', () => {
  const now = Date.now();
  const batch = randomUUID();
  const policy = identityFixture.fixturePolicy(batch, now);
  const auth = identityFixture.fixtureAuth(0, now);
  const principal = identity.resolvePrincipal(policy, auth, now);
  assert.equal(principal.agent_id, 'A0');
  assert.throws(() => identity.resolvePrincipal(policy, { ...auth, tokenSha256: identity.hash('token-1') }, now));
  assert.throws(() => identity.resolvePrincipal(policy, auth, now + 400_000));
  assert.throws(() => identity.resolvePrincipal(policy, { ...auth, expiresAt: now - 1 }, now));
  assert.equal(identity.resolvePrincipal(policy, { clientId: 'legacy-client' }, now), null);
  const shared = structuredClone(policy); delete shared.key;
  shared.bindings[1].client_sha256 = shared.bindings[0].client_sha256;
  assert.throws(() => identity.validatePolicy(shared, policy.key));
  const request = { requestId: randomUUID(), method: 'scheduler/claim', params: { agent_id: 'A0', batch_id: batch } };
  const signed = identity.signRequest(request, principal, policy);
  assert.equal(validateRpcRequest(signed, policy).ok, true);
  assert.equal(validateRpcRequest(signed).code, 'identity_disabled');
  assert.equal(validateRpcRequest({ ...signed, identity: { ...signed.identity, proof: { toString: 'invalid' } } }, policy).code, 'identity_invalid');
  assert.equal(validateRpcRequest({ ...signed, params: { ...signed.params, agent_id: 'A1' } }, policy).ok, false);
  const otherBatch = identity.signRequest({ ...request, params: { ...request.params, batch_id: randomUUID() } }, principal, policy);
  assert.equal(validateRpcRequest(otherBatch, policy).code, 'identity_batch_mismatch');
  assert.equal(identity.verifyRequest(signed, policy, now + 60_001).ok, false);
  const otherAgent = identity.signRequest({ ...request, params: { ...request.params, agent_id: 'A1' } }, principal, policy);
  assert.equal(validateRpcRequest(otherAgent, policy).code, 'identity_agent_mismatch');
  const readPolicy = identityFixture.fixturePolicy(batch, now, ['coord:read']);
  const denied = identity.signRequest(request, identity.resolvePrincipal(readPolicy, auth, now), readPolicy);
  assert.equal(validateRpcRequest(denied, readPolicy).code, 'identity_scope_denied');
});

async function signedRelayCall(rig, request) {
  const response = await fetch(`${rig.clientUrl}/v1/rpc`, { method: 'POST',
    headers: { authorization: `Bearer ${clientSecret}`, 'content-type': 'application/json' }, body: JSON.stringify(request) });
  return { status: response.status, body: await response.json() };
}

test('bound relay contexts reach the correct agent and keep task/batch scope, replay and legacy fences', async t => {
  const batch = randomUUID(); const foreignBatch = randomUUID();
  const ownedId = randomUUID(); const foreignId = randomUUID(); const lease = randomUUID();
  const policy = identityFixture.fixturePolicy(batch);
  const principal = identity.resolvePrincipal(policy, identityFixture.fixtureAuth(0));
  const sign = (method, params, requestId = randomUUID()) => identity.signRequest({ requestId, method, params }, principal, policy);
  const rig = await startRig({ identityPolicy: policy, handler: async request => {
    if (request.method === 'tasks/get') return { result: { id: request.params.id, batch_id: request.params.id === foreignId ? foreignBatch : batch, claimed_by: 'A0', lease_token: lease } };
    if (request.method === 'tasks/list') return { result: [{ id: ownedId, batch_id: batch }, { id: foreignId, batch_id: foreignBatch }] };
    assert.equal(request.params.agent_id, 'A0');
    assert.equal(request.params.batch_id, batch);
    return { result: { empty: false, task: { id: ownedId, batch_id: batch, claimed_by: 'A0', lease_token: lease } } };
  } });
  t.after(() => rig.close());
  const request = sign('scheduler/claim', { agent_id: 'A0', batch_id: batch });
  const accepted = await signedRelayCall(rig, request);
  assert.equal(accepted.status, 200);
  assert.equal(accepted.body.result.task.lease_token, lease);
  assert.equal((await signedRelayCall(rig, request)).status, 504);
  const crossBatch = await signedRelayCall(rig, sign('tasks/get', { id: foreignId }));
  assert.equal(crossBatch.body.code, 'identity_batch_denied');
  assert.equal(JSON.stringify(crossBatch).includes(foreignId), false);
  const legacyOwned = await rpc(rig.clientUrl, randomUUID(), 'tasks/get', { id: ownedId });
  assert.equal(legacyOwned.body.code, 'identity_batch_denied');
  const legacyBatch = await rpc(rig.clientUrl, randomUUID(), 'scheduler/claim', { agent_id: COORD_AGENT_ID, batch_id: batch });
  assert.equal(legacyBatch.body.code, 'identity_batch_denied');
  const legacyGlobal = await rpc(rig.clientUrl, randomUUID(), 'scheduler/claim', { agent_id: COORD_AGENT_ID });
  assert.equal(legacyGlobal.body.code, 'identity_batch_required');
  const legacyList = await rpc(rig.clientUrl);
  assert.deepEqual(legacyList.body.result.map(task => task.id), [foreignId]);
  const forged = structuredClone(request); forged.requestId = randomUUID();
  assert.equal((await signedRelayCall(rig, forged)).status, 400);
  const journal = await readFile(rig.journalFile, 'utf8');
  assert.equal(journal.includes(lease), false);
  assert.equal(journal.includes(principal.binding_id), false);
  assert.equal(journal.includes(request.identity.proof), false);
});

test('Arasaka relay independently rejects bound requests when its identity policy is absent', async t => {
  const batch = randomUUID(); const policy = identityFixture.fixturePolicy(batch);
  const rig = await startRig({ identityPolicy: policy, agentIdentityPolicy: null });
  t.after(() => rig.close());
  const request = identity.signRequest({ requestId: randomUUID(), method: 'scheduler/claim', params: { agent_id: 'A0', batch_id: batch } }, identity.resolvePrincipal(policy, identityFixture.fixtureAuth(0)), policy);
  const result = await signedRelayCall(rig, request);
  assert.equal(result.body.code, 'identity_disabled');
  assert.equal(rig.coord.state.count, 0);
});

test('optional protected coord RPC key is sent only by the loopback Agent and absent in the legacy mode', async t => {
  const mainRpcKey = '77'.repeat(32);
  let authenticated = 0;
  const protectedRig = await startRig({ coordAuthKey: mainRpcKey, handler: async (request, headers) => {
    assert.equal(headers.authorization, `Bearer ${mainRpcKey}`); authenticated += 1; return { result: [] };
  } });
  t.after(() => protectedRig.close());
  const result = await rpc(protectedRig.clientUrl);
  assert.equal(result.status, 200);
  assert.equal(authenticated, 1);
  assert.equal(JSON.stringify(result.body).includes(mainRpcKey), false);
  assert.equal((await readFile(protectedRig.journalFile, 'utf8')).includes(mainRpcKey), false);
  const legacy = await startRig({ handler: async (request, headers) => {
    assert.equal(headers.authorization, undefined); return { result: [] };
  } });
  t.after(() => legacy.close());
  assert.equal((await rpc(legacy.clientUrl)).status, 200);
});

test('coord RPC key configuration rejects malformed keys and non-loopback endpoints before connecting', async t => {
  const folder = await mkdtemp(join(tmpdir(), 'coord-rpc-key-test-'));
  t.after(() => rm(folder, { recursive: true, force: true }));
  const coordAuthKeyFile = join(folder, 'invalid.key');
  await writeFile(coordAuthKeyFile, 'not-a-key');
  await assert.rejects(createAgent({ secret: agentSecret, coordAuthKeyFile }), /32-byte hex/);
  await assert.rejects(createAgent({ secret: agentSecret, coordAuthKeyFile: 'relative.key' }), /absolute/);
  await assert.rejects(createAgent({ secret: agentSecret, coordAuthKeyFile, coordUrl: 'http://192.0.2.1:7777/' }), /local IPv4/);
  await writeFile(coordAuthKeyFile, 'a'.repeat(129));
  await assert.rejects(createAgent({ secret: agentSecret, coordAuthKeyFile }), /too large/);
});

test('authenticated coord RPC never follows a redirect with its private credential', async t => {
  let redirectedRequests = 0;
  const destination = createHttpServer((request, response) => { redirectedRequests += 1; response.end('must not be reached'); });
  const address = await listen(destination);
  t.after(() => new Promise(done => destination.close(done)));
  const rig = await startRig({ coordAuthKey: '88'.repeat(32), handler: async () => ({ httpRedirect: `http://127.0.0.1:${address.port}/` }) });
  t.after(() => rig.close());
  const result = await rpc(rig.clientUrl);
  assert.equal(result.body.state, 'uncertain');
  assert.equal(result.body.code, 'coord_outcome_unknown');
  assert.equal(redirectedRequests, 0);
});

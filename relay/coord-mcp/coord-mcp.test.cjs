const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs/promises');
const http = require('node:http');
const os = require('node:os');
const path = require('node:path');
const { after, test } = require('node:test');
const express = require('express');
const {
  MAX_BODY_BYTES,
  MAX_CONCURRENT,
  RelayOutcomeError,
  SidecarClient,
  TOOLS,
  registerCoordMcp,
  removeLeaseTokens,
  validateToolInput,
} = require('./index.cjs');
const { createCoordOAuthServer } = require('./oauth.cjs');
const identity = require('../identity.cjs');
const { fixturePolicy, fixtureAuth } = require('../identity-test-fixture.cjs');

const ACCESS_TOKEN = 'coord-test-access-token';
const TASK_ID = '123e4567-e89b-42d3-a456-426614174000';
const LEASE_TOKEN = '123e4567-e89b-42d3-a456-426614174001';
const BATCH_ID = '123e4567-e89b-42d3-a456-426614174002';

const fixtures = new Set();
const tempDirs = new Set();

after(async () => {
  await Promise.all([...fixtures].map((fixture) => fixture.close()));
  await Promise.all([...tempDirs].map((dir) => fs.rm(dir, { recursive: true, force: true })));
});

function fakeOAuth(authInfo) {
  return {
    async init() {},
    setCors(res) {
      res.setHeader('Access-Control-Allow-Origin', '*');
      res.setHeader('Access-Control-Allow-Headers', 'authorization, content-type, mcp-protocol-version');
    },
    challenge(res, invalid = false) {
      this.setCors(res);
      res.setHeader('WWW-Authenticate', 'Bearer resource_metadata="https://example.test/coord/.well-known/oauth-protected-resource/mcp"');
      res.status(401).json({ error: invalid ? 'invalid_token' : 'unauthorized' });
    },
    async authenticateRequest(req) {
      return req.get('authorization') === `Bearer ${ACCESS_TOKEN}`
        ? authInfo || { scope: 'coord', resource: 'https://example.test/coord/' }
        : null;
    },
  };
}

async function createFixture(t, options = {}) {
  const app = express();
  const calls = [];
  const oauth = fakeOAuth(options.authInfo);
  const sidecar = {
    async call(rpc) {
      calls.push(rpc);
      if (options.onCall) return options.onCall(rpc);
      return options.sidecarResult || {
        tasks: [{
          id: TASK_ID,
          state: 'claimed',
          lease_token: LEASE_TOKEN,
          leaseToken: 'camel-secret',
          nested: { lease_token: 'nested-secret' },
        }],
      };
    },
  };
  const handle = await registerCoordMcp(app, {
    express,
    oauth,
    oauthModule: { installOAuthRoutes() {} },
    sidecar,
    skipOAuthInit: true,
    identityPolicy: options.identityPolicy,
  });
  const server = http.createServer(app);
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const address = server.address();
  const fixture = {
    app,
    calls,
    handle,
    oauth,
    server,
    sidecar,
    url: `http://127.0.0.1:${address.port}`,
    async close() {
      if (!server.listening) return;
      await new Promise((resolve) => {
        server.close(resolve);
        server.closeAllConnections();
      });
      fixtures.delete(fixture);
    },
  };
  fixtures.add(fixture);
  t.after(() => fixture.close());
  return fixture;
}

async function postMcp(fixture, payload, options = {}) {
  const headers = {
    accept: 'application/json, text/event-stream',
    'content-type': options.contentType || 'application/json',
    'mcp-protocol-version': '2025-03-26',
  };
  if (options.token !== null) headers.authorization = `Bearer ${options.token ?? ACCESS_TOKEN}`;
  const response = await fetch(`${fixture.url}/coord/mcp`, {
    method: 'POST',
    headers,
    body: typeof payload === 'string' ? payload : JSON.stringify(payload),
  });
  return { response, text: await response.text() };
}

function parseJsonResponse(text) {
  const body = text.trim();
  if (body.startsWith('data:')) return JSON.parse(body.slice(5).trim());
  const data = body.split(/\r?\n/)
    .filter((line) => line.startsWith('data:'))
    .map((line) => line.slice(5).trimStart())
    .join('\n');
  if (data) return JSON.parse(data);
  return JSON.parse(body);
}

function rpc(method, id, params = {}) {
  return { jsonrpc: '2.0', id, method, params };
}

test('OAuth facade publishes coord scope and only accepts coord bearer tokens', async () => {
  class BaseOAuthServer {
    constructor(config) {
      this.config = config;
    }

    metadata() {
      return { issuer: this.config.issuer, scopes_supported: ['desktop-commander', 'offline_access'] };
    }

    protectedResourceMetadata() {
      return { resource: this.config.resource, scopes_supported: ['desktop-commander', 'offline_access'] };
    }

    async registerClient() {
      return { client_id: 'client', scope: 'desktop-commander' };
    }

    async validateAuthorizationRequest(params) {
      this.authorizationScope = params.scope;
      return { scope: params.scope };
    }

    async issueTokens(_clientId, scope) {
      return { scope };
    }

    async validateAccessToken(_token, requiredScope) {
      this.requiredScope = requiredScope;
      return { scope: 'desktop-commander offline_access' };
    }
  }

  const oauth = createCoordOAuthServer(BaseOAuthServer, {
    issuer: 'https://example.test/app/CoordMCP',
    resource: 'https://example.test/app/CoordMCP/',
  });
  assert.deepEqual(oauth.metadata().scopes_supported, ['coord', 'offline_access']);
  assert.deepEqual(oauth.protectedResourceMetadata().scopes_supported, ['coord', 'offline_access']);
  assert.equal((await oauth.registerClient({})).scope, 'coord');
  await oauth.validateAuthorizationRequest({
    scope: 'coord offline_access',
    response_type: 'code',
    client_id: 'client',
    redirect_uri: 'http://127.0.0.1/callback',
    resource: 'https://example.test/app/CoordMCP/',
    code_challenge: 'A'.repeat(43),
    code_challenge_method: 'S256',
  });
  assert.equal(oauth.authorizationScope, 'desktop-commander offline_access');
  await assert.rejects(oauth.validateAuthorizationRequest({ scope: 'admin' }));
  assert.equal((await oauth.issueTokens('client', 'desktop-commander offline_access')).scope, 'coord offline_access');
  assert.equal((await oauth.validateAccessToken('opaque-token')).scope, 'coord offline_access');
  assert.equal(oauth.requiredScope, 'desktop-commander');
  assert.equal(await oauth.authenticateRequest({ headers: { authorization: 'Basic opaque-token' } }), null);
  assert.equal(await oauth.authenticateRequest({ headers: { authorization: 'Bearer opaque-token' } }).then(Boolean), true);
  assert.equal(await oauth.authenticateRequest({ headers: { 'cf-access-jwt-assertion': 'unsigned' } }), null);
});

test('strict tool allowlist maps only fixed coord RPCs and fences mutating inputs', () => {
  assert.equal(TOOLS.length, 10);
  assert.ok(TOOLS.every((tool) => tool.inputSchema.additionalProperties === false));
  assert.ok(!TOOLS.some((tool) => /create|cancel|batch_status/i.test(tool.name)));

  const list = validateToolInput('coord_tasks_list', { limit: 5 });
  assert.equal(list.method, 'tasks/list');
  assert.deepEqual(list.params, { limit: 5, skip_reclaim: true });
  assert.equal(validateToolInput('coord_get_task', { id: TASK_ID }).method, 'tasks/get');

  const claim = validateToolInput('coord_claim_task', { id: TASK_ID, request_id: TASK_ID });
  assert.deepEqual(claim.params, { id: TASK_ID, agentId: 'coord-relay-agent' });
  const next = validateToolInput('coord_claim_next', { batch_id: BATCH_ID, request_id: TASK_ID });
  assert.deepEqual(next.params, { agent_id: 'coord-relay-agent', batch_id: BATCH_ID });
  const extend = validateToolInput('coord_extend_lease', { id: TASK_ID, lease_token: LEASE_TOKEN, request_id: TASK_ID });
  assert.equal(extend.params.agentId, 'coord-relay-agent');
  const progress = validateToolInput('coord_report_progress', {
    id: TASK_ID, lease_token: LEASE_TOKEN, attempt_no: 2, sequence: 1,
    request_id: TASK_ID, checkpoint: 'Reviewed the first reference sheet.',
  });
  assert.equal(progress.method, 'scheduler/progress');
  assert.equal(progress.requestId, progress.params.request_id);
  assert.equal(progress.params.agent_id, 'coord-relay-agent');
  const progressExtend = validateToolInput('coord_extend_lease_progress', {
    id: TASK_ID, lease_token: LEASE_TOKEN, attempt_no: 2, progress_sequence: 1,
    request_id: TASK_ID,
  });
  assert.equal(progressExtend.method, 'scheduler/extend_progress');
  assert.equal(progressExtend.params.lease_seconds, 300);
  assert.equal(validateToolInput('coord_agent_status', { batch_id: BATCH_ID }).readOnly, true);
  const submit = validateToolInput('coord_submit_result', { id: TASK_ID, lease_token: LEASE_TOKEN, request_id: TASK_ID, result: { ok: true } });
  assert.equal(submit.method, 'tasks/complete');
  const failure = validateToolInput('coord_report_failure', { id: TASK_ID, lease_token: LEASE_TOKEN, request_id: TASK_ID, error: 'failed' });
  assert.equal(failure.method, 'scheduler/failure');

  assert.throws(() => validateToolInput('coord_tasks_list', { limit: 5, skip_reclaim: false }));
  assert.throws(() => validateToolInput('coord_claim_task', { id: TASK_ID, request_id: TASK_ID, agentId: 'attacker' }));
  assert.throws(() => validateToolInput('coord_claim_next', { request_id: TASK_ID }));
  assert.throws(() => validateToolInput('coord_claim_next', { batch_id: BATCH_ID, request_id: TASK_ID, agent_id: 'A0' }));
  assert.throws(() => validateToolInput('coord_submit_result', { id: TASK_ID, lease_token: LEASE_TOKEN, request_id: TASK_ID, result: {}, bypass_fence: true }));
  assert.throws(() => validateToolInput('coord_submit_result', { id: TASK_ID, lease_token: LEASE_TOKEN, request_id: TASK_ID, result: {}, agent_id: 'A1' }));
  assert.throws(() => validateToolInput('coord_report_failure', { id: TASK_ID, lease_token: LEASE_TOKEN, request_id: 'bad', error: 'failed' }));
  assert.throws(() => validateToolInput('coord_report_progress', { id: TASK_ID, lease_token: LEASE_TOKEN, attempt_no: 2, sequence: 1, request_id: TASK_ID, checkpoint: '   ' }));
  assert.throws(() => validateToolInput('coord_extend_lease_progress', { id: TASK_ID, lease_token: LEASE_TOKEN, attempt_no: 2, progress_sequence: 1, request_id: TASK_ID, lease_seconds: 601 }));
  assert.throws(() => validateToolInput('coord_agent_status', { batch_id: 'bad' }));
  assert.throws(() => validateToolInput('tasks/cancel', { id: TASK_ID }));
});

test('lease tokens are removed recursively from read responses in either JSON casing', () => {
  assert.deepEqual(removeLeaseTokens({
    lease_token: 'one',
    leaseToken: 'two',
    tasks: [{ lease_token: 'three', nested: { leaseToken: 'four', keep: true } }],
  }), { tasks: [{ nested: { keep: true } }] });
});

test('Streamable HTTP serves initialize, tools/list, and a token-redacted tasks/list', async (t) => {
  const fixture = await createFixture(t);
  const initialized = await postMcp(fixture, rpc('initialize', 1, {
    protocolVersion: '2025-03-26',
    capabilities: {},
    clientInfo: { name: 'coord-test', version: '1.0.0' },
  }));
  assert.equal(initialized.response.status, 200, initialized.text);
  const initResult = parseJsonResponse(initialized.text).result;
  assert.equal(initResult.serverInfo.name, 'coord-mcp');
  assert.ok(initResult.protocolVersion);

  const listed = await postMcp(fixture, rpc('tools/list', 2));
  assert.equal(listed.response.status, 200, listed.text);
  const toolResult = parseJsonResponse(listed.text).result.tools;
  assert.equal(toolResult.length, TOOLS.length);
  assert.ok(toolResult.some((tool) => tool.name === 'coord_tasks_list'));

  const called = await postMcp(fixture, rpc('tools/call', 3, {
    name: 'coord_tasks_list',
    arguments: { limit: 1 },
  }));
  assert.equal(called.response.status, 200, called.text);
  const callResult = parseJsonResponse(called.text).result;
  assert.equal(callResult.isError, undefined);
  const visible = callResult.content[0].text;
  assert.match(visible, new RegExp(TASK_ID));
  assert.doesNotMatch(visible, /lease_token|leaseToken|nested-secret|camel-secret/);
  assert.deepEqual(fixture.calls[0], {
    requestId: fixture.calls[0].requestId,
    method: 'tasks/list',
    params: { limit: 1, skip_reclaim: true },
  });
  assert.match(fixture.calls[0].requestId, /^[0-9a-f-]{36}$/i);
});

test('unauthorized, invalid content type, malformed and oversized HTTP requests are rejected', async (t) => {
  const fixture = await createFixture(t);
  const unauthorized = await postMcp(fixture, rpc('tools/list', 1), { token: null });
  assert.equal(unauthorized.response.status, 401);
  assert.match(unauthorized.response.headers.get('www-authenticate'), /oauth-protected-resource/);

  const invalidToken = await postMcp(fixture, rpc('tools/list', 2), { token: 'wrong-token' });
  assert.equal(invalidToken.response.status, 401);

  const wrongContentType = await postMcp(fixture, rpc('tools/list', 3), { contentType: 'text/plain' });
  assert.equal(wrongContentType.response.status, 415);

  const malformed = await postMcp(fixture, '{not-json');
  assert.equal(malformed.response.status, 400);

  const oversized = await postMcp(fixture, `{"filler":"${'x'.repeat(MAX_BODY_BYTES)}"}`);
  assert.equal(oversized.response.status, 413);
  assert.equal(fixture.calls.length, 0);
});

test('unknown tools and extra tool arguments never reach the sidecar', async (t) => {
  const fixture = await createFixture(t);
  const unknown = await postMcp(fixture, rpc('tools/call', 1, { name: 'coord_create_batch', arguments: {} }));
  const unknownPayload = parseJsonResponse(unknown.text);
  assert.ok(unknownPayload.error || unknownPayload.result?.isError);

  const extra = await postMcp(fixture, rpc('tools/call', 2, {
    name: 'coord_tasks_list',
    arguments: { limit: 1, method: 'tasks/cancel' },
  }));
  assert.equal(parseJsonResponse(extra.text).result.isError, true);
  assert.equal(fixture.calls.length, 0);
});

test('uncertain sidecar writes are MCP errors and are never retried', async (t) => {
  let dispatched = 0;
  const fixture = await createFixture(t, {
    onCall(rpcCall) {
      dispatched += 1;
      throw new RelayOutcomeError('uncertain', 'transport_outcome_unknown', rpcCall.requestId);
    },
  });
  const result = await postMcp(fixture, rpc('tools/call', 1, {
    name: 'coord_claim_task',
    arguments: { id: TASK_ID, request_id: crypto.randomUUID() },
  }));
  const callResult = parseJsonResponse(result.text).result;
  assert.equal(callResult.isError, true);
  assert.match(callResult.content[0].text, /Outcome uncertain/);
  assert.match(callResult.content[0].text, /request_id=/);
  assert.equal(dispatched, 1);
});

test('four active MCP calls are bounded and excess work receives 429', async (t) => {
  let inFlight = 0;
  let peak = 0;
  const fixture = await createFixture(t, {
    async onCall() {
      inFlight += 1;
      peak = Math.max(peak, inFlight);
      await new Promise((resolve) => setTimeout(resolve, 150));
      inFlight -= 1;
      return { ok: true };
    },
  });
  const requests = Array.from({ length: MAX_CONCURRENT + 1 }, (_, index) => postMcp(fixture, rpc('tools/call', index + 1, {
    name: 'coord_tasks_list',
    arguments: { limit: 1 },
  })));
  const responses = await Promise.all(requests);
  assert.equal(responses.filter(({ response }) => response.status === 200).length, MAX_CONCURRENT);
  assert.equal(responses.filter(({ response }) => response.status === 429).length, 1);
  assert.equal(peak, MAX_CONCURRENT);
});

async function createSidecarClient(fetchImpl, options = {}) {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'coord-mcp-test-'));
  tempDirs.add(dir);
  const secretFile = path.join(dir, 'client.secret');
  await fs.writeFile(secretFile, `${'a'.repeat(64)}\n`, { mode: 0o600 });
  const client = new SidecarClient({
    fetch: fetchImpl,
    secretFile,
    timeoutMs: options.timeoutMs,
    maxPending: options.maxPending,
  });
  await client.init();
  return { client, dir };
}

function sidecarRpc(params = {}) {
  return { requestId: crypto.randomUUID(), method: 'tasks/list', params };
}

test('SidecarClient marks timeouts and oversized replies uncertain without retrying', async () => {
  let timeoutCalls = 0;
  const timeoutClient = await createSidecarClient((_url, options) => new Promise((_resolve, reject) => {
    timeoutCalls += 1;
    options.signal.addEventListener('abort', () => reject(new Error('aborted')), { once: true });
  }), { timeoutMs: 20 });
  await assert.rejects(timeoutClient.client.call(sidecarRpc()), (error) => error.state === 'uncertain');
  assert.equal(timeoutCalls, 1);

  let responseCalls = 0;
  const oversizedClient = await createSidecarClient(async () => {
    responseCalls += 1;
    return new Response(JSON.stringify({ state: 'ok', result: 'x'.repeat(65 * 1024) }), { status: 200 });
  });
  await assert.rejects(oversizedClient.client.call(sidecarRpc()), (error) => error.state === 'uncertain' && error.code === 'response_too_large');
  assert.equal(responseCalls, 1);
});

test('SidecarClient caps in-flight calls and rejects oversized requests before dispatch', async () => {
  let release;
  let dispatched = 0;
  const client = await createSidecarClient(async () => {
    dispatched += 1;
    await new Promise((resolve) => { release = resolve; });
    return new Response(JSON.stringify({ state: 'ok', result: { ok: true } }), { status: 200 });
  }, { maxPending: 1 });
  const first = client.client.call(sidecarRpc());
  while (!release) await new Promise((resolve) => setImmediate(resolve));
  await assert.rejects(client.client.call(sidecarRpc()), (error) => error.state === 'not_dispatched' && error.code === 'concurrency_limit');
  release();
  assert.deepEqual(await first, { ok: true });
  assert.equal(dispatched, 1);

  await assert.rejects(client.client.call(sidecarRpc({ filler: 'x'.repeat(MAX_BODY_BYTES) })), (error) => error.state === 'not_dispatched' && error.code === 'request_too_large');
  assert.equal(dispatched, 1);
});

test('administrator-pinned OAuth binding supplies identity; MCP inputs cannot choose another agent or batch', async t => {
  const policy = fixturePolicy(BATCH_ID);
  const fixturesByAgent = await Promise.all([0, 1].map(i => createFixture(t, {
    authInfo: fixtureAuth(i), identityPolicy: policy,
    onCall: call => ({ claimed_by: call.params.agent_id, lease_token: LEASE_TOKEN }),
  })));
  for (let i = 0; i < fixturesByAgent.length; i++) {
    const f = fixturesByAgent[i];
    const request = rpc('tools/call', 1, { name: 'coord_claim_next', arguments: { batch_id: BATCH_ID, request_id: crypto.randomUUID() } });
    const accepted = parseJsonResponse((await postMcp(f, request)).text).result;
    assert.equal(accepted.isError, undefined);
    assert.equal(JSON.parse(accepted.content[0].text).claimed_by, `A${i}`);
    assert.equal(identity.verifyRequest(f.calls[0], policy).ok, true);
    const visible = accepted.content[0].text;
    assert.doesNotMatch(visible, /tokenSha256|proof|binding_id|clientId/);
    const count = f.calls.length;
    const wrongBatch = await postMcp(f, rpc('tools/call', 2, { name: 'coord_claim_next', arguments: { batch_id: TASK_ID, request_id: crypto.randomUUID() } }));
    assert.equal(parseJsonResponse(wrongBatch.text).result.isError, true);
    const wrongAgent = await postMcp(f, rpc('tools/call', 3, { name: 'coord_claim_next', arguments: { batch_id: BATCH_ID, request_id: crypto.randomUUID(), agent_id: `A${1-i}` } }));
    assert.equal(parseJsonResponse(wrongAgent.text).result.isError, true);
    const globalList = await postMcp(f, rpc('tools/call', 4, { name: 'coord_tasks_list', arguments: {} }));
    assert.equal(parseJsonResponse(globalList.text).result.isError, true);
    assert.equal(f.calls.length, count);
  }
});

test('enabled identity policy rejects unknown, expired and cross-token OAuth contexts while preserving pinned legacy', async t => {
  const policy = fixturePolicy(BATCH_ID);
  for (const authInfo of [{ clientId: 'unknown' }, { ...fixtureAuth(0), tokenSha256: identity.hash('token-1') }, { ...fixtureAuth(0), expiresAt: Date.now() - 1 }]) {
    const f = await createFixture(t, { authInfo, identityPolicy: policy });
    const response = await postMcp(f, rpc('tools/list', 1));
    assert.equal(response.response.status, 403);
    assert.equal(f.calls.length, 0);
  }
  const legacy = await createFixture(t, { authInfo: { clientId: 'legacy-client' }, identityPolicy: policy });
  const response = await postMcp(legacy, rpc('tools/call', 1, { name: 'coord_claim_task', arguments: { id: TASK_ID, request_id: crypto.randomUUID() } }));
  assert.equal(parseJsonResponse(response.text).result.isError, undefined);
  assert.equal(legacy.calls[0].params.agentId, 'coord-relay-agent');
  assert.equal(legacy.calls[0].identity, undefined);
});

test('logical worker tools require OAuth, bind explicit worker and batch, and preserve legacy calls', async t => {
  const time = Date.now();
  const authInfo = { clientId: 'logical-owner-fixture', expiresAt: time + 300000 };
  const policy = identity.validatePolicy({ version: 2, mode: 'logical', enabled: true,
    signing_key_file: path.resolve('synthetic-key'), legacy_client_sha256: [identity.hash(authInfo.clientId)],
    bindings: [{ id: 'logical-test', client_sha256: identity.hash(authInfo.clientId), worker_ids: ['0000', '0001'], batch_id: BATCH_ID,
      scopes: ['coord:read', 'coord:claim'], not_before: time - 1000, expires_at: time + 300000 }] }, 'cd'.repeat(32));
  const f = await createFixture(t, { authInfo, identityPolicy: policy,
    onCall: call => ({ claimed_by: call.params.agent_id || call.params.agentId, lease_token: LEASE_TOKEN }) });
  const tool = (worker, batch = BATCH_ID) => rpc('tools/call', 1, { name: 'coord_claim_next', arguments: { worker_id: worker, batch_id: batch, request_id: crypto.randomUUID() } });
  assert.equal((await postMcp(f, tool('0000'), { token: null })).response.status, 401);
  assert.equal(f.calls.length, 0);
  for (const worker of ['0000', '0001']) {
    const accepted = parseJsonResponse((await postMcp(f, tool(worker))).text).result;
    assert.equal(accepted.isError, undefined);
    assert.equal(JSON.parse(accepted.content[0].text).claimed_by, `emote-${worker}`);
    assert.equal(identity.verifyRequest(f.calls.at(-1), policy).ok, true);
  }
  const count = f.calls.length;
  for (const request of [tool('0002'), tool('0000', TASK_ID), tool('GGGG')]) {
    assert.equal(parseJsonResponse((await postMcp(f, request)).text).result.isError, true);
  }
  assert.equal(f.calls.length, count);
  const legacy = parseJsonResponse((await postMcp(f, rpc('tools/call', 2, { name: 'coord_claim_task', arguments: { id: TASK_ID, request_id: crypto.randomUUID() } }))).text).result;
  assert.equal(legacy.isError, undefined);
  assert.equal(f.calls.at(-1).identity, undefined);
  assert.equal(f.calls.at(-1).params.agentId, 'coord-relay-agent');
});

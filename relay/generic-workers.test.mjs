import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { createServer } from 'node:http';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import { test } from 'node:test';
import express from './coord-mcp/node_modules/express/index.js';
import { Client } from './coord-mcp/node_modules/@modelcontextprotocol/sdk/dist/esm/client/index.js';
import { StreamableHTTPClientTransport } from './coord-mcp/node_modules/@modelcontextprotocol/sdk/dist/esm/client/streamableHttp.js';
import { createSidecar } from './server.mjs';
import { createAgent } from './agent.mjs';
import { reservePort } from './test-ports.mjs';
import { validateRpcRequest } from './protocol.mjs';
import identity from './identity.cjs';
import mcp from './coord-mcp/index.cjs';

test('independent HTTP MCP client runs generic workers across signed Relay and Rust with scope isolation', { timeout: 40_000 }, async () => {
  const folder = await mkdtemp(join(tmpdir(), 'coord-generic-http-'));
  const coordPort = await reservePort();
  const coordUrl = `http://127.0.0.1:${coordPort}/`;
  let daemon;
  const start = () => {
    daemon = spawn(resolve(process.env.COORD_TEST_BIN || 'target/release/coord.exe'),
      ['serve', '--addr', `127.0.0.1:${coordPort}`, '--db', join(folder, 'state.db')], { stdio: 'ignore' });
    daemon.on('error', () => {});
  };
  const direct = async (method, params) => {
    const response = await fetch(coordUrl, { method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', id: randomUUID(), method, params }) });
    const value = await response.json();
    assert.equal(value.error, undefined);
    return value.result;
  };
  const ready = async () => {
    for (let index = 0; index < 100; index++) {
      try { await direct('tasks/list', { skip_reclaim: true }); return; } catch { await delay(30); }
    }
    assert.fail('isolated Core did not start');
  };
  let sidecar; let agent; let httpServer; let client;
  try {
    start(); await ready();
    const batches = [];
    for (const name of ['allowed-a', 'allowed-b', 'foreign']) {
      const batch = await direct('batches/create', { name, config: { max_attempts: 2, retry_backoff_ms: 50, require_progress: true } });
      batches.push(batch.id);
      await direct('batches/add_tasks', { batch_id: batch.id, tasks: Array.from({ length: 3 }, (_, index) => ({ key: `${index}`, name: `${name}-${index}` })) });
    }
    const now = Date.now();
    const auth = { clientId: 'generic-test-owner', expiresAt: now + 600_000 };
    const policy = identity.validatePolicy({ version: 2, mode: 'logical', enabled: true,
      signing_key_file: fileURLToPath(new URL('./synthetic-key', import.meta.url)),
      legacy_client_sha256: [identity.hash('legacy-test-owner')],
      bindings: batches.slice(0, 2).map((batch_id, index) => ({ id: `allowed-${index}`, client_sha256: identity.hash(auth.clientId),
        worker_ids: index === 0 ? ['writer-1', 'writer-2', 'ABCD'] : ['writer-1'], batch_id,
        scopes: ['coord:read', 'coord:claim', 'coord:progress', 'coord:submit', 'coord:failure', 'coord:release'],
        not_before: now - 1000, expires_at: now + 600_000 })) }, 'ac'.repeat(32));
    sidecar = await createSidecar({ agentHost: '127.0.0.1', agentPort: 0, clientPort: 0, allowedPeerIp: '127.0.0.1',
      agentSecret: '12'.repeat(32), clientSecret: '34'.repeat(32), identityPolicy: policy, journalFile: join(folder, 'requests.ndjson') });
    const addresses = await sidecar.start();
    agent = await createAgent({ serverHost: '127.0.0.1', serverPort: addresses.agentAddress.port, coordUrl,
      secret: '12'.repeat(32), identityPolicy: policy, heartbeatMs: 100 });
    agent.start();
    assert.equal(await agent.waitUntilConnected(3000), true);
    const app = express();
    const secretFile = join(folder, 'client.secret');
    await writeFile(secretFile, '34'.repeat(32));
    await mcp.registerCoordMcp(app, { express, identityPolicy: policy, skipOAuthInit: true,
      oauthModule: { installOAuthRoutes() {} },
      oauth: { async init() {}, setCors() {}, challenge(response) { response.status(401).end(); },
        async authenticateRequest(request) { return request.get('authorization') === 'Bearer fixture-owner' ? auth : null; } },
      sidecar: new mcp.SidecarClient({ url: `http://127.0.0.1:${addresses.clientAddress.port}/v1/rpc`, secretFile }) });
    httpServer = createServer(app);
    await new Promise(done => httpServer.listen(0, '127.0.0.1', done));
    client = new Client({ name: 'independent-generic-acceptance', version: '1' });
    await client.connect(new StreamableHTTPClientTransport(new URL(`http://127.0.0.1:${httpServer.address().port}/coord/mcp`),
      { requestInit: { headers: { authorization: 'Bearer fixture-owner' } } }));
    const tools = (await client.listTools()).tools;
    assert.ok(tools.some(tool => tool.name === 'coord_fail'));
    assert.ok(tools.some(tool => tool.name === 'coord_report_failure'));
    assert.equal(tools.find(tool => tool.name === 'coord_tasks_list').annotations.readOnlyHint, true);
    const tool = async (name, args, rejected = false) => {
      const response = await client.callTool({ name, arguments: args });
      assert.equal(Boolean(response.isError), rejected, JSON.stringify(response));
      return rejected ? response : JSON.parse(response.content[0].text);
    };
    const scope = { worker_id: 'writer-1', batch_id: batches[0] };
    for (const batch_id of batches.slice(0, 2)) {
      const tasks = await tool('coord_tasks_list', { ...scope, batch_id });
      assert.equal(tasks.length, 3);
      assert.ok(tasks.every(task => task.batch_id === batch_id && !Object.hasOwn(task, 'lease_token')));
    }
    await tool('coord_tasks_list', {}, true);
    await tool('coord_tasks_list', { worker_id: 'writer-1' }, true);
    await tool('coord_tasks_list', { ...scope, batch_id: batches[2] }, true);
    await tool('coord_tasks_list', { worker_id: 'writer-2', batch_id: batches[1] }, true);
    await tool('coord_tasks_list', { ...scope, agent_id: 'admin' }, true);
    const principal = identity.resolvePrincipal(policy, auth, Date.now(), scope);
    const forged = identity.signRequest({ requestId: randomUUID(), method: 'tasks/list', params: { batch_id: batches[2], skip_reclaim: true } }, principal, policy);
    assert.equal(validateRpcRequest(forged, policy).ok, false);
    const [first, second] = await Promise.all([
      tool('coord_claim', scope), tool('coord_claim', { ...scope, worker_id: 'writer-2' }),
    ]);
    assert.notEqual(first.id, second.id);
    assert.equal(first.claimed_by, 'writer-1');
    assert.equal((await tool('coord_claim', scope)).id, first.id);
    assert.ok(!Object.hasOwn(first, 'lease_token'));
    const attempt = { ...scope, task_id: first.id, attempt_no: first.attempt_no };
    await tool('coord_progress', { ...attempt, checkpoint: 'saved durable draft' });
    assert.equal((await tool('coord_progress', { ...attempt, checkpoint: 'saved durable draft' })).replayed, true);
    const stopped = new Promise(done => daemon.once('exit', done)); daemon.kill(); await stopped;
    start(); await ready();
    const resumed = await tool('coord_resume', scope);
    assert.equal(resumed.task.id, first.id);
    assert.equal(resumed.checkpoint, 'saved durable draft');
    await tool('coord_fail', { ...attempt, worker_id: 'writer-2', error: 'wrong owner' }, true);
    await tool('coord_fail', { ...attempt, batch_id: batches[1], error: 'wrong batch' }, true);
    await tool('coord_fail', { ...attempt, attempt_no: first.attempt_no + 1, error: 'wrong attempt' }, true);
    assert.equal((await tool('coord_fail', { ...attempt, error: 'invalid input' })).failure_count, 1);
    assert.equal((await tool('coord_fail', { ...attempt, error: 'invalid input' })).replayed, true);
    await tool('coord_fail', { ...attempt, error: 'changed failure' }, true);
    await tool('coord_release', { ...scope, worker_id: 'writer-2', task_id: second.id, attempt_no: second.attempt_no });
    assert.equal((await direct('tasks/get', { id: second.id })).failure_count, 0);
    await delay(80);
    const recovered = await tool('coord_claim', scope);
    assert.ok(recovered.attempt_no > first.attempt_no);
    await tool('coord_complete', { ...attempt, result: { stale: true } }, true);
    const completion = { ...scope, task_id: recovered.id, attempt_no: recovered.attempt_no, result: { text: 'generic accepted' } };
    assert.equal((await tool('coord_complete', completion)).state, 'completed');
    assert.equal((await tool('coord_complete', completion)).replayed, true);
    assert.equal((await tool('coord_resume', scope)).state, 'completed');
    const temporary = await tool('coord_claim', { ...scope, batch_id: batches[1], lease_seconds: 1 });
    await delay(1100);
    const before = await direct('tasks/get', { id: temporary.id });
    await tool('coord_tasks_list', { ...scope, batch_id: batches[1] });
    await tool('coord_status', { ...scope, batch_id: batches[1] });
    assert.deepEqual(await direct('tasks/get', { id: temporary.id }), before);
    const deniedPrincipal = { ...principal, scopes: ['coord:read'] };
    assert.equal(identity.verifyRequest(identity.signRequest({ requestId: randomUUID(), method: 'scheduler/trusted_fail',
      params: { ...attempt, error: 'unauthorized' } }, deniedPrincipal, policy), policy).ok, false);
    assert.equal((await direct('batches/status', { batch_id: batches[2] })).states.pending, 3);
  } finally {
    await client?.close();
    if (httpServer) await new Promise(done => { httpServer.close(done); httpServer.closeAllConnections(); });
    await agent?.stop(); await sidecar?.stop();
    if (daemon && daemon.exitCode === null) { const exited = new Promise(done => daemon.once('exit', done)); daemon.kill(); await exited; }
    await rm(folder, { recursive: true, force: true });
  }
});

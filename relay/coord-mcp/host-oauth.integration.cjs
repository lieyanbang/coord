const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs/promises');
const http = require('node:http');
const os = require('node:os');
const path = require('node:path');
const { registerCoordMcp, SidecarClient } = require('./index.cjs');

const TASK_ID = '123e4567-e89b-42d3-a456-426614174000';
const PUBLIC_ISSUER = 'https://lieyanbang.com/app/CoordMCP';
const PUBLIC_RESOURCE = `${PUBLIC_ISSUER}/`;

function requiredEnvironment(name) {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is required.`);
  return value;
}

function parseSseOrJson(text) {
  const body = text.trim();
  if (!body.startsWith('event:')) return JSON.parse(body);
  const data = body.split(/\r?\n/)
    .filter((line) => line.startsWith('data:'))
    .map((line) => line.slice(5).trimStart())
    .join('\n');
  return JSON.parse(data);
}

function findValue(value, predicate) {
  if (Array.isArray(value)) {
    for (const item of value) {
      const found = findValue(item, predicate);
      if (found !== undefined) return found;
    }
    return undefined;
  }
  if (!value || typeof value !== 'object') return undefined;
  if (predicate(value)) return value;
  for (const item of Object.values(value)) {
    const found = findValue(item, predicate);
    if (found !== undefined) return found;
  }
  return undefined;
}

function findLeaseToken(value) {
  if (Array.isArray(value)) {
    for (const item of value) {
      const found = findLeaseToken(item);
      if (found) return found;
    }
    return undefined;
  }
  if (!value || typeof value !== 'object') return undefined;
  if (typeof value.lease_token === 'string') return value.lease_token;
  if (typeof value.leaseToken === 'string') return value.leaseToken;
  for (const item of Object.values(value)) {
    const found = findLeaseToken(item);
    if (found) return found;
  }
  return undefined;
}

async function postForm(url, values, origin, headers = {}) {
  return fetch(url, {
    method: 'POST',
    headers: {
      'content-type': 'application/x-www-form-urlencoded',
      ...(origin ? { origin } : {}),
      ...headers,
    },
    body: new URLSearchParams(values),
    redirect: 'manual',
  });
}

function signAccessAssertion(privateKey, claims, kid = 'coord-test-access-key') {
  const encode = (value) => Buffer.from(JSON.stringify(value)).toString('base64url');
  const header = encode({ alg: 'RS256', typ: 'JWT', kid });
  const payload = encode(claims);
  const signer = crypto.createSign('RSA-SHA256');
  signer.update(`${header}.${payload}`);
  signer.end();
  return `${header}.${payload}.${signer.sign(privateKey).toString('base64url')}`;
}

async function mcpPost(url, token, message) {
  return fetch(url, {
    method: 'POST',
    headers: {
      accept: 'application/json, text/event-stream',
      authorization: `Bearer ${token}`,
      'content-type': 'application/json',
      'mcp-protocol-version': '2025-03-26',
    },
    body: JSON.stringify(message),
  });
}

async function main() {
  const originalSalt = process.env.DC_OAUTH_OWNER_PASSWORD_SALT;
  const originalHash = process.env.DC_OAUTH_OWNER_PASSWORD_HASH;
  const accessTokenTtlSeconds = Number(process.env.COORD_MCP_ACCESS_TOKEN_TTL_SECONDS || 2);
  if (!Number.isInteger(accessTokenTtlSeconds) || accessTokenTtlSeconds < 1 || accessTokenTtlSeconds > 3600) {
    throw new Error('COORD_MCP_ACCESS_TOKEN_TTL_SECONDS must be an integer from 1 to 3600.');
  }
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), 'coord-mcp-host-oauth-'));
  const { privateKey, publicKey } = crypto.generateKeyPairSync('rsa', { modulusLength: 2048 });
  const testAccessJwk = { ...publicKey.export({ format: 'jwk' }), kid: 'coord-test-access-key', kty: 'RSA' };
  const app = require(requiredEnvironment('COORD_MCP_EXPRESS_MODULE'))();
  const oauthModule = require(requiredEnvironment('COORD_MCP_OAUTH_MODULE'));
  const sdkResolutionBase = requiredEnvironment('COORD_MCP_SDK_RESOLUTION_BASE');
  const server = http.createServer(app);
  let registered;

  try {
    const salt = crypto.randomBytes(16);
    const password = crypto.randomBytes(24).toString('base64url');
    process.env.DC_OAUTH_OWNER_PASSWORD_SALT = salt.toString('base64url');
    process.env.DC_OAUTH_OWNER_PASSWORD_HASH = crypto.scryptSync(password, salt, 32).toString('base64url');

    await new Promise((resolve, reject) => {
      server.once('error', reject);
      server.listen(0, '127.0.0.1', resolve);
    });
    const address = server.address();
    const origin = `http://127.0.0.1:${address.port}`;
    const appEndpoint = `${origin}/coord/mcp`;
    const oauthEndpoint = `${origin}/coord/oauth`;
    const sidecarCalls = [];
    const useLiveSidecar = process.env.COORD_MCP_USE_LIVE_SIDECAR === '1';
    const liveSidecar = useLiveSidecar ? new SidecarClient() : null;
    const sidecar = {
      async init() {
        await liveSidecar?.init();
      },
      async call(rpc) {
        sidecarCalls.push(rpc);
        if (liveSidecar) return liveSidecar.call(rpc);
        if (rpc.method === 'scheduler/claim') {
          return { empty: false, task: { id: TASK_ID, state: 'claimed', lease_token: 'test-lease-token' } };
        }
        if (rpc.method === 'tasks/complete') {
          return { id: TASK_ID, state: 'completed', result: rpc.params.result };
        }
        if (rpc.method === 'tasks/get') {
          return { id: TASK_ID, state: 'completed', result: { f3_acceptance: 'PASS' } };
        }
        return { tasks: [{ id: TASK_ID, state: 'pending', lease_token: 'never-return-this-on-read' }] };
      },
    };

    registered = await registerCoordMcp(app, {
      express: require(requiredEnvironment('COORD_MCP_EXPRESS_MODULE')),
      oauthModule,
      issuer: PUBLIC_ISSUER,
      authorizationOrigin: origin,
      accessTeamDomain: 'https://coord-test.cloudflareaccess.com',
      accessApplicationAud: 'coord-test-access-aud',
      accessAllowedEmails: ['coord-owner@example.test'],
      accessJwksProvider: async () => [testAccessJwk],
      oauthStatePath: path.join(tempDir, 'oauth.json'),
      oauthDiagnosticLogPath: path.join(tempDir, 'oauth-audit.jsonl'),
      oauthAccessTokenTtlSeconds: accessTokenTtlSeconds,
      sidecar,
      sdkResolutionBase,
    });

    const stateMode = (await fs.stat(tempDir)).mode & 0o777;
    if (process.platform !== 'win32') assert.equal(stateMode, 0o700);

    const metadata = await (await fetch(`${origin}/.well-known/oauth-authorization-server/coord`)).json();
    assert.equal(metadata.issuer, PUBLIC_ISSUER);
    assert.equal(metadata.registration_endpoint, `${PUBLIC_ISSUER}/oauth/register`);
    assert.deepEqual(metadata.scopes_supported, ['coord', 'offline_access']);
    const resourceMetadata = await (await fetch(`${origin}/.well-known/oauth-protected-resource/coord`)).json();
    assert.equal(resourceMetadata.resource, PUBLIC_RESOURCE);

    const registration = await fetch(`${oauthEndpoint}/register`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        client_name: 'isolated coord integration test',
        redirect_uris: ['http://127.0.0.1:45454/callback'],
        token_endpoint_auth_method: 'none',
      }),
    });
    assert.equal(registration.status, 201);
    const client = await registration.json();
    assert.equal(client.scope, 'coord');

    const verifier = crypto.randomBytes(32).toString('base64url');
    const challenge = crypto.createHash('sha256').update(verifier, 'ascii').digest('base64url');
    const authorizeParams = {
      response_type: 'code',
      client_id: client.client_id,
      redirect_uri: client.redirect_uris[0],
      scope: 'coord',
      resource: PUBLIC_RESOURCE,
      code_challenge: challenge,
      code_challenge_method: 'S256',
      state: crypto.randomUUID(),
    };
    const authorizeUrl = `${oauthEndpoint}/authorize?${new URLSearchParams(authorizeParams)}`;
    const accessAssertion = signAccessAssertion(privateKey, {
      iss: 'https://coord-test.cloudflareaccess.com',
      aud: ['coord-test-access-aud'],
      sub: 'coord-test-owner',
      email: 'coord-owner@example.test',
      exp: Math.floor(Date.now() / 1000) + 600,
      nbf: Math.floor(Date.now() / 1000) - 5,
    });
    const noAccess = await fetch(authorizeUrl, { redirect: 'manual' });
    assert.equal(noAccess.status, 403);
    const authorizeHeaders = { 'cf-access-jwt-assertion': accessAssertion };
    const authorizePage = await fetch(authorizeUrl, { headers: authorizeHeaders, redirect: 'manual' });
    assert.equal(authorizePage.status, 200);
    const authorizeHtml = await authorizePage.text();
    assert.match(authorizeHtml, /Authorize Coord MCP/);
    assert.doesNotMatch(authorizeHtml, /password|type="password"/i);
    const csrfToken = authorizeHtml.match(/name="csrf_token" value="([A-Za-z0-9_-]{43})"/)?.[1];
    assert.ok(csrfToken);

    const wrongScope = await fetch(`${oauthEndpoint}/authorize?${new URLSearchParams({ ...authorizeParams, scope: 'admin' })}`, {
      headers: authorizeHeaders,
      redirect: 'manual',
    });
    assert.equal(wrongScope.status, 302);
    assert.equal(new URL(wrongScope.headers.get('location')).searchParams.get('error'), 'invalid_request');

    const authorization = await postForm(`${oauthEndpoint}/authorize`, { ...authorizeParams, csrf_token: csrfToken }, origin, {
      ...authorizeHeaders,
    });
    assert.equal(authorization.status, 302, await authorization.clone().text());
    const authorizationRedirect = new URL(authorization.headers.get('location'));
    const code = authorizationRedirect.searchParams.get('code');
    assert.ok(code);

    const tokenRequest = {
      grant_type: 'authorization_code',
      client_id: client.client_id,
      code,
      redirect_uri: client.redirect_uris[0],
      code_verifier: verifier,
      resource: PUBLIC_RESOURCE,
    };
    const wrongResource = await postForm(`${oauthEndpoint}/token`, { ...tokenRequest, resource: `${PUBLIC_ISSUER}/other` });
    assert.equal(wrongResource.status, 400);
    const wrongPkce = await postForm(`${oauthEndpoint}/token`, { ...tokenRequest, code_verifier: `${verifier.slice(0, -1)}x` });
    assert.equal(wrongPkce.status, 400);
    const tokenExchanges = await Promise.all([
      postForm(`${oauthEndpoint}/token`, tokenRequest),
      postForm(`${oauthEndpoint}/token`, tokenRequest),
    ]);
    assert.deepEqual(tokenExchanges.map((response) => response.status).sort(), [200, 400]);
    const tokenResponse = tokenExchanges.find((response) => response.status === 200);
    const tokens = await tokenResponse.json();
    assert.equal(tokens.scope, 'coord');
    assert.equal(tokens.resource, PUBLIC_RESOURCE);
    assert.equal(tokens.token_type, 'Bearer');

    const unauthenticated = await fetch(appEndpoint, {
      method: 'POST',
      headers: { accept: 'application/json, text/event-stream', 'content-type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'initialize', params: {} }),
    });
    assert.equal(unauthenticated.status, 401);
    assert.match(unauthenticated.headers.get('www-authenticate'), /oauth-protected-resource\/mcp/);

    const initialized = await mcpPost(appEndpoint, tokens.access_token, {
      jsonrpc: '2.0',
      id: 2,
      method: 'initialize',
      params: { protocolVersion: '2025-03-26', capabilities: {}, clientInfo: { name: 'coord-test', version: '1' } },
    });
    assert.equal(initialized.status, 200);
    assert.equal(parseSseOrJson(await initialized.text()).result.serverInfo.name, 'coord-mcp');

    const tools = await mcpPost(appEndpoint, tokens.access_token, { jsonrpc: '2.0', id: 3, method: 'tools/list', params: {} });
    assert.equal(tools.status, 200);
    assert.equal(parseSseOrJson(await tools.text()).result.tools.length, 7);

    const tasks = await mcpPost(appEndpoint, tokens.access_token, {
      jsonrpc: '2.0',
      id: 4,
      method: 'tools/call',
      params: { name: 'coord_tasks_list', arguments: { limit: 1 } },
    });
    assert.equal(tasks.status, 200);
    const taskResult = parseSseOrJson(await tasks.text()).result.content[0].text;
    const taskData = JSON.parse(taskResult);
    assert.ok(taskData && typeof taskData === 'object');
    const arrayCounts = Array.isArray(taskData)
      ? [taskData.length]
      : Object.values(taskData).filter(Array.isArray).map((items) => items.length);
    if (!useLiveSidecar) assert.match(taskResult, new RegExp(TASK_ID));
    assert.doesNotMatch(taskResult, /lease_token|leaseToken/);
    assert.equal(sidecarCalls.length, 1);
    assert.equal(sidecarCalls[0].method, 'tasks/list');
    assert.deepEqual(sidecarCalls[0].params, { limit: 1, skip_reclaim: true });

    const refresh = await postForm(`${oauthEndpoint}/token`, {
      grant_type: 'refresh_token',
      client_id: client.client_id,
      refresh_token: tokens.refresh_token,
      resource: PUBLIC_RESOURCE,
    });
    assert.equal(refresh.status, 200);
    const refreshed = await refresh.json();
    assert.equal(refreshed.scope, 'coord');

    const sidecarLabel = useLiveSidecar ? 'live Sidecar to coord tasks/list' : 'isolated sidecar stub';
    const shapeLabel = useLiveSidecar
      ? `, result_type=${Array.isArray(taskData) ? 'array' : 'object'}, array_lengths=${arrayCounts.join(',') || 'none'}`
      : '';
    let writeLabel = '';
    const batchId = process.env.COORD_MCP_TEST_BATCH_ID;
    const taskId = process.env.COORD_MCP_TEST_TASK_ID;
    if (Boolean(batchId) !== Boolean(taskId)) throw new Error('Both COORD_MCP_TEST_BATCH_ID and COORD_MCP_TEST_TASK_ID are required for the write probe.');
    if (batchId && taskId) {
      const callTool = async (id, name, args) => {
        const response = await mcpPost(appEndpoint, refreshed.access_token, {
          jsonrpc: '2.0',
          id,
          method: 'tools/call',
          params: { name, arguments: args },
        });
        assert.equal(response.status, 200);
        const result = parseSseOrJson(await response.text()).result;
        assert.ok(result && result.isError !== true, result?.content?.[0]?.text || `MCP tool ${name} failed.`);
        return result;
      };
      const claim = await callTool(6, 'coord_claim_next', {
        batch_id: batchId,
        request_id: crypto.randomUUID(),
        lease_seconds: 60,
      });
      const claimed = JSON.parse(claim.content[0].text);
      const task = findValue(claimed, (value) => value.id === taskId);
      assert.ok(task, 'The isolated batch claim did not return the requested task.');
      const leaseToken = findLeaseToken(task);
      assert.ok(leaseToken, 'The isolated task claim did not include its fence token.');

      const completionResult = { f3_acceptance: 'PASS', batch_id: batchId };
      await callTool(7, 'coord_submit_result', {
        id: taskId,
        lease_token: leaseToken,
        request_id: crypto.randomUUID(),
        result: completionResult,
        post_ack: false,
      });
      const readback = await callTool(8, 'coord_get_task', { id: taskId });
      const finalTask = findValue(JSON.parse(readback.content[0].text), (value) => value.id === taskId);
      assert.ok(finalTask, 'The isolated task read-back did not return the task.');
      assert.equal(finalTask.state, 'completed');
      assert.deepEqual(finalTask.result, completionResult);
      assert.doesNotMatch(readback.content[0].text, /lease_token|leaseToken/);
      writeLabel = `, isolated_batch_write=PASS, batch_id=${batchId}, task_id=${taskId}`;
    }

    const sidecarCallsBeforeExpiry = sidecarCalls.length;
    await new Promise((resolve) => setTimeout(resolve, (accessTokenTtlSeconds * 1000) + 200));
    const expired = await mcpPost(appEndpoint, refreshed.access_token, { jsonrpc: '2.0', id: 9, method: 'tools/list', params: {} });
    assert.equal(expired.status, 401);
    assert.match(expired.headers.get('www-authenticate'), /invalid_token/);
    assert.equal(sidecarCalls.length, sidecarCallsBeforeExpiry);

    process.stdout.write(`host OAuth and MCP integration: PASS (Cloudflare Access RS256/JWKS owner validation, password-free consent, CSRF binding, one-time code, DCR, S256 PKCE, resource binding, scope mapping, refresh, expiry, initialize, tools/list, ${sidecarLabel}${shapeLabel})\n`);
    if (writeLabel) process.stdout.write(`production batch tool round-trip: PASS${writeLabel}\n`);
  } finally {
    await registered?.shutdown();
    if (server.listening) {
      await new Promise((resolve) => {
        server.close(resolve);
        server.closeAllConnections();
      });
    }
    await fs.rm(tempDir, { recursive: true, force: true });
    if (originalSalt === undefined) delete process.env.DC_OAUTH_OWNER_PASSWORD_SALT;
    else process.env.DC_OAUTH_OWNER_PASSWORD_SALT = originalSalt;
    if (originalHash === undefined) delete process.env.DC_OAUTH_OWNER_PASSWORD_HASH;
    else process.env.DC_OAUTH_OWNER_PASSWORD_HASH = originalHash;
  }
}

main().catch((error) => {
  process.stderr.write(`${error.stack || error.message}\n`);
  process.exitCode = 1;
});

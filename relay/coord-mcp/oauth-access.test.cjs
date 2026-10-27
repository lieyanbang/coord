const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs/promises');
const http = require('node:http');
const os = require('node:os');
const path = require('node:path');
const { after, test } = require('node:test');
const express = require('express');
const { registerCoordMcp } = require('./index.cjs');
const { createCoordOAuthServer } = require('./oauth.cjs');

const TEAM_DOMAIN = 'https://coord-test.cloudflareaccess.com';
const ACCESS_AUD = 'coord-test-access-aud';
const OWNER_EMAIL = 'owner@example.test';
const CLIENT_ID = 'test-client';
const REDIRECT_URI = 'http://127.0.0.1:45454/callback';
const RESOURCE = 'https://lieyanbang.com/app/CoordMCP/';
const ISSUER = 'https://lieyanbang.com/app/CoordMCP';

const servers = new Set();
const tempDirs = new Set();

after(async () => {
  await Promise.all([...servers].map((server) => new Promise((resolve) => {
    if (!server.listening) return resolve();
    server.close(resolve);
    server.closeAllConnections();
  })));
  await Promise.all([...tempDirs].map((dir) => fs.rm(dir, { recursive: true, force: true })));
});

class MemoryStore {
  constructor() {
    this.state = {
      clients: {
        [CLIENT_ID]: {
          client_id: CLIENT_ID,
          client_name: 'Coord test client',
          redirect_uris: [REDIRECT_URI],
          token_endpoint_auth_method: 'none',
        },
      },
      codes: {},
      accessTokens: {},
      refreshTokens: {},
    };
    this.queue = Promise.resolve();
  }

  async init() {}

  async read() {
    await this.queue;
    return this.state;
  }

  async update(mutator) {
    const operation = this.queue.then(async () => {
      const result = await mutator(this.state);
      return result;
    });
    this.queue = operation.catch(() => undefined);
    return operation;
  }
}

class TestOAuthServer {
  constructor(config, options = {}) {
    this.config = config;
    this.store = options.store || new MemoryStore();
    this.oauthRoutePrefix = options.oauthRoutePrefix || '/coord/oauth';
    this.authorizationFormActionPrefix = options.authorizationFormActionPrefix || this.oauthRoutePrefix;
    this.protectedResourceMetadataUrl = options.protectedResourceMetadataUrl || `${config.issuer}/.well-known/oauth-protected-resource/mcp`;
    this.accessTeamDomain = config.accessTeamDomain || '';
    this.accessApplicationAud = config.accessApplicationAud || '';
    this.accessJwksProvider = options.accessJwksProvider || (async () => []);
  }

  async init() { await this.store.init(); }

  auditRequest(req, res, route) {
    this.auditRecords ||= [];
    res.once('finish', () => {
      this.auditRecords.push({ request: { ...req, route }, response: { statusCode: res.statusCode } });
    });
  }

  metadata() {
    return {
      issuer: this.config.issuer,
      authorization_endpoint: `${this.config.issuer}/oauth/authorize`,
      token_endpoint: `${this.config.issuer}/oauth/token`,
      registration_endpoint: `${this.config.issuer}/oauth/register`,
      scopes_supported: ['desktop-commander', 'offline_access'],
    };
  }

  protectedResourceMetadata() {
    return { resource: this.config.resource, authorization_servers: [this.config.issuer] };
  }

  setCors(res) {
    res.setHeader('Access-Control-Allow-Origin', '*');
  }

  sendJson(res, status, body) {
    this.setCors(res);
    res.status(status).json(body);
  }

  sendOAuthError(res, status, error, description) {
    this.sendJson(res, status, { error, error_description: description, iss: this.config.issuer });
  }

  async getClient(clientId) {
    return (await this.store.read()).clients[clientId] || null;
  }

  async registerClient(input) {
    const clientId = `client-${crypto.randomUUID()}`;
    const client = {
      client_id: clientId,
      client_name: input.client_name || 'MCP client',
      redirect_uris: input.redirect_uris,
      token_endpoint_auth_method: 'none',
    };
    await this.store.update((state) => { state.clients[clientId] = client; });
    return client;
  }

  async validateAuthorizationRequest(params) {
    const client = await this.getClient(params.client_id);
    if (!client) throw new Error('Unknown client_id.');
    if (params.response_type !== 'code') throw new Error('response_type=code is required.');
    if (!client.redirect_uris.includes(params.redirect_uri)) throw new Error('redirect_uri is not registered.');
    if (params.resource !== this.config.resource) throw new Error('resource does not match.');
    if (params.code_challenge_method !== 'S256' || !params.code_challenge) throw new Error('S256 PKCE is required.');
    const scopes = String(params.scope || 'desktop-commander').split(/\s+/).filter(Boolean);
    if (!scopes.includes('desktop-commander') || scopes.some((scope) => !['desktop-commander', 'offline_access'].includes(scope))) {
      throw new Error('Unsupported scope.');
    }
    return { client, clientId: params.client_id, redirectUri: params.redirect_uri, scope: scopes.join(' ') };
  }

  authorizationError(res, params, error, description, status = 400) {
    const client = this.store.state.clients[params.client_id];
    if (client?.redirect_uris.includes(params.redirect_uri)) {
      const redirect = new URL(params.redirect_uri);
      redirect.searchParams.set('error', error);
      redirect.searchParams.set('error_description', description);
      if (params.state) redirect.searchParams.set('state', params.state);
      redirect.searchParams.set('iss', this.config.issuer);
      res.redirect(302, redirect.toString());
      return;
    }
    this.sendOAuthError(res, status, error, description);
  }

  async getAccessJwks() {
    return this.accessJwksProvider();
  }

  async validateAccessAssertion(rawAssertion) {
    const parts = String(rawAssertion || '').split('.');
    if (parts.length !== 3) return null;
    const decode = (value) => {
      try { return JSON.parse(Buffer.from(value, 'base64url').toString('utf8')); } catch { return null; }
    };
    const header = decode(parts[0]);
    const claims = decode(parts[1]);
    if (!header || !claims || header.alg !== 'RS256' || !header.kid) return null;
    const audiences = Array.isArray(claims.aud) ? claims.aud : [claims.aud];
    const now = Math.floor(Date.now() / 1000);
    if (claims.iss !== this.accessTeamDomain || !audiences.includes(this.accessApplicationAud)
        || !Number.isFinite(claims.exp) || claims.exp <= now
        || (claims.nbf !== undefined && (!Number.isFinite(claims.nbf) || claims.nbf > now + 60))) return null;
    const jwk = (await this.getAccessJwks()).find((key) => key.kid === header.kid && key.kty === 'RSA');
    if (!jwk) return null;
    try {
      const verifier = crypto.createVerify('RSA-SHA256');
      verifier.update(`${parts[0]}.${parts[1]}`);
      verifier.end();
      if (!verifier.verify(crypto.createPublicKey({ key: jwk, format: 'jwk' }), Buffer.from(parts[2], 'base64url'))) return null;
    } catch {
      return null;
    }
    return { access: true, tokenType: 'Cloudflare-Access', claims };
  }

  verifyPkce(verifier, challenge) {
    if (typeof verifier !== 'string' || !/^[A-Za-z0-9._~-]{43,128}$/.test(verifier)) return false;
    const calculated = crypto.createHash('sha256').update(verifier, 'ascii').digest('base64url');
    return calculated === challenge;
  }

  async issueTokens(clientId, scope) {
    const accessToken = crypto.randomBytes(32).toString('base64url');
    const refreshToken = crypto.randomBytes(32).toString('base64url');
    const now = Date.now();
    await this.store.update((state) => {
      state.accessTokens[crypto.createHash('sha256').update(accessToken).digest('hex')] = {
        clientId,
        scope,
        resource: this.config.resource,
        expiresAt: now + this.config.oauthAccessTokenTtlSeconds * 1000,
      };
      state.refreshTokens[crypto.createHash('sha256').update(refreshToken).digest('hex')] = {
        clientId,
        scope,
        resource: this.config.resource,
        expiresAt: now + this.config.oauthRefreshTokenTtlSeconds * 1000,
      };
    });
    return { access_token: accessToken, refresh_token: refreshToken, token_type: 'Bearer', scope, resource: this.config.resource };
  }

  async validateAccessToken(rawToken, requiredScope) {
    const key = crypto.createHash('sha256').update(rawToken).digest('hex');
    const record = (await this.store.read()).accessTokens[key];
    return record && record.expiresAt > Date.now() && record.scope.split(/\s+/).includes(requiredScope) ? record : null;
  }

  async tokenPost(_req, res) {
    this.sendOAuthError(res, 400, 'unsupported_grant_type', 'Only authorization_code is supported by this test fixture.');
  }
}

function installOAuthRoutes(app, expressImpl, oauth, prefix) {
  app.use(prefix, expressImpl.urlencoded({ extended: false, limit: '64kb' }));
  app.get(`${prefix}/authorize`, (req, res) => { void oauth.authorizationGet(req, res); });
  app.post(`${prefix}/authorize`, (req, res) => { void oauth.authorizationPost(req, res); });
  app.post(`${prefix}/token`, (req, res) => { void oauth.tokenPost(req, res); });
}

function signJwt(privateKey, claims, kid = 'test-key') {
  const encode = (value) => Buffer.from(JSON.stringify(value)).toString('base64url');
  const header = encode({ alg: 'RS256', typ: 'JWT', kid });
  const payload = encode(claims);
  const signer = crypto.createSign('RSA-SHA256');
  signer.update(`${header}.${payload}`);
  signer.end();
  return `${header}.${payload}.${signer.sign(privateKey).toString('base64url')}`;
}

function authorizationParams(verifier, overrides = {}) {
  return {
    response_type: 'code',
    client_id: CLIENT_ID,
    redirect_uri: REDIRECT_URI,
    scope: 'coord',
    resource: RESOURCE,
    code_challenge: crypto.createHash('sha256').update(verifier, 'ascii').digest('base64url'),
    code_challenge_method: 'S256',
    state: 'bound-client-state',
    ...overrides,
  };
}

async function postForm(url, values, origin, headers = {}) {
  return fetch(url, {
    method: 'POST',
    headers: { origin, 'content-type': 'application/x-www-form-urlencoded', ...headers },
    body: new URLSearchParams(values),
    redirect: 'manual',
  });
}

test('Cloudflare Access is required for a password-free, CSRF-bound, one-time OAuth authorization', async (t) => {
  const { privateKey, publicKey } = crypto.generateKeyPairSync('rsa', { modulusLength: 2048 });
  const jwk = { ...publicKey.export({ format: 'jwk' }), kid: 'test-key', kty: 'RSA' };
  const keys = [jwk];
  const app = express();
  const server = http.createServer(app);
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  servers.add(server);
  t.after(async () => {
    if (server.listening) {
      await new Promise((resolve) => {
        server.close(resolve);
        server.closeAllConnections();
      });
    }
    servers.delete(server);
  });
  const origin = `http://127.0.0.1:${server.address().port}`;
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), 'coord-access-oauth-'));
  tempDirs.add(tempDir);

  const validClaims = {
    iss: TEAM_DOMAIN,
    aud: [ACCESS_AUD],
    sub: 'owner-subject-1',
    email: OWNER_EMAIL,
    exp: Math.floor(Date.now() / 1000) + 600,
    nbf: Math.floor(Date.now() / 1000) - 5,
  };
  const validAssertion = signJwt(privateKey, validClaims);
  const sidecar = { async call() { throw new Error('not used in this OAuth-only test'); } };
  await registerCoordMcp(app, {
    express,
    oauthModule: { OAuthServer: TestOAuthServer, installOAuthRoutes },
    issuer: ISSUER,
    authorizationOrigin: origin,
    accessTeamDomain: TEAM_DOMAIN,
    accessApplicationAud: ACCESS_AUD,
    accessAllowedEmails: [OWNER_EMAIL],
    accessJwksProvider: async () => keys,
    oauthStatePath: path.join(tempDir, 'oauth.json'),
    oauthDiagnosticLogPath: path.join(tempDir, 'oauth-audit.jsonl'),
    oauthAccessTokenTtlSeconds: 60,
    sidecar,
  });

  const oauthEndpoint = `${origin}/coord/oauth`;
  const params = authorizationParams('A'.repeat(43));
  const authorizeUrl = `${oauthEndpoint}/authorize?${new URLSearchParams(params)}`;

  const missing = await fetch(authorizeUrl, { redirect: 'manual' });
  assert.equal(missing.status, 403);

  const wrongAud = await fetch(authorizeUrl, {
    headers: { 'cf-access-jwt-assertion': signJwt(privateKey, { ...validClaims, aud: ['wrong-aud'] }) },
    redirect: 'manual',
  });
  assert.equal(wrongAud.status, 403);

  const wrongIssuer = await fetch(authorizeUrl, {
    headers: { 'cf-access-jwt-assertion': signJwt(privateKey, { ...validClaims, iss: 'https://wrong.cloudflareaccess.com' }) },
    redirect: 'manual',
  });
  assert.equal(wrongIssuer.status, 403);

  const expired = await fetch(authorizeUrl, {
    headers: { 'cf-access-jwt-assertion': signJwt(privateKey, { ...validClaims, exp: Math.floor(Date.now() / 1000) - 1 }) },
    redirect: 'manual',
  });
  assert.equal(expired.status, 403);

  const premature = await fetch(authorizeUrl, {
    headers: { 'cf-access-jwt-assertion': signJwt(privateKey, { ...validClaims, nbf: Math.floor(Date.now() / 1000) + 30 }) },
    redirect: 'manual',
  });
  assert.equal(premature.status, 403);

  const wrongIdentity = await fetch(authorizeUrl, {
    headers: { 'cf-access-jwt-assertion': signJwt(privateKey, { ...validClaims, email: 'not-owner@example.test' }) },
    redirect: 'manual',
  });
  assert.equal(wrongIdentity.status, 403);

  const noSubjectClaims = { ...validClaims };
  delete noSubjectClaims.sub;
  const noSubject = await fetch(authorizeUrl, {
    headers: { 'cf-access-jwt-assertion': signJwt(privateKey, noSubjectClaims) },
    redirect: 'manual',
  });
  assert.equal(noSubject.status, 200);

  const wrongSignature = await fetch(authorizeUrl, {
    headers: { 'cf-access-jwt-assertion': signJwt(crypto.generateKeyPairSync('rsa', { modulusLength: 2048 }).privateKey, validClaims) },
    redirect: 'manual',
  });
  assert.equal(wrongSignature.status, 403);

  const wrongCallback = await fetch(`${oauthEndpoint}/authorize?${new URLSearchParams(authorizationParams('A'.repeat(43), { redirect_uri: 'http://evil.example/callback' }))}`, {
    headers: { 'cf-access-jwt-assertion': validAssertion },
    redirect: 'manual',
  });
  assert.equal(wrongCallback.status, 400);

  const pageResponse = await fetch(authorizeUrl, {
    headers: { 'cf-access-jwt-assertion': validAssertion },
    redirect: 'manual',
  });
  assert.equal(pageResponse.status, 200);
  assert.equal(
    pageResponse.headers.get('content-security-policy'),
    `default-src 'none'; style-src 'unsafe-inline'; form-action 'self' ${new URL(REDIRECT_URI).origin}; base-uri 'none'; frame-ancestors 'none'`,
  );
  const page = await pageResponse.text();
  assert.match(page, /Authorize Coord MCP/);
  assert.match(page, /Coord test client/);
  assert.match(page, new RegExp(OWNER_EMAIL));
  assert.doesNotMatch(page, /password|type="password"/i);
  const csrfToken = page.match(/name="csrf_token" value="([A-Za-z0-9_-]{43})"/)?.[1];
  assert.ok(csrfToken);

  const localizedUrl = new URL(authorizeUrl);
  localizedUrl.searchParams.set('ui_locales', 'zh-CN');
  const localizedPage = await fetch(localizedUrl, {
    headers: { 'cf-access-jwt-assertion': validAssertion },
    redirect: 'manual',
  });
  assert.equal(localizedPage.status, 200);
  assert.match(await localizedPage.text(), /Authorize Coord MCP/);

  const duplicateLocaleUrl = new URL(localizedUrl);
  duplicateLocaleUrl.searchParams.append('ui_locales', 'en');
  const duplicateLocale = await fetch(duplicateLocaleUrl, { redirect: 'manual' });
  assert.equal(duplicateLocale.status, 400);
  assert.match((await duplicateLocale.json()).error_description, /unsupported or duplicate/);

  const unknownParameterUrl = new URL(authorizeUrl);
  unknownParameterUrl.searchParams.set('unexpected', 'value');
  const unknownParameter = await fetch(unknownParameterUrl, { redirect: 'manual' });
  assert.equal(unknownParameter.status, 400);
  assert.match((await unknownParameter.json()).error_description, /unsupported or duplicate/);

  const noAccessPost = await postForm(`${oauthEndpoint}/authorize`, { ...params, csrf_token: csrfToken }, origin);
  assert.equal(noAccessPost.status, 403);

  const jsonAuthorization = await fetch(`${oauthEndpoint}/authorize`, {
    method: 'POST',
    headers: {
      origin,
      'cf-access-jwt-assertion': validAssertion,
      'content-type': 'application/json',
    },
    body: JSON.stringify({ ...params, csrf_token: csrfToken }),
    redirect: 'manual',
  });
  assert.equal(jsonAuthorization.status, 415);

  const wrongOrigin = await postForm(`${oauthEndpoint}/authorize`, { ...params, csrf_token: csrfToken }, 'https://attacker.example', {
    'cf-access-jwt-assertion': validAssertion,
  });
  assert.equal(wrongOrigin.status, 403);

  const passwordFallback = await postForm(`${oauthEndpoint}/authorize`, { ...params, csrf_token: csrfToken, password: 'not-accepted' }, origin, {
    'cf-access-jwt-assertion': validAssertion,
  });
  assert.equal(passwordFallback.status, 400);

  const changedState = await postForm(`${oauthEndpoint}/authorize`, { ...params, state: 'attacker-state', csrf_token: csrfToken }, origin, {
    'cf-access-jwt-assertion': validAssertion,
  });
  assert.equal(changedState.status, 403);

  const freshPage = await fetch(authorizeUrl, {
    headers: { 'cf-access-jwt-assertion': validAssertion },
    redirect: 'manual',
  });
  const freshHtml = await freshPage.text();
  const freshCsrf = freshHtml.match(/name="csrf_token" value="([A-Za-z0-9_-]{43})"/)?.[1];
  assert.ok(freshCsrf);
  const authorized = await postForm(`${oauthEndpoint}/authorize`, { ...params, csrf_token: freshCsrf }, origin, {
    'cf-access-jwt-assertion': validAssertion,
  });
  assert.equal(authorized.status, 302);
  const redirect = new URL(authorized.headers.get('location'));
  const code = redirect.searchParams.get('code');
  assert.ok(code);
  assert.equal(redirect.searchParams.get('state'), params.state);

  const verifier = 'A'.repeat(43);
  const tokenRequest = {
    grant_type: 'authorization_code',
    client_id: CLIENT_ID,
    code,
    redirect_uri: REDIRECT_URI,
    code_verifier: verifier,
    resource: RESOURCE,
  };
  const exchange = () => postForm(`${oauthEndpoint}/token`, tokenRequest, origin);
  const exchanges = await Promise.all([exchange(), exchange()]);
  assert.deepEqual(exchanges.map((response) => response.status).sort(), [200, 400]);
});

test('missing Access configuration fails closed without taking down protocol metadata', async () => {
  const oauth = createCoordOAuthServer(TestOAuthServer, {
    issuer: ISSUER,
    resource: RESOURCE,
    accessAllowedEmails: [OWNER_EMAIL],
  });
  const response = {
    statusCode: 200,
    headers: {},
    setHeader(name, value) { this.headers[name] = value; },
    status(value) { this.statusCode = value; return this; },
    json(value) { this.body = value; return this; },
  };
  await oauth.authorizationGet({ originalUrl: '/coord/oauth/authorize', headers: {} }, response);
  assert.equal(response.statusCode, 503);
  assert.equal(response.body.error, 'access_not_configured');
  assert.equal(oauth.metadata().issuer, ISSUER);
  assert.deepEqual(oauth.metadata().scopes_supported, ['coord', 'offline_access']);
});

test('token lifecycle audit records grant and status without credentials or arbitrary input', async () => {
  const oauth = createCoordOAuthServer(TestOAuthServer, { issuer: ISSUER, resource: RESOURCE });
  const app = express();
  installOAuthRoutes(app, express, oauth, '/coord/oauth');
  const server = http.createServer(app);
  servers.add(server);
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const url = `http://127.0.0.1:${server.address().port}/coord/oauth/token?code=PRIVATE_QUERY`;
  for (const grant of ['authorization_code', 'refresh_token', 'PRIVATE_GRANT']) {
    const response = await fetch(url, {
      method: 'POST',
      headers: { 'content-type': 'application/x-www-form-urlencoded', authorization: 'Bearer PRIVATE_HEADER', cookie: 'PRIVATE_COOKIE' },
      body: new URLSearchParams({ grant_type: grant, client_id: 'PRIVATE_CLIENT', code: 'PRIVATE_CODE', refresh_token: 'PRIVATE_REFRESH', code_verifier: 'PRIVATE_VERIFIER', arbitrary: 'PRIVATE_FIELD' }),
    });
    assert.equal(response.status, 400);
    await response.text();
  }
  assert.deepEqual(oauth.auditRecords.map((entry) => entry.request.body.grant_type), ['authorization_code', 'refresh_token', 'unsupported']);
  assert.deepEqual(oauth.auditRecords.map((entry) => entry.response.statusCode), [400, 400, 400]);
  for (const entry of oauth.auditRecords) {
    assert.deepEqual(Object.keys(entry.request).sort(), ['body', 'headers', 'method', 'originalUrl', 'route']);
    assert.deepEqual(entry.request.headers, {});
    assert.equal(entry.request.originalUrl, '/coord/oauth/token');
    assert.deepEqual(Object.keys(entry.request.body), ['grant_type']);
  }
  assert.doesNotMatch(JSON.stringify(oauth.auditRecords), /PRIVATE_/);
});

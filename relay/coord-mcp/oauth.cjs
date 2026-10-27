const crypto = require('node:crypto');

const INTERNAL_SCOPE = 'desktop-commander';
const COORD_SCOPE = 'coord';
const OFFLINE_SCOPE = 'offline_access';
const AUTHORIZATION_PARAMETERS = [
  'response_type',
  'client_id',
  'redirect_uri',
  'scope',
  'resource',
  'code_challenge',
  'code_challenge_method',
  'state',
];
const AUTHORIZATION_PARAMETER_SET = new Set(AUTHORIZATION_PARAMETERS);
const OPTIONAL_IGNORED_AUTHORIZATION_PARAMETERS = new Set(['ui_locales']);
const AUTHORIZATION_REQUEST_TTL_MS = 5 * 60 * 1000;
const MAX_PENDING_AUTHORIZATION_REQUESTS = 256;

function toInternalScope(value) {
  const requested = [...new Set(String(value || COORD_SCOPE).trim().split(/\s+/).filter(Boolean))];
  if (!requested.includes(COORD_SCOPE)
      || requested.some((scope) => scope !== COORD_SCOPE && scope !== OFFLINE_SCOPE)) {
    throw new Error('Only the coord and optional offline_access scopes are supported.');
  }
  return requested.map((scope) => scope === COORD_SCOPE ? INTERNAL_SCOPE : scope).join(' ');
}

function toPublicScope(value) {
  return String(value || INTERNAL_SCOPE)
    .trim()
    .split(/\s+/)
    .filter(Boolean)
    .map((scope) => scope === INTERNAL_SCOPE ? COORD_SCOPE : scope)
    .join(' ');
}

function htmlEscape(value) {
  return String(value)
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

function hash(value) {
  return crypto.createHash('sha256').update(String(value), 'utf8').digest('hex');
}

function safeEqual(left, right) {
  const a = Buffer.from(String(left), 'utf8');
  const b = Buffer.from(String(right), 'utf8');
  return a.length === b.length && crypto.timingSafeEqual(a, b);
}

function parseAuthorizationQuery(req) {
  const originalUrl = String(req.originalUrl || req.url || '');
  const queryIndex = originalUrl.indexOf('?');
  const query = queryIndex < 0 ? '' : originalUrl.slice(queryIndex + 1).split('#', 1)[0];
  const params = {};
  const seen = new Set();
  for (const [key, value] of new URLSearchParams(query)) {
    if ((!AUTHORIZATION_PARAMETER_SET.has(key) && !OPTIONAL_IGNORED_AUTHORIZATION_PARAMETERS.has(key))
        || seen.has(key)) {
      throw new Error('The authorization request contains unsupported or duplicate parameters.');
    }
    seen.add(key);
    if (AUTHORIZATION_PARAMETER_SET.has(key)) params[key] = value;
  }
  return params;
}

function canonicalAuthorizationBinding(validated) {
  const params = validated.canonicalParams;
  return {
    clientId: validated.clientId,
    redirectUri: validated.redirectUri,
    responseType: params.response_type,
    scope: validated.scope,
    resource: params.resource,
    codeChallenge: params.code_challenge,
    codeChallengeMethod: params.code_challenge_method,
    state: params.state ?? null,
  };
}

function authorizationBindingHash(validated) {
  return hash(JSON.stringify(canonicalAuthorizationBinding(validated)));
}

function canonicalAccessTeamDomain(value) {
  if (typeof value !== 'string' || !value.trim()) return '';
  try {
    const parsed = new URL(value.trim());
    if (parsed.protocol !== 'https:' || parsed.pathname !== '/' || parsed.search || parsed.hash || parsed.username || parsed.password) return '';
    return parsed.origin;
  } catch {
    return '';
  }
}

function normalizeAllowedEmails(value) {
  const entries = Array.isArray(value) ? value : String(value || '').split(',');
  return [...new Set(entries
    .map((entry) => String(entry).trim().toLowerCase())
    .filter((entry) => entry && entry.length <= 320 && /^[^\s@]+@[^\s@]+$/.test(entry)))];
}

function accessHeader(req) {
  return typeof req.get === 'function'
    ? req.get('cf-access-jwt-assertion') || ''
    : req.headers?.['cf-access-jwt-assertion'] || '';
}

function requestOrigin(req) {
  return typeof req.get === 'function' ? req.get('origin') || '' : req.headers?.origin || '';
}

function requestBody(req) {
  return req.body && typeof req.body === 'object' && !Array.isArray(req.body) ? req.body : {};
}

function accessFailure(res, status, error, description) {
  res.setHeader('Cache-Control', 'no-store');
  res.setHeader('Referrer-Policy', 'no-referrer');
  res.status(status).json({ error, error_description: description });
}

function createCoordOAuthServer(BaseOAuthServer, config, options = {}) {
  if (typeof BaseOAuthServer !== 'function') throw new TypeError('OAuthServer implementation is required.');

  class CoordOAuthServer extends BaseOAuthServer {
    constructor(baseConfig, baseOptions) {
      super(baseConfig, baseOptions);
      this.accessTeamDomain = canonicalAccessTeamDomain(baseConfig.accessTeamDomain);
      this.accessApplicationAud = typeof baseConfig.accessApplicationAud === 'string'
        ? baseConfig.accessApplicationAud.trim()
        : '';
      this.accessAllowedEmails = new Set(normalizeAllowedEmails(baseConfig.accessAllowedEmails));
      this.accessAuthorizationOrigin = baseConfig.authorizationOrigin;
      if (typeof options.accessJwksProvider === 'function') this.getAccessJwks = options.accessJwksProvider;
      this.accessConfigurationReady = Boolean(
        this.accessTeamDomain
        && this.accessApplicationAud
        && this.accessAllowedEmails.size
        && typeof this.validateAccessAssertion === 'function',
      );
    }

    metadata() {
      return { ...super.metadata(), scopes_supported: [COORD_SCOPE, OFFLINE_SCOPE] };
    }

    protectedResourceMetadata() {
      return { ...super.protectedResourceMetadata(), scopes_supported: [COORD_SCOPE, OFFLINE_SCOPE] };
    }

    challenge(res, invalid = false) {
      const details = invalid ? ', error="invalid_token"' : '';
      this.setCors(res);
      res.setHeader(
        'WWW-Authenticate',
        `Bearer resource_metadata="${this.protectedResourceMetadataUrl}", scope="${COORD_SCOPE}"${details}`,
      );
      res.status(401).json({
        error: 'unauthorized',
        error_description: 'A valid Coord MCP OAuth 2.1 Bearer token is required.',
      });
    }

    async registerClient(body) {
      const client = await super.registerClient(body);
      return { ...client, scope: COORD_SCOPE };
    }

    async validateAuthorizationRequest(params) {
      const scope = toInternalScope(params.scope);
      const validated = await super.validateAuthorizationRequest({ ...params, scope });
      if (params.state !== undefined && (typeof params.state !== 'string' || !params.state || Buffer.byteLength(params.state) > 1024)) {
        throw new Error('state must be a non-empty string no longer than 1024 bytes.');
      }
      if (typeof params.code_challenge !== 'string' || !/^[A-Za-z0-9_-]{43}$/.test(params.code_challenge)) {
        throw new Error('A valid S256 PKCE code_challenge is required.');
      }
      if (params.code_challenge_method !== 'S256') throw new Error('PKCE with code_challenge_method=S256 is required.');

      const publicScope = toPublicScope(validated.scope || scope);
      const canonicalParams = {
        response_type: params.response_type,
        client_id: validated.clientId,
        redirect_uri: validated.redirectUri,
        scope: publicScope,
        resource: params.resource,
        code_challenge: params.code_challenge,
        code_challenge_method: params.code_challenge_method,
      };
      if (params.state !== undefined) canonicalParams.state = params.state;
      return { ...validated, scope: publicScope, canonicalParams };
    }

    async validateAccessIdentity(req) {
      if (!this.accessConfigurationReady) return { ok: false, status: 503, code: 'access_not_configured' };
      const assertion = accessHeader(req);
      if (typeof assertion !== 'string' || !assertion) return { ok: false, status: 403, code: 'access_assertion_required' };

      let verified;
      try {
        verified = await super.validateAccessAssertion(assertion);
      } catch {
        verified = null;
      }
      const claims = verified?.claims;
      if (!verified?.access || !claims || typeof claims !== 'object') return { ok: false, status: 403, code: 'invalid_access_assertion' };

      const audience = Array.isArray(claims.aud) ? claims.aud : [claims.aud];
      const now = Math.floor(Date.now() / 1000);
      if (claims.iss !== this.accessTeamDomain
          || !audience.includes(this.accessApplicationAud)
          || !Number.isFinite(claims.exp)
          || claims.exp <= now
          || (claims.nbf !== undefined && (!Number.isFinite(claims.nbf) || claims.nbf > now))) {
        return { ok: false, status: 403, code: 'invalid_access_claims' };
      }

      const email = typeof claims.email === 'string' ? claims.email.trim().toLowerCase() : '';
      const subject = typeof claims.sub === 'string' && claims.sub.trim()
        ? claims.sub.trim()
        : email;
      if (!email || !subject || subject.length > 1024 || !this.accessAllowedEmails.has(email)) {
        return { ok: false, status: 403, code: 'access_identity_not_allowed' };
      }
      return { ok: true, identity: { email, subject } };
    }

    verifyOwnerPassword() {
      return false;
    }

    async createAuthorizationChallenge(validated, identity) {
      const csrfToken = crypto.randomBytes(32).toString('base64url');
      const csrfHash = hash(csrfToken);
      const bindingHash = authorizationBindingHash(validated);
      const now = Date.now();
      await this.store.update((state) => {
        if (!state.authorizationRequests || typeof state.authorizationRequests !== 'object') state.authorizationRequests = {};
        for (const [key, record] of Object.entries(state.authorizationRequests)) {
          if (!record || record.expiresAt <= now) delete state.authorizationRequests[key];
        }
        if (Object.keys(state.authorizationRequests).length >= MAX_PENDING_AUTHORIZATION_REQUESTS) {
          throw new Error('Too many pending authorization requests.');
        }
        state.authorizationRequests[csrfHash] = {
          bindingHash,
          email: identity.email,
          subject: identity.subject,
          expiresAt: now + AUTHORIZATION_REQUEST_TTL_MS,
        };
      });
      return csrfToken;
    }

    setAuthorizationPageHeaders(res, redirectUri = '') {
      res.setHeader('Cache-Control', 'no-store');
      res.setHeader('Referrer-Policy', 'no-referrer');
      res.setHeader('X-Content-Type-Options', 'nosniff');
      let formAction = "'self'";
      if (redirectUri) {
        const target = new URL(redirectUri);
        if (target.origin !== this.accessAuthorizationOrigin) {
          formAction += ` ${target.origin}`;
        }
      }
      res.setHeader('Content-Security-Policy', `default-src 'none'; style-src 'unsafe-inline'; form-action ${formAction}; base-uri 'none'; frame-ancestors 'none'`);
    }

    async authorizationGet(req, res) {
      let params = {};
      try {
        params = parseAuthorizationQuery(req);
        const access = await this.validateAccessIdentity(req);
        if (!access.ok) {
          accessFailure(res, access.status, access.code, 'Cloudflare Access identity verification is required.');
          return;
        }
        const validated = await this.validateAuthorizationRequest(params);
        const csrfToken = await this.createAuthorizationChallenge(validated, access.identity);
        const hidden = Object.entries({ ...validated.canonicalParams, csrf_token: csrfToken })
          .map(([key, value]) => `<input type="hidden" name="${htmlEscape(key)}" value="${htmlEscape(value)}">`)
          .join('');
        const clientName = validated.client?.client_name || 'OAuth client';
        const action = `${this.authorizationFormActionPrefix}/authorize`;
        this.setAuthorizationPageHeaders(res, validated.redirectUri);
        this.setCors(res);
        res.type('html').send(`<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Authorize Coord MCP</title></head><body><main><h1>Authorize Coord MCP</h1><p><strong>${htmlEscape(clientName)}</strong> requests access to the coord task service.</p><p>Signed in as ${htmlEscape(access.identity.email)}.</p><form method="post" action="${htmlEscape(action)}">${hidden}<button type="submit">Authorize</button></form></main></body></html>`);
      } catch (error) {
        if (res.headersSent) return;
        this.setAuthorizationPageHeaders(res);
        this.authorizationError(res, params, 'invalid_request', error.message || 'Invalid authorization request.');
      }
    }

    async authorizationPost(req, res) {
      const contentType = typeof req.get === 'function'
        ? req.get('content-type') || ''
        : req.headers?.['content-type'] || '';
      if (!/^application\/x-www-form-urlencoded(?:\s*;|$)/i.test(contentType)) {
        accessFailure(res, 415, 'form_encoding_required', 'Authorization must be submitted as a browser form.');
        return;
      }
      const body = requestBody(req);
      const allowedKeys = new Set([...AUTHORIZATION_PARAMETERS, 'csrf_token']);
      const params = {};
      try {
        for (const [key, value] of Object.entries(body)) {
          if (!allowedKeys.has(key) || typeof value !== 'string') {
            accessFailure(res, 400, 'invalid_authorization_request', 'The authorization form is invalid.');
            return;
          }
          if (key !== 'csrf_token') params[key] = value;
        }
        const access = await this.validateAccessIdentity(req);
        if (!access.ok) {
          accessFailure(res, access.status, access.code, 'Cloudflare Access identity verification is required.');
          return;
        }
        let origin = '';
        try {
          origin = new URL(requestOrigin(req)).origin;
        } catch {
          origin = '';
        }
        if (!origin || origin !== this.accessAuthorizationOrigin) {
          accessFailure(res, 403, 'invalid_authorization_origin', 'The authorization request origin is not allowed.');
          return;
        }

        const csrfToken = body.csrf_token;
        if (typeof csrfToken !== 'string' || !/^[A-Za-z0-9_-]{43}$/.test(csrfToken)) {
          accessFailure(res, 403, 'invalid_authorization_request', 'The authorization request could not be verified.');
          return;
        }
        const validated = await this.validateAuthorizationRequest(params);
        const expectedBinding = authorizationBindingHash(validated);
        const csrfHash = hash(csrfToken);
        const now = Date.now();
        const code = crypto.randomBytes(32).toString('base64url');
        let accepted = false;

        await this.store.update((state) => {
          const pending = state.authorizationRequests?.[csrfHash];
          if (!pending) return;
          delete state.authorizationRequests[csrfHash];
          if (pending.expiresAt <= now
              || pending.subject !== access.identity.subject
              || pending.email !== access.identity.email
              || !safeEqual(pending.bindingHash, expectedBinding)) return;
          if (!state.codes || typeof state.codes !== 'object') state.codes = {};
          state.codes[hash(code)] = {
            clientId: validated.clientId,
            redirectUri: validated.redirectUri,
            codeChallenge: params.code_challenge,
            resource: this.config.resource,
            scope: toInternalScope(validated.scope),
            expiresAt: now + 300000,
          };
          accepted = true;
        });

        if (!accepted) {
          accessFailure(res, 403, 'invalid_authorization_request', 'The authorization request expired or could not be verified.');
          return;
        }

        const redirect = new URL(validated.redirectUri);
        redirect.searchParams.set('code', code);
        if (params.state) redirect.searchParams.set('state', params.state);
        redirect.searchParams.set('iss', this.config.issuer);
        res.setHeader('Cache-Control', 'no-store');
        res.setHeader('Referrer-Policy', 'no-referrer');
        res.redirect(302, redirect.toString());
      } catch (error) {
        if (res.headersSent) return;
        this.authorizationError(res, params, 'invalid_request', error.message || 'Invalid authorization request.');
      }
    }

    async tokenPost(req, res) {
      const body = requestBody(req);
      this.auditRequest({
        method: 'POST',
        originalUrl: '/coord/oauth/token',
        headers: {},
        body: { grant_type: ['authorization_code', 'refresh_token'].includes(body.grant_type) ? body.grant_type : 'unsupported' },
      }, res, 'coord.oauth.token');
      if (body.grant_type !== 'authorization_code') return super.tokenPost(req, res);

      try {
        const client = await this.getClient(body.client_id);
        if (!client || client.token_endpoint_auth_method !== 'none') {
          this.sendOAuthError(res, 400, 'invalid_client', 'Unknown or unsupported client.');
          return;
        }
        if (body.resource !== this.config.resource) {
          this.sendOAuthError(res, 400, 'invalid_grant', 'resource does not match the protected resource.');
          return;
        }

        const codeHash = hash(body.code || '');
        const current = (await this.store.read()).codes?.[codeHash];
        if (!current || current.expiresAt <= Date.now()
            || current.clientId !== body.client_id
            || current.redirectUri !== body.redirect_uri
            || current.resource !== body.resource
            || !this.verifyPkce(body.code_verifier, current.codeChallenge)) {
          this.sendOAuthError(res, 400, 'invalid_grant', 'The authorization code, redirect URI, resource, or PKCE verifier is invalid.');
          return;
        }

        let acceptedRecord = null;
        await this.store.update((state) => {
          const record = state.codes?.[codeHash];
          if (!record || record.expiresAt <= Date.now()
              || record.clientId !== body.client_id
              || record.redirectUri !== body.redirect_uri
              || record.resource !== body.resource
              || !this.verifyPkce(body.code_verifier, record.codeChallenge)) return;
          delete state.codes[codeHash];
          acceptedRecord = record;
        });
        if (!acceptedRecord) {
          this.sendOAuthError(res, 400, 'invalid_grant', 'The authorization code, redirect URI, resource, or PKCE verifier is invalid.');
          return;
        }
        this.sendJson(res, 200, await this.issueTokens(body.client_id, acceptedRecord.scope, null));
      } catch {
        this.sendOAuthError(res, 500, 'server_error', 'Token issuance failed.');
      }
    }

    async issueTokens(clientId, scope, familyId) {
      const tokens = await super.issueTokens(clientId, scope, familyId);
      return { ...tokens, scope: toPublicScope(tokens.scope) };
    }

    async validateAccessToken(rawToken, requiredScope = COORD_SCOPE) {
      if (requiredScope !== COORD_SCOPE) return null;
      const record = await super.validateAccessToken(rawToken, INTERNAL_SCOPE);
      return record ? { ...record, scope: toPublicScope(record.scope), tokenSha256: crypto.createHash('sha256').update(rawToken).digest('hex') } : null;
    }

    async authenticateRequest(req) {
      const value = typeof req.get === 'function'
        ? req.get('authorization') || ''
        : req.headers?.authorization || '';
      const match = String(value).match(/^Bearer[ \t]+([^ \t]+)$/i);
      return match ? this.validateAccessToken(match[1]) : null;
    }
  }

  return new CoordOAuthServer({
    ...config,
    accessTeamDomain: config.accessTeamDomain,
    accessApplicationAud: config.accessApplicationAud,
    accessAllowedEmails: config.accessAllowedEmails,
    authorizationOrigin: config.authorizationOrigin || new URL(config.issuer).origin,
  }, options);
}

module.exports = { COORD_SCOPE, createCoordOAuthServer, toInternalScope, toPublicScope };

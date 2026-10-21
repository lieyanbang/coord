const INTERNAL_SCOPE = 'desktop-commander';
const COORD_SCOPE = 'coord';
const OFFLINE_SCOPE = 'offline_access';

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

function createCoordOAuthServer(BaseOAuthServer, config, options = {}) {
  if (typeof BaseOAuthServer !== 'function') throw new TypeError('OAuthServer implementation is required.');

  class CoordOAuthServer extends BaseOAuthServer {
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
      return super.validateAuthorizationRequest({
        ...params,
        scope: toInternalScope(params.scope),
      });
    }

    async issueTokens(clientId, scope, familyId) {
      const tokens = await super.issueTokens(clientId, scope, familyId);
      return { ...tokens, scope: toPublicScope(tokens.scope) };
    }

    async validateAccessToken(rawToken, requiredScope = COORD_SCOPE) {
      if (requiredScope !== COORD_SCOPE) return null;
      const record = await super.validateAccessToken(rawToken, INTERNAL_SCOPE);
      return record ? { ...record, scope: toPublicScope(record.scope) } : null;
    }

    async authenticateRequest(req) {
      const value = typeof req.get === 'function'
        ? req.get('authorization') || ''
        : req.headers?.authorization || '';
      const match = String(value).match(/^Bearer[ \t]+([^ \t]+)$/i);
      return match ? this.validateAccessToken(match[1]) : null;
    }

    async authorizationGet(req, res) {
      const send = res.send;
      res.send = function sendCoordAuthorization(body) {
        const rendered = typeof body === 'string'
          ? body.replaceAll('Desktop Commander', 'Coord MCP')
          : body;
        return send.call(this, rendered);
      };
      try {
        await super.authorizationGet(req, res);
      } finally {
        res.send = send;
      }
    }
  }

  return new CoordOAuthServer(config, options);
}

module.exports = { COORD_SCOPE, createCoordOAuthServer, toInternalScope, toPublicScope };

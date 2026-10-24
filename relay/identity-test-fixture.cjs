const { hash, validatePolicy } = require('./identity.cjs');

// Synthetic values only. This fixture proves protocol behavior, never ChatGPT
// conversation identity, production credentials, or a production security gate.
function fixturePolicy(batchId, now = Date.now(), scopes) {
  return validatePolicy({ version: 1, enabled: true,
    signing_key_file: require('node:path').resolve('synthetic-test-key'), legacy_client_sha256: [hash('legacy-client')],
    bindings: [0, 1].map(i => ({ id: `synthetic-A${i}`, client_sha256: hash(`client-${i}`), token_sha256: hash(`token-${i}`),
      agent_id: `A${i}`, batch_id: batchId,
      scopes: scopes || ['coord:read', 'coord:claim', 'coord:progress', 'coord:extend', 'coord:submit', 'coord:failure'],
      not_before: now - 1_000, expires_at: now + 300_000 })),
  }, 'ab'.repeat(32));
}
function fixtureAuth(i, now = Date.now()) {
  return { clientId: `client-${i}`, tokenSha256: hash(`token-${i}`), expiresAt: now + 300_000, scope: 'coord' };
}
module.exports = { fixturePolicy, fixtureAuth };

const crypto = require('node:crypto');
const fs = require('node:fs/promises');
const path = require('node:path');

const HASH = /^[a-f0-9]{64}$/;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const SCOPES = Object.freeze({
  'tasks/get': 'coord:read', 'tasks/list': 'coord:read', 'scheduler/agent_status': 'coord:read',
  'tasks/claim': 'coord:claim', 'scheduler/claim': 'coord:claim',
  'scheduler/trusted_claim': 'coord:claim',
  'scheduler/progress': 'coord:progress',
  'scheduler/trusted_progress': 'coord:progress',
  'tasks/extend': 'coord:extend', 'scheduler/extend': 'coord:extend', 'scheduler/extend_progress': 'coord:extend',
  'tasks/complete': 'coord:submit', 'scheduler/submit': 'coord:submit', 'scheduler/failure': 'coord:failure',
  'scheduler/trusted_complete': 'coord:submit', 'scheduler/trusted_release': 'coord:release',
  'scheduler/trusted_fail': 'coord:failure',
  'scheduler/trusted_resume': 'coord:read', 'scheduler/trusted_status': 'coord:read',
});
const VALID_SCOPES = new Set(Object.values(SCOPES));
// Linux facade and Windows Relay use separate wall clocks. Keep the signed
// lifetime and hard expiry unchanged while tolerating a bounded future issue.
const MAX_FUTURE_ISSUE_SKEW_MS = 5_000;
const hash = value => crypto.createHash('sha256').update(value).digest('hex');
const record = value => value && typeof value === 'object' && !Array.isArray(value);
const exact = (value, keys) => record(value) && Object.keys(value).every(k => keys.includes(k)) && keys.every(k => Object.hasOwn(value, k));
const WORKER_ID_PATTERN = '^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$';
function normalizeWorkerId(value) {
  if (typeof value !== 'string' || !new RegExp(WORKER_ID_PATTERN).test(value)) return null;
  if (/^[0-9a-f]{4}$/i.test(value)) return value.toUpperCase();
  if (/^emote-[0-9a-f]{4}$/i.test(value)) return `emote-${value.slice(6).toUpperCase()}`;
  return value;
}
function logicalAgentId(worker) {
  return /^[0-9A-F]{4}$/.test(worker) ? `emote-${worker}` : worker;
}

function validatePolicy(config, key) {
  const persistent = config?.version === 3 && config.mode === 'logical';
  const logical = persistent || (config?.version === 2 && config.mode === 'logical');
  if (!exact(config, ['version', 'enabled', 'signing_key_file', 'legacy_client_sha256', 'bindings', ...(logical ? ['mode'] : [])])
      || (!logical && config.version !== 1) || typeof config.enabled !== 'boolean'
      || typeof config.signing_key_file !== 'string' || !path.isAbsolute(config.signing_key_file)
      || !Array.isArray(config.bindings) || !Array.isArray(config.legacy_client_sha256)
      || !config.legacy_client_sha256.every(v => typeof v === 'string' && HASH.test(v))) throw new Error('identity_policy_invalid');
  if (!config.enabled) return null;
  if (!HASH.test(key || '')) throw new Error('identity_signing_key_invalid');
  const ids = new Set(); const clients = new Set(); const agents = new Set(); const tokens = new Set(); const pairs = new Set();
  for (const b of config.bindings) {
    if (!exact(b, ['id', 'client_sha256', 'batch_id', 'scopes', ...(!persistent ? ['not_before', 'expires_at'] : []), ...(logical ? ['worker_ids'] : ['token_sha256', 'agent_id'])])
        || typeof b.id !== 'string' || !/^[a-zA-Z0-9_-]{1,64}$/.test(b.id)
        || typeof b.client_sha256 !== 'string' || !HASH.test(b.client_sha256)
        || typeof b.batch_id !== 'string' || !(UUID.test(b.batch_id) || (persistent && b.batch_id === '*')) || !Array.isArray(b.scopes) || b.scopes.length === 0
        || !b.scopes.every(s => VALID_SCOPES.has(s)) || new Set(b.scopes).size !== b.scopes.length
        || (!persistent && (!Number.isSafeInteger(b.not_before) || !Number.isSafeInteger(b.expires_at)
          || b.not_before < 0 || b.expires_at <= b.not_before || b.expires_at - b.not_before > 86_400_000))
        || ids.has(b.id)) throw new Error('identity_binding_invalid_or_shared');
    if (logical) {
      const pair = `${b.client_sha256}:${b.batch_id}`;
      if (!Array.isArray(b.worker_ids) || b.worker_ids.length < 1 || b.worker_ids.length > 64
          || !b.worker_ids.every(w => typeof w === 'string' && (w === '*' || normalizeWorkerId(w) === w))
          || (b.worker_ids.includes('*') && b.worker_ids.length !== 1)
          || (persistent && (b.worker_ids.length !== 1 || b.worker_ids[0] !== '*'))
          || new Set(b.worker_ids).size !== b.worker_ids.length || pairs.has(pair)
          || (persistent && config.bindings.some(other => other !== b && other.client_sha256 === b.client_sha256
            && (other.batch_id === '*' || b.batch_id === '*')))) throw new Error('logical_binding_invalid');
      pairs.add(pair);
    } else if (typeof b.token_sha256 !== 'string' || !HASH.test(b.token_sha256)
        || typeof b.agent_id !== 'string' || !/^A[0-5]$/.test(b.agent_id)
        || clients.has(b.client_sha256) || agents.has(b.agent_id) || tokens.has(b.token_sha256)
        || config.legacy_client_sha256.includes(b.client_sha256)) throw new Error('identity_binding_invalid_or_shared');
    ids.add(b.id); clients.add(b.client_sha256); agents.add(b.agent_id); tokens.add(b.token_sha256);
  }
  return { ...config, bindings: persistent ? config.bindings.map(binding => ({ ...binding, not_before: 0, expires_at: Number.MAX_SAFE_INTEGER })) : config.bindings, key };
}

async function loadPolicy(file) {
  if (!file) return null;
  if (!path.isAbsolute(file)) throw new Error('identity_policy_path_not_absolute');
  const raw = JSON.parse(await fs.readFile(file, 'utf8'));
  const key = raw.enabled ? (await fs.readFile(raw.signing_key_file, 'utf8')).trim() : '';
  return validatePolicy(raw, key);
}

function resolvePrincipal(policy, authInfo, now = Date.now(), declaration) {
  if (!policy) {
    if (declaration) throw new Error('logical_worker_disabled');
    return null;
  }
  if (!record(authInfo) || typeof authInfo.clientId !== 'string') throw new Error('identity_binding_missing');
  const client = hash(authInfo.clientId);
  if (policy.mode === 'logical') {
    if (!Number.isSafeInteger(authInfo.expiresAt) || authInfo.expiresAt <= now) throw new Error('identity_binding_missing_or_expired');
    if (!declaration) {
      if (!policy.legacy_client_sha256.includes(client) && !policy.bindings.some(b => b.client_sha256 === client)) throw new Error('identity_binding_missing');
      return null;
    }
    if (!exact(declaration, ['worker_id', 'batch_id']) || typeof declaration.worker_id !== 'string'
        || !normalizeWorkerId(declaration.worker_id) || !UUID.test(declaration.batch_id)) throw new Error('logical_worker_invalid');
    const worker = normalizeWorkerId(declaration.worker_id);
    const b = policy.bindings.find(b => b.client_sha256 === client && (b.batch_id === declaration.batch_id || b.batch_id === '*')
      && (b.worker_ids.includes(worker) || b.worker_ids.includes('*')));
    if (!b || now < b.not_before || now >= b.expires_at) throw new Error('logical_worker_or_batch_denied');
    return { binding_id: b.id, agent_id: logicalAgentId(worker), worker_id: worker, batch_id: declaration.batch_id,
      scopes: [...b.scopes], issued_at: now, expires_at: Math.min(b.expires_at, authInfo.expiresAt, now + 60_000) };
  }
  if (declaration) throw new Error('logical_worker_disabled');
  if (policy.legacy_client_sha256.includes(client)) return null;
  const b = policy.bindings.find(item => item.client_sha256 === client && item.token_sha256 === authInfo.tokenSha256);
  if (!b || now < b.not_before || now >= b.expires_at || !Number.isSafeInteger(authInfo.expiresAt) || authInfo.expiresAt <= now) {
    throw new Error('identity_binding_missing_or_expired');
  }
  return { binding_id: b.id, agent_id: b.agent_id, batch_id: b.batch_id,
    scopes: [...b.scopes], issued_at: now, expires_at: Math.min(b.expires_at, authInfo.expiresAt, now + 60_000) };
}

function signature(request, context, key) {
  return crypto.createHmac('sha256', Buffer.from(key, 'hex')).update(JSON.stringify([
    'coord-identity-v1', request.requestId, request.method, request.params, context,
  ])).digest('hex');
}

function signRequest(request, principal, policy) {
  if (!principal) return request;
  return { ...request, identity: { ...principal, proof: signature(request, principal, policy.key) } };
}

function verifyRequest(request, policy, now = Date.now()) {
  if (!request.identity) return { ok: true, agentId: 'coord-relay-agent' };
  if (!policy) return { ok: false, code: 'identity_disabled' };
  const { proof, ...p } = request.identity;
  const logical = policy.mode === 'logical';
  if (!exact(p, ['binding_id', 'agent_id', 'batch_id', 'scopes', 'issued_at', 'expires_at', ...(logical ? ['worker_id'] : [])]) || typeof proof !== 'string' || !HASH.test(proof)) return { ok: false, code: 'identity_invalid' };
  const b = policy.bindings.find(item => item.id === p.binding_id);
  const expected = signature(request, p, policy.key);
  if (!crypto.timingSafeEqual(Buffer.from(expected, 'hex'), Buffer.from(proof, 'hex'))
      || !b || !UUID.test(p.batch_id) || (b.batch_id !== '*' && b.batch_id !== p.batch_id)
      || (logical ? !normalizeWorkerId(p.worker_id) || normalizeWorkerId(p.worker_id) !== p.worker_id || !(b.worker_ids.includes(p.worker_id) || b.worker_ids.includes('*')) || p.agent_id !== logicalAgentId(p.worker_id) : b.agent_id !== p.agent_id)
      || JSON.stringify(b.scopes) !== JSON.stringify(p.scopes)
      || !Number.isSafeInteger(p.issued_at) || !Number.isSafeInteger(p.expires_at)
      || p.issued_at > now + MAX_FUTURE_ISSUE_SKEW_MS || p.issued_at < b.not_before || p.expires_at > b.expires_at
      || p.expires_at - p.issued_at > 60_000 || p.expires_at <= now || p.expires_at <= p.issued_at) return { ok: false, code: 'identity_invalid_or_expired' };
  const scope = SCOPES[request.method];
  if (!scope || !b.scopes.includes(scope)) return { ok: false, code: 'identity_scope_denied' };
  if (request.method === 'tasks/list' && request.params.batch_id !== p.batch_id) return { ok: false, code: 'identity_batch_mismatch' };
  const agent = request.params.agentId ?? request.params.agent_id;
  if (agent !== undefined && agent !== p.agent_id) return { ok: false, code: 'identity_agent_mismatch' };
  if (logical && request.params.worker_id !== undefined && request.params.worker_id !== p.worker_id) return { ok: false, code: 'identity_worker_mismatch' };
  if (['scheduler/claim', 'scheduler/agent_status', 'scheduler/trusted_claim', 'scheduler/trusted_progress', 'scheduler/trusted_complete', 'scheduler/trusted_release', 'scheduler/trusted_resume', 'scheduler/trusted_status'].includes(request.method) && request.params.batch_id !== p.batch_id) return { ok: false, code: 'identity_batch_mismatch' };
  if (request.params.batch_id !== undefined && request.params.batch_id !== p.batch_id) return { ok: false, code: 'identity_batch_mismatch' };
  return { ok: true, agentId: p.agent_id, batchId: p.batch_id };
}

function protectedBatch(policy, batchId) {
  return Boolean(policy && batchId && policy.bindings.some(b => b.batch_id === batchId || b.batch_id === '*'));
}

module.exports = { SCOPES, WORKER_ID_PATTERN, normalizeWorkerId, hash, loadPolicy, validatePolicy, resolvePrincipal, signRequest, verifyRequest, protectedBatch };

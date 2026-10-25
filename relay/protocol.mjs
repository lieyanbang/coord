import { createHmac, timingSafeEqual } from 'node:crypto';
import identity from './identity.cjs';

export const PROTOCOL_VERSION = 1;
export const RELAY_AGENT_ID = 'arasaka-coord-agent';
export const COORD_AGENT_ID = 'coord-relay-agent';
export const MAX_REQUEST_BYTES = 64 * 1024;
export const MAX_FRAME_BYTES = 96 * 1024;
export const MAX_RESPONSE_BYTES = 64 * 1024;
export const MAX_PENDING = 4;

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const LEASE_TOKEN_RE = UUID_RE;
const STATES = new Set(['pending', 'claimed', 'submitted', 'completed', 'failed', 'cancelled']);
const PRIORITIES = new Set(['low', 'normal', 'high', 'urgent']);
const TRUSTED_METHODS = new Set([
  'scheduler/trusted_claim', 'scheduler/trusted_progress', 'scheduler/trusted_complete',
  'scheduler/trusted_release', 'scheduler/trusted_resume', 'scheduler/trusted_status',
]);

function isRecord(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function exactKeys(value, allowed, required = []) {
  if (!isRecord(value)) return false;
  const keys = Object.keys(value);
  return keys.every(key => allowed.includes(key)) && required.every(key => Object.hasOwn(value, key));
}

function validUuid(value) {
  return typeof value === 'string' && UUID_RE.test(value);
}

function validLeaseToken(value) {
  return typeof value === 'string' && LEASE_TOKEN_RE.test(value);
}

function validLeaseSeconds(value) {
  return value === undefined || (Number.isInteger(value) && value >= 1 && value <= 600);
}

function boundedJson(value, maxBytes = MAX_REQUEST_BYTES) {
  try {
    return Buffer.byteLength(JSON.stringify(value)) <= maxBytes;
  } catch {
    return false;
  }
}

function validTaskResult(value) {
  return value !== undefined && boundedJson(value, MAX_RESPONSE_BYTES);
}

function validAck(params, postAckKey, ackNameKey, ackPriorityKey) {
  if (params[postAckKey] === undefined) return true;
  if (typeof params[postAckKey] !== 'boolean') return false;
  if (!params[postAckKey]) return true;
  return typeof params[ackNameKey] === 'string'
    && params[ackNameKey].length > 0
    && params[ackNameKey].length <= 256
    && (params[ackPriorityKey] === undefined || PRIORITIES.has(params[ackPriorityKey]));
}

function validTrustedAck(value) {
  if (!isRecord(value)) return false;
  return exactKeys(value, ['name', 'priority', 'payload'], ['name'])
    && typeof value.name === 'string' && value.name.length > 0 && value.name.length <= 256
    && (value.priority === undefined || PRIORITIES.has(value.priority))
    && (value.payload === undefined || (isRecord(value.payload) && boundedJson(value.payload, 16 * 1024)));
}

function validWorkerId(value) {
  return typeof value === 'string' && /^[0-9A-F]{4}$/.test(value);
}

function validParams(method, params, agentId = COORD_AGENT_ID) {
  switch (method) {
    case 'tasks/list':
      return exactKeys(params, ['limit', 'kind', 'priority', 'state', 'skip_reclaim'], ['skip_reclaim'])
        && params.skip_reclaim === true
        && (params.limit === undefined || (Number.isInteger(params.limit) && params.limit >= 1 && params.limit <= 100))
        && (params.kind === undefined || (typeof params.kind === 'string' && params.kind.length <= 64))
        && (params.priority === undefined || PRIORITIES.has(params.priority))
        && (params.state === undefined || STATES.has(params.state));
    case 'tasks/get':
      return exactKeys(params, ['id'], ['id']) && validUuid(params.id);
    case 'agents/heartbeat':
      return exactKeys(params, ['id', 'name'], ['id', 'name'])
        && params.id === agentId
        && typeof params.name === 'string'
        && params.name.length > 0
        && params.name.length <= 64;
    case 'tasks/claim':
      return exactKeys(params, ['id', 'agentId', 'leaseSeconds'], ['id', 'agentId'])
        && validUuid(params.id)
        && params.agentId === agentId
        && validLeaseSeconds(params.leaseSeconds);
    case 'tasks/extend':
      return exactKeys(params, ['id', 'agentId', 'leaseToken', 'leaseSeconds'], ['id', 'agentId', 'leaseToken'])
        && validUuid(params.id)
        && params.agentId === agentId
        && validLeaseToken(params.leaseToken)
        && validLeaseSeconds(params.leaseSeconds);
    case 'tasks/complete':
      return exactKeys(params, ['id', 'agentId', 'leaseToken', 'result', 'postAck', 'ackName', 'ackPriority', 'ackPayload'], ['id', 'agentId', 'leaseToken', 'result'])
        && validUuid(params.id)
        && params.agentId === agentId
        && validLeaseToken(params.leaseToken)
        && validTaskResult(params.result)
        && validAck(params, 'postAck', 'ackName', 'ackPriority')
        && (params.ackPayload === undefined || (isRecord(params.ackPayload) && boundedJson(params.ackPayload, 16 * 1024)));
    case 'scheduler/claim':
      return exactKeys(params, ['agent_id', 'batch_id', 'lease_seconds'], ['agent_id'])
        && params.agent_id === agentId
        && (params.batch_id === undefined || validUuid(params.batch_id))
        && validLeaseSeconds(params.lease_seconds);
    case 'scheduler/extend':
      return exactKeys(params, ['id', 'agent_id', 'lease_token', 'lease_seconds'], ['id', 'agent_id', 'lease_token'])
        && validUuid(params.id)
        && params.agent_id === agentId
        && validLeaseToken(params.lease_token)
        && validLeaseSeconds(params.lease_seconds);
    case 'scheduler/progress':
      return exactKeys(params, ['id', 'agent_id', 'lease_token', 'attempt_no', 'sequence', 'request_id', 'checkpoint'], ['id', 'agent_id', 'lease_token', 'attempt_no', 'sequence', 'request_id', 'checkpoint'])
        && validUuid(params.id)
        && params.agent_id === agentId
        && validLeaseToken(params.lease_token)
        && Number.isInteger(params.attempt_no) && params.attempt_no >= 1 && params.attempt_no <= 1_000_000_000
        && Number.isInteger(params.sequence) && params.sequence >= 1 && params.sequence <= 1_000_000_000
        && validUuid(params.request_id)
        && typeof params.checkpoint === 'string' && params.checkpoint.trim().length > 0 && params.checkpoint.length <= 1024;
    case 'scheduler/extend_progress':
      return exactKeys(params, ['id', 'agent_id', 'lease_token', 'attempt_no', 'progress_sequence', 'request_id', 'lease_seconds'], ['id', 'agent_id', 'lease_token', 'attempt_no', 'progress_sequence', 'request_id', 'lease_seconds'])
        && validUuid(params.id)
        && params.agent_id === agentId
        && validLeaseToken(params.lease_token)
        && Number.isInteger(params.attempt_no) && params.attempt_no >= 1 && params.attempt_no <= 1_000_000_000
        && Number.isInteger(params.progress_sequence) && params.progress_sequence >= 1 && params.progress_sequence <= 1_000_000_000
        && validUuid(params.request_id)
        && validLeaseSeconds(params.lease_seconds);
    case 'scheduler/agent_status':
      return exactKeys(params, ['agent_id', 'batch_id'], ['agent_id', 'batch_id'])
        && params.agent_id === agentId
        && validUuid(params.batch_id);
    case 'scheduler/trusted_claim':
      return exactKeys(params, ['batch_id', 'worker_id', 'lease_seconds'], ['batch_id', 'worker_id'])
        && validUuid(params.batch_id) && validWorkerId(params.worker_id) && validLeaseSeconds(params.lease_seconds);
    case 'scheduler/trusted_progress':
      return exactKeys(params, ['task_id', 'batch_id', 'worker_id', 'attempt_no', 'checkpoint', 'lease_seconds'], ['task_id', 'batch_id', 'worker_id', 'attempt_no', 'checkpoint'])
        && validUuid(params.task_id) && validUuid(params.batch_id) && validWorkerId(params.worker_id)
        && Number.isInteger(params.attempt_no) && params.attempt_no >= 1 && params.attempt_no <= 1_000_000_000
        && typeof params.checkpoint === 'string' && params.checkpoint.trim().length > 0 && params.checkpoint.length <= 1024
        && validLeaseSeconds(params.lease_seconds);
    case 'scheduler/trusted_complete':
      return exactKeys(params, ['task_id', 'batch_id', 'worker_id', 'attempt_no', 'result', 'ack'], ['task_id', 'batch_id', 'worker_id', 'attempt_no', 'result'])
        && validUuid(params.task_id) && validUuid(params.batch_id) && validWorkerId(params.worker_id)
        && Number.isInteger(params.attempt_no) && params.attempt_no >= 1 && params.attempt_no <= 1_000_000_000
        && validTaskResult(params.result) && (params.ack === undefined || validTrustedAck(params.ack));
    case 'scheduler/trusted_release':
      return exactKeys(params, ['task_id', 'batch_id', 'worker_id', 'attempt_no', 'reason'], ['task_id', 'batch_id', 'worker_id', 'attempt_no'])
        && validUuid(params.task_id) && validUuid(params.batch_id) && validWorkerId(params.worker_id)
        && Number.isInteger(params.attempt_no) && params.attempt_no >= 1 && params.attempt_no <= 1_000_000_000
        && (params.reason === undefined || (typeof params.reason === 'string' && params.reason.length <= 1024));
    case 'scheduler/trusted_resume':
      return exactKeys(params, ['batch_id', 'worker_id'], ['batch_id', 'worker_id'])
        && validUuid(params.batch_id) && validWorkerId(params.worker_id);
    case 'scheduler/trusted_status':
      return exactKeys(params, ['batch_id'], ['batch_id']) && validUuid(params.batch_id);
    case 'scheduler/submit':
      return exactKeys(params, ['id', 'agent_id', 'lease_token', 'result', 'post_ack', 'ack_name', 'ack_priority', 'ack_payload'], ['id', 'agent_id', 'lease_token', 'result'])
        && validUuid(params.id)
        && params.agent_id === agentId
        && validLeaseToken(params.lease_token)
        && validTaskResult(params.result)
        && validAck(params, 'post_ack', 'ack_name', 'ack_priority')
        && (params.ack_payload === undefined || (isRecord(params.ack_payload) && boundedJson(params.ack_payload, 16 * 1024)));
    case 'scheduler/failure':
      return exactKeys(params, ['id', 'agent_id', 'lease_token', 'error'], ['id', 'agent_id', 'lease_token', 'error'])
        && validUuid(params.id)
        && params.agent_id === agentId
        && validLeaseToken(params.lease_token)
        && typeof params.error === 'string'
        && params.error.length > 0
        && params.error.length <= 4096;
    default:
      return false;
  }
}

export function validateRpcRequest(request, identityPolicy = null) {
  if (!exactKeys(request, ['requestId', 'method', 'params', 'identity'], ['requestId', 'method', 'params'])) {
    return { ok: false, code: 'invalid_request' };
  }
  if (!validUuid(request.requestId) || typeof request.method !== 'string') {
    return { ok: false, code: 'invalid_request' };
  }
  if (request.identity !== undefined && !isRecord(request.identity)) return { ok: false, code: 'identity_invalid' };
  const authorization = identity.verifyRequest(request, identityPolicy);
  if (!authorization.ok) return authorization;
  if (TRUSTED_METHODS.has(request.method) && identityPolicy?.mode === 'logical' && !request.identity) {
    return { ok: false, code: 'trusted_owner_identity_required' };
  }
  if (!validParams(request.method, request.params, authorization.agentId)) {
    return { ok: false, code: 'method_or_params_denied' };
  }
  if (['scheduler/progress', 'scheduler/extend_progress'].includes(request.method)
    && request.params.request_id !== request.requestId) {
    return { ok: false, code: 'request_id_mismatch' };
  }
  if (!boundedJson(request, MAX_REQUEST_BYTES)) {
    return { ok: false, code: 'request_too_large' };
  }
  return { ok: true };
}

export function makeAgentProof(secretHex, agentId, nonce) {
  return createHmac('sha256', Buffer.from(secretHex, 'hex'))
    .update(`coord-relay-v${PROTOCOL_VERSION}\0${agentId}\0${nonce}`)
    .digest('hex');
}

export function timingSafeHexEqual(expected, actual) {
  if (typeof expected !== 'string' || typeof actual !== 'string' || expected.length !== actual.length) return false;
  if (!/^[0-9a-f]+$/i.test(expected) || !/^[0-9a-f]+$/i.test(actual) || expected.length % 2 !== 0) return false;
  return timingSafeEqual(Buffer.from(expected, 'hex'), Buffer.from(actual, 'hex'));
}

export function normalizeIp(address) {
  if (typeof address !== 'string') return '';
  return address.startsWith('::ffff:') ? address.slice(7) : address;
}

export function redactForeignLeaseTokens(value, agentId = COORD_AGENT_ID, allowOwnToken = true) {
  if (Array.isArray(value)) return value.map(item => redactForeignLeaseTokens(item, agentId, allowOwnToken));
  if (!isRecord(value)) return value;
  const result = {};
  for (const [key, item] of Object.entries(value)) {
    if (['lease_token', 'leaseToken'].includes(key) && (!allowOwnToken || value.claimed_by !== agentId)) continue;
    result[key] = redactForeignLeaseTokens(item, agentId, allowOwnToken);
  }
  return result;
}

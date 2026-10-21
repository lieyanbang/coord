import { createHmac, timingSafeEqual } from 'node:crypto';

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

function validParams(method, params) {
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
        && params.id === COORD_AGENT_ID
        && typeof params.name === 'string'
        && params.name.length > 0
        && params.name.length <= 64;
    case 'tasks/claim':
      return exactKeys(params, ['id', 'agentId', 'leaseSeconds'], ['id', 'agentId'])
        && validUuid(params.id)
        && params.agentId === COORD_AGENT_ID
        && validLeaseSeconds(params.leaseSeconds);
    case 'tasks/extend':
      return exactKeys(params, ['id', 'agentId', 'leaseToken', 'leaseSeconds'], ['id', 'agentId', 'leaseToken'])
        && validUuid(params.id)
        && params.agentId === COORD_AGENT_ID
        && validLeaseToken(params.leaseToken)
        && validLeaseSeconds(params.leaseSeconds);
    case 'tasks/complete':
      return exactKeys(params, ['id', 'agentId', 'leaseToken', 'result', 'postAck', 'ackName', 'ackPriority', 'ackPayload'], ['id', 'agentId', 'leaseToken', 'result'])
        && validUuid(params.id)
        && params.agentId === COORD_AGENT_ID
        && validLeaseToken(params.leaseToken)
        && validTaskResult(params.result)
        && validAck(params, 'postAck', 'ackName', 'ackPriority')
        && (params.ackPayload === undefined || (isRecord(params.ackPayload) && boundedJson(params.ackPayload, 16 * 1024)));
    case 'scheduler/claim':
      return exactKeys(params, ['agent_id', 'batch_id', 'lease_seconds'], ['agent_id'])
        && params.agent_id === COORD_AGENT_ID
        && (params.batch_id === undefined || validUuid(params.batch_id))
        && validLeaseSeconds(params.lease_seconds);
    case 'scheduler/extend':
      return exactKeys(params, ['id', 'agent_id', 'lease_token', 'lease_seconds'], ['id', 'agent_id', 'lease_token'])
        && validUuid(params.id)
        && params.agent_id === COORD_AGENT_ID
        && validLeaseToken(params.lease_token)
        && validLeaseSeconds(params.lease_seconds);
    case 'scheduler/submit':
      return exactKeys(params, ['id', 'agent_id', 'lease_token', 'result', 'post_ack', 'ack_name', 'ack_priority', 'ack_payload'], ['id', 'agent_id', 'lease_token', 'result'])
        && validUuid(params.id)
        && params.agent_id === COORD_AGENT_ID
        && validLeaseToken(params.lease_token)
        && validTaskResult(params.result)
        && validAck(params, 'post_ack', 'ack_name', 'ack_priority')
        && (params.ack_payload === undefined || (isRecord(params.ack_payload) && boundedJson(params.ack_payload, 16 * 1024)));
    case 'scheduler/failure':
      return exactKeys(params, ['id', 'agent_id', 'lease_token', 'error'], ['id', 'agent_id', 'lease_token', 'error'])
        && validUuid(params.id)
        && params.agent_id === COORD_AGENT_ID
        && validLeaseToken(params.lease_token)
        && typeof params.error === 'string'
        && params.error.length > 0
        && params.error.length <= 4096;
    default:
      return false;
  }
}

export function validateRpcRequest(request) {
  if (!exactKeys(request, ['requestId', 'method', 'params'], ['requestId', 'method', 'params'])) {
    return { ok: false, code: 'invalid_request' };
  }
  if (!validUuid(request.requestId) || typeof request.method !== 'string') {
    return { ok: false, code: 'invalid_request' };
  }
  if (!validParams(request.method, request.params)) {
    return { ok: false, code: 'method_or_params_denied' };
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

export function redactForeignLeaseTokens(value) {
  if (Array.isArray(value)) return value.map(redactForeignLeaseTokens);
  if (!isRecord(value)) return value;
  const result = {};
  for (const [key, item] of Object.entries(value)) {
    if (key === 'lease_token' && value.claimed_by !== COORD_AGENT_ID) continue;
    result[key] = redactForeignLeaseTokens(item);
  }
  return result;
}

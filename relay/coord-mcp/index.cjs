const crypto = require('node:crypto');
const fs = require('node:fs/promises');
const path = require('node:path');
const { createRequire } = require('node:module');
const { pathToFileURL } = require('node:url');
const { createCoordOAuthServer } = require('./oauth.cjs');
const identity = require('../identity.cjs');

const COORD_AGENT_ID = 'coord-relay-agent';
const MAX_BODY_BYTES = 64 * 1024;
const MAX_RESULT_BYTES = 16 * 1024;
const MAX_ACK_PAYLOAD_BYTES = 16 * 1024;
const MAX_RESPONSE_BYTES = 64 * 1024;
const MAX_CONCURRENT = 4;
const RPC_TIMEOUT_MS = 12_000;
const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const TASK_STATES = new Set(['pending', 'claimed', 'submitted', 'completed', 'failed', 'cancelled']);
const PRIORITIES = new Set(['low', 'normal', 'high', 'urgent']);

function objectSchema(properties, required = []) {
  return { type: 'object', properties, required, additionalProperties: false };
}

function stringSchema(description, maxLength) {
  return { type: 'string', description, ...(maxLength ? { maxLength } : {}) };
}

function integerSchema(description, minimum, maximum) {
  return { type: 'integer', description, minimum, maximum };
}

const REQUEST_ID_PROPERTY = stringSchema('Unique UUID for this state-changing relay request. Reuse it unchanged when reconciling an uncertain outcome.', 36);
const TASK_ID_PROPERTY = stringSchema('coord task UUID.', 36);
const LEASE_TOKEN_PROPERTY = stringSchema('The lease token returned by this agent claim. Never substitute a token from another task or attempt.', 36);
const ATTEMPT_NO_PROPERTY = integerSchema('The attempt number returned by the exact current task claim.', 1, 1000000000);
const PROGRESS_SEQUENCE_PROPERTY = integerSchema('The next monotonically increasing checkpoint number for this task attempt.', 1, 1000000000);

const TOOLS = [
  {
    name: 'coord_tasks_list',
    description: 'List coord tasks without reclaiming expired leases. Lease tokens are always removed from this result.',
    inputSchema: objectSchema({
      limit: integerSchema('Maximum number of tasks to return.', 1, 100),
      kind: stringSchema('Optional task kind filter.', 64),
      priority: { type: 'string', enum: [...PRIORITIES] },
      state: { type: 'string', enum: [...TASK_STATES] },
    }),
    annotations: { title: 'List coord tasks', readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false },
  },
  {
    name: 'coord_get_task',
    description: 'Read one coord task. Lease tokens are always removed from this result.',
    inputSchema: objectSchema({ id: TASK_ID_PROPERTY }, ['id']),
    annotations: { title: 'Get coord task', readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false },
  },
  {
    name: 'coord_claim_task',
    description: 'Claim one explicitly selected task as the server-authenticated relay identity. A claim returns the current attempt lease token. State-changing calls require request_id; never automatically retry an uncertain outcome.',
    inputSchema: objectSchema({
      id: TASK_ID_PROPERTY,
      request_id: REQUEST_ID_PROPERTY,
      lease_seconds: integerSchema('Requested lease duration.', 1, 600),
    }, ['id', 'request_id']),
    annotations: { title: 'Claim coord task', readOnlyHint: false, destructiveHint: false, idempotentHint: false, openWorldHint: false },
  },
  {
    name: 'coord_claim_next',
    description: 'Claim the next task only from an explicitly named batch permitted by the server-authenticated relay identity; no agent ID can be selected.',
    inputSchema: objectSchema({
      batch_id: stringSchema('Required batch UUID. This tool never claims from the global queue.', 36),
      request_id: REQUEST_ID_PROPERTY,
      lease_seconds: integerSchema('Requested lease duration.', 1, 600),
    }, ['batch_id', 'request_id']),
    annotations: { title: 'Claim next batch task', readOnlyHint: false, destructiveHint: false, idempotentHint: false, openWorldHint: false },
  },
  {
    name: 'coord_extend_lease',
    description: 'Extend a legacy lease using its exact current token. Progress-required batches reject this method; use coord_report_progress then coord_extend_lease_progress. Do not retry an uncertain result.',
    inputSchema: objectSchema({
      id: TASK_ID_PROPERTY,
      lease_token: LEASE_TOKEN_PROPERTY,
      request_id: REQUEST_ID_PROPERTY,
      lease_seconds: integerSchema('Requested lease duration.', 1, 600),
    }, ['id', 'lease_token', 'request_id']),
    annotations: { title: 'Extend coord lease', readOnlyHint: false, destructiveHint: false, idempotentHint: false, openWorldHint: false },
  },
  {
    name: 'coord_report_progress',
    description: 'Record a short self-reported checkpoint for the current attempt. It is fenced by the lease token and attempt number; it is not proof of model liveness.',
    inputSchema: objectSchema({
      id: TASK_ID_PROPERTY,
      lease_token: LEASE_TOKEN_PROPERTY,
      attempt_no: ATTEMPT_NO_PROPERTY,
      sequence: PROGRESS_SEQUENCE_PROPERTY,
      request_id: REQUEST_ID_PROPERTY,
      checkpoint: stringSchema('Short concrete work completed since the previous checkpoint.', 1024),
    }, ['id', 'lease_token', 'attempt_no', 'sequence', 'request_id', 'checkpoint']),
    annotations: { title: 'Report coord progress', readOnlyHint: false, destructiveHint: false, idempotentHint: false, openWorldHint: false },
  },
  {
    name: 'coord_extend_lease_progress',
    description: 'Extend a progress-required lease after a fresh, unconsumed checkpoint for the same attempt. One request extends at most 600 seconds and the batch total budget is finite.',
    inputSchema: objectSchema({
      id: TASK_ID_PROPERTY,
      lease_token: LEASE_TOKEN_PROPERTY,
      attempt_no: ATTEMPT_NO_PROPERTY,
      progress_sequence: PROGRESS_SEQUENCE_PROPERTY,
      request_id: REQUEST_ID_PROPERTY,
      lease_seconds: integerSchema('Requested extension, capped at 600 seconds.', 1, 600),
    }, ['id', 'lease_token', 'attempt_no', 'progress_sequence', 'request_id']),
    annotations: { title: 'Extend coord lease with progress', readOnlyHint: false, destructiveHint: false, idempotentHint: false, openWorldHint: false },
  },
  {
    name: 'coord_agent_status',
    description: 'Read current tasks and last accepted progress for this relay identity within one batch. No lease tokens are returned.',
    inputSchema: objectSchema({ batch_id: stringSchema('Batch UUID.', 36) }, ['batch_id']),
    annotations: { title: 'Read coord agent status', readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false },
  },
  {
    name: 'coord_submit_result',
    description: 'Complete the task for this exact lease attempt. This is a terminal state change. Verify the result before submission and do not automatically retry an uncertain outcome.',
    inputSchema: objectSchema({
      id: TASK_ID_PROPERTY,
      lease_token: LEASE_TOKEN_PROPERTY,
      request_id: REQUEST_ID_PROPERTY,
      result: {},
      post_ack: { type: 'boolean' },
      ack_name: stringSchema('Required when post_ack is true.', 256),
      ack_priority: { type: 'string', enum: [...PRIORITIES] },
      ack_payload: { type: 'object', additionalProperties: true },
    }, ['id', 'lease_token', 'request_id', 'result']),
    annotations: { title: 'Submit coord result', readOnlyHint: false, destructiveHint: true, idempotentHint: false, openWorldHint: false },
  },
  {
    name: 'coord_report_failure',
    description: 'Report failure for the exact active lease attempt. This may consume an attempt and is a state change; do not automatically retry an uncertain outcome.',
    inputSchema: objectSchema({
      id: TASK_ID_PROPERTY,
      lease_token: LEASE_TOKEN_PROPERTY,
      request_id: REQUEST_ID_PROPERTY,
      error: stringSchema('Bounded failure detail.', 4096),
    }, ['id', 'lease_token', 'request_id', 'error']),
    annotations: { title: 'Report coord failure', readOnlyHint: false, destructiveHint: true, idempotentHint: false, openWorldHint: false },
  },
];

const TOOL_NAMES = new Set(TOOLS.map(({ name }) => name));

class RelayOutcomeError extends Error {
  constructor(state, code, requestId, statusCode = undefined) {
    super('Relay request did not return a confirmed successful result.');
    this.name = 'RelayOutcomeError';
    this.state = state;
    this.code = code || 'outcome_unknown';
    this.requestId = requestId;
    this.statusCode = statusCode;
  }
}

class ToolInputError extends Error {
  constructor(code) {
    super(code);
    this.name = 'ToolInputError';
    this.code = code;
  }
}

function isRecord(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function hasExactKeys(value, allowed, required = []) {
  if (!isRecord(value)) return false;
  const keys = Object.keys(value);
  return keys.every((key) => allowed.includes(key)) && required.every((key) => Object.hasOwn(value, key));
}

function jsonBytes(value) {
  try {
    return Buffer.byteLength(JSON.stringify(value));
  } catch {
    return Infinity;
  }
}

function validUuid(value) {
  return typeof value === 'string' && UUID_RE.test(value);
}

function boundedResult(value, maxBytes = MAX_RESULT_BYTES) {
  return value !== undefined && jsonBytes(value) <= maxBytes;
}

function validateToolInput(name, args) {
  if (!TOOL_NAMES.has(name) || !isRecord(args)) throw new ToolInputError('invalid_tool_or_arguments');

  switch (name) {
    case 'coord_tasks_list': {
      if (!hasExactKeys(args, ['limit', 'kind', 'priority', 'state'])) throw new ToolInputError('invalid_list_arguments');
      if (args.limit !== undefined && (!Number.isInteger(args.limit) || args.limit < 1 || args.limit > 100)) throw new ToolInputError('invalid_limit');
      if (args.kind !== undefined && (typeof args.kind !== 'string' || args.kind.length > 64)) throw new ToolInputError('invalid_kind');
      if (args.priority !== undefined && !PRIORITIES.has(args.priority)) throw new ToolInputError('invalid_priority');
      if (args.state !== undefined && !TASK_STATES.has(args.state)) throw new ToolInputError('invalid_state');
      return { method: 'tasks/list', requestId: crypto.randomUUID(), params: { ...args, skip_reclaim: true }, readOnly: true };
    }
    case 'coord_get_task': {
      if (!hasExactKeys(args, ['id'], ['id']) || !validUuid(args.id)) throw new ToolInputError('invalid_task_id');
      return { method: 'tasks/get', requestId: crypto.randomUUID(), params: { id: args.id }, readOnly: true };
    }
    case 'coord_claim_task': {
      if (!hasExactKeys(args, ['id', 'request_id', 'lease_seconds'], ['id', 'request_id'])
          || !validUuid(args.id) || !validUuid(args.request_id)
          || (args.lease_seconds !== undefined && (!Number.isInteger(args.lease_seconds) || args.lease_seconds < 1 || args.lease_seconds > 600))) {
        throw new ToolInputError('invalid_claim_arguments');
      }
      const params = { id: args.id, agentId: COORD_AGENT_ID };
      if (args.lease_seconds !== undefined) params.leaseSeconds = args.lease_seconds;
      return { method: 'tasks/claim', requestId: args.request_id, params, readOnly: false };
    }
    case 'coord_claim_next': {
      if (!hasExactKeys(args, ['batch_id', 'request_id', 'lease_seconds'], ['batch_id', 'request_id'])
          || !validUuid(args.batch_id) || !validUuid(args.request_id)
          || (args.lease_seconds !== undefined && (!Number.isInteger(args.lease_seconds) || args.lease_seconds < 1 || args.lease_seconds > 600))) {
        throw new ToolInputError('invalid_claim_arguments');
      }
      const params = { agent_id: COORD_AGENT_ID, batch_id: args.batch_id };
      if (args.lease_seconds !== undefined) params.lease_seconds = args.lease_seconds;
      return { method: 'scheduler/claim', requestId: args.request_id, params, readOnly: false };
    }
    case 'coord_extend_lease': {
      if (!hasExactKeys(args, ['id', 'lease_token', 'request_id', 'lease_seconds'], ['id', 'lease_token', 'request_id'])
          || !validUuid(args.id) || !validUuid(args.lease_token) || !validUuid(args.request_id)
          || (args.lease_seconds !== undefined && (!Number.isInteger(args.lease_seconds) || args.lease_seconds < 1 || args.lease_seconds > 600))) {
        throw new ToolInputError('invalid_extend_arguments');
      }
      const params = { id: args.id, agentId: COORD_AGENT_ID, leaseToken: args.lease_token };
      if (args.lease_seconds !== undefined) params.leaseSeconds = args.lease_seconds;
      return { method: 'tasks/extend', requestId: args.request_id, params, readOnly: false };
    }
    case 'coord_report_progress': {
      if (!hasExactKeys(args, ['id', 'lease_token', 'attempt_no', 'sequence', 'request_id', 'checkpoint'], ['id', 'lease_token', 'attempt_no', 'sequence', 'request_id', 'checkpoint'])
          || !validUuid(args.id) || !validUuid(args.lease_token) || !validUuid(args.request_id)
          || !Number.isInteger(args.attempt_no) || args.attempt_no < 1 || args.attempt_no > 1000000000
          || !Number.isInteger(args.sequence) || args.sequence < 1 || args.sequence > 1000000000
          || typeof args.checkpoint !== 'string' || args.checkpoint.trim().length < 1 || args.checkpoint.length > 1024) {
        throw new ToolInputError('invalid_progress_arguments');
      }
      return {
        method: 'scheduler/progress',
        requestId: args.request_id,
        params: {
          id: args.id,
          agent_id: COORD_AGENT_ID,
          lease_token: args.lease_token,
          attempt_no: args.attempt_no,
          sequence: args.sequence,
          request_id: args.request_id,
          checkpoint: args.checkpoint,
        },
        readOnly: false,
      };
    }
    case 'coord_extend_lease_progress': {
      if (!hasExactKeys(args, ['id', 'lease_token', 'attempt_no', 'progress_sequence', 'request_id', 'lease_seconds'], ['id', 'lease_token', 'attempt_no', 'progress_sequence', 'request_id'])
          || !validUuid(args.id) || !validUuid(args.lease_token) || !validUuid(args.request_id)
          || !Number.isInteger(args.attempt_no) || args.attempt_no < 1 || args.attempt_no > 1000000000
          || !Number.isInteger(args.progress_sequence) || args.progress_sequence < 1 || args.progress_sequence > 1000000000
          || (args.lease_seconds !== undefined && (!Number.isInteger(args.lease_seconds) || args.lease_seconds < 1 || args.lease_seconds > 600))) {
        throw new ToolInputError('invalid_progress_extend_arguments');
      }
      return {
        method: 'scheduler/extend_progress',
        requestId: args.request_id,
        params: {
          id: args.id,
          agent_id: COORD_AGENT_ID,
          lease_token: args.lease_token,
          attempt_no: args.attempt_no,
          progress_sequence: args.progress_sequence,
          request_id: args.request_id,
          lease_seconds: args.lease_seconds ?? 300,
        },
        readOnly: false,
      };
    }
    case 'coord_agent_status': {
      if (!hasExactKeys(args, ['batch_id'], ['batch_id']) || !validUuid(args.batch_id)) {
        throw new ToolInputError('invalid_agent_status_arguments');
      }
      return {
        method: 'scheduler/agent_status',
        requestId: crypto.randomUUID(),
        params: { agent_id: COORD_AGENT_ID, batch_id: args.batch_id },
        readOnly: true,
      };
    }
    case 'coord_submit_result': {
      const allowed = ['id', 'lease_token', 'request_id', 'result', 'post_ack', 'ack_name', 'ack_priority', 'ack_payload'];
      if (!hasExactKeys(args, allowed, ['id', 'lease_token', 'request_id', 'result'])
          || !validUuid(args.id) || !validUuid(args.lease_token) || !validUuid(args.request_id)
          || !boundedResult(args.result)) throw new ToolInputError('invalid_submit_arguments');
      if (args.post_ack !== undefined && typeof args.post_ack !== 'boolean') throw new ToolInputError('invalid_ack');
      if (args.post_ack === true && (typeof args.ack_name !== 'string' || args.ack_name.length < 1 || args.ack_name.length > 256)) throw new ToolInputError('invalid_ack_name');
      if (args.ack_name !== undefined && (typeof args.ack_name !== 'string' || args.ack_name.length > 256)) throw new ToolInputError('invalid_ack_name');
      if (args.ack_priority !== undefined && !PRIORITIES.has(args.ack_priority)) throw new ToolInputError('invalid_ack_priority');
      if (args.ack_payload !== undefined && (!isRecord(args.ack_payload) || jsonBytes(args.ack_payload) > MAX_ACK_PAYLOAD_BYTES)) throw new ToolInputError('invalid_ack_payload');
      const params = { id: args.id, agentId: COORD_AGENT_ID, leaseToken: args.lease_token, result: args.result };
      if (args.post_ack !== undefined) params.postAck = args.post_ack;
      if (args.ack_name !== undefined) params.ackName = args.ack_name;
      if (args.ack_priority !== undefined) params.ackPriority = args.ack_priority;
      if (args.ack_payload !== undefined) params.ackPayload = args.ack_payload;
      return { method: 'tasks/complete', requestId: args.request_id, params, readOnly: false };
    }
    case 'coord_report_failure': {
      if (!hasExactKeys(args, ['id', 'lease_token', 'request_id', 'error'], ['id', 'lease_token', 'request_id', 'error'])
          || !validUuid(args.id) || !validUuid(args.lease_token) || !validUuid(args.request_id)
          || typeof args.error !== 'string' || args.error.length < 1 || args.error.length > 4096) {
        throw new ToolInputError('invalid_failure_arguments');
      }
      return {
        method: 'scheduler/failure',
        requestId: args.request_id,
        params: { id: args.id, agent_id: COORD_AGENT_ID, lease_token: args.lease_token, error: args.error },
        readOnly: false,
      };
    }
    default:
      throw new ToolInputError('invalid_tool_or_arguments');
  }
}

function removeLeaseTokens(value) {
  if (Array.isArray(value)) return value.map(removeLeaseTokens);
  if (!isRecord(value)) return value;
  const result = {};
  for (const [key, item] of Object.entries(value)) {
    if (key === 'lease_token' || key === 'leaseToken') continue;
    result[key] = removeLeaseTokens(item);
  }
  return result;
}

async function readBoundedBody(response, maxBytes) {
  if (!response.body) return '';
  const reader = response.body.getReader();
  const chunks = [];
  let total = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      total += value.byteLength;
      if (total > maxBytes) {
        await reader.cancel().catch(() => undefined);
        throw new Error('response_too_large');
      }
      chunks.push(Buffer.from(value));
    }
  } finally {
    reader.releaseLock();
  }
  return Buffer.concat(chunks, total).toString('utf8');
}

class SidecarClient {
  constructor(options = {}) {
    this.url = options.url || 'http://127.0.0.1:13013/v1/rpc';
    this.secretFile = options.secretFile || '/etc/coord-relay/client.secret';
    this.fetch = options.fetch || globalThis.fetch;
    this.timeoutMs = options.timeoutMs ?? RPC_TIMEOUT_MS;
    this.maxPending = options.maxPending ?? MAX_CONCURRENT;
    this.pending = 0;
    this.secret = null;
  }

  async init() {
    const parsed = new URL(this.url);
    if (parsed.protocol !== 'http:' || parsed.hostname !== '127.0.0.1' || parsed.pathname !== '/v1/rpc') {
      throw new Error('Coord Sidecar URL must be the local /v1/rpc endpoint.');
    }
    if (typeof this.fetch !== 'function') throw new Error('Fetch is unavailable.');
    this.secret = (await fs.readFile(this.secretFile, 'utf8')).trim();
    if (!/^[0-9a-f]{64}$/i.test(this.secret)) throw new Error('Coord Sidecar credential has an invalid format.');
  }

  async call(rpc) {
    if (!this.secret) throw new RelayOutcomeError('not_dispatched', 'client_not_ready', rpc.requestId);
    if (this.pending >= this.maxPending) throw new RelayOutcomeError('not_dispatched', 'concurrency_limit', rpc.requestId, 429);
    const body = JSON.stringify(rpc);
    if (Buffer.byteLength(body) > MAX_BODY_BYTES) throw new RelayOutcomeError('not_dispatched', 'request_too_large', rpc.requestId, 413);

    this.pending += 1;
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeoutMs);
    try {
      let response;
      try {
        response = await this.fetch(this.url, {
          method: 'POST',
          headers: {
            authorization: `Bearer ${this.secret}`,
            'content-type': 'application/json',
            accept: 'application/json',
          },
          body,
          signal: controller.signal,
        });
      } catch {
        throw new RelayOutcomeError('uncertain', 'transport_outcome_unknown', rpc.requestId);
      }

      let payload;
      try {
        const text = await readBoundedBody(response, MAX_RESPONSE_BYTES);
        payload = JSON.parse(text);
      } catch (error) {
        if (error.message === 'response_too_large') throw new RelayOutcomeError('uncertain', 'response_too_large', rpc.requestId, response.status);
        throw new RelayOutcomeError('uncertain', 'invalid_sidecar_response', rpc.requestId, response.status);
      }
      if (response.status === 200 && payload?.state === 'ok' && Object.hasOwn(payload, 'result')) return payload.result;
      if (['uncertain', 'not_dispatched', 'rpc_error'].includes(payload?.state)) {
        const code = payload.code || payload.error?.code || 'outcome_unknown';
        throw new RelayOutcomeError(payload.state, code, rpc.requestId, response.status);
      }
      throw new RelayOutcomeError('uncertain', 'invalid_sidecar_response', rpc.requestId, response.status);
    } finally {
      clearTimeout(timer);
      this.pending -= 1;
    }
  }
}

function defaultSdkLoader(resolutionBase = path.join(__dirname, 'package.json')) {
  const resolver = createRequire(resolutionBase);
  const load = (specifier) => import(pathToFileURL(resolver.resolve(specifier)).href);
  return async () => {
    const [server, types, transport] = await Promise.all([
      load('@modelcontextprotocol/sdk/server/index.js'),
      load('@modelcontextprotocol/sdk/types.js'),
      load('@modelcontextprotocol/sdk/server/streamableHttp.js'),
    ]);
    return { server, types, transport };
  };
}

function toolErrorText(error) {
  if (error instanceof ToolInputError) return `Request rejected: ${error.code}.`;
  if (error instanceof RelayOutcomeError) {
    const prefix = error.state === 'uncertain'
      ? 'Outcome uncertain; this request was not retried. Inspect the task state before deciding what to do next.'
      : error.state === 'not_dispatched'
        ? 'Request was not dispatched by the relay.'
        : 'coord rejected the request.';
    return `${prefix} code=${error.code} request_id=${error.requestId}`;
  }
  return 'Coord MCP operation failed; no success is being reported.';
}

function createCoordMcpServer(authInfo, sidecar, loadSdk, identityPolicy = null) {
  return (async () => {
    const { server: sdk, types } = await loadSdk();
    const instructions = [
      'This endpoint exposes only the listed coord task operations. It cannot create or cancel tasks or batches, and it does not forward arbitrary JSON-RPC.',
      'Read task details before acting. List/get results never contain lease tokens; use only the token returned by this client claim for the same task attempt.',
      'State-changing calls require a caller-supplied UUID request_id. On an uncertain result, do not retry automatically; inspect task state before deciding whether a new request is appropriate.',
    ].join(' ');
    const server = new sdk.Server(
      { name: 'coord-mcp', version: '1.0.0' },
      { capabilities: { tools: {} } },
    );

    server.setRequestHandler(types.InitializeRequestSchema, async (request) => {
      const requested = request.params?.protocolVersion;
      const protocolVersion = types.SUPPORTED_PROTOCOL_VERSIONS.includes(requested)
        ? requested
        : types.LATEST_PROTOCOL_VERSION;
      return {
        protocolVersion,
        capabilities: { tools: {} },
        serverInfo: { name: 'coord-mcp', version: '1.0.0' },
        instructions,
      };
    });
    server.setRequestHandler(types.ListToolsRequestSchema, async () => ({ tools: TOOLS }));
    server.setRequestHandler(types.CallToolRequestSchema, async (request) => {
      const name = request.params?.name;
      if (!TOOL_NAMES.has(name)) throw new Error('Unknown Coord MCP tool.');
      let call;
      try {
        call = validateToolInput(name, request.params.arguments || {});
      } catch (error) {
        return { isError: true, content: [{ type: 'text', text: toolErrorText(error) }] };
      }
      try {
        const principal = identity.resolvePrincipal(identityPolicy, authInfo);
        if (principal) {
          if (Object.hasOwn(call.params, 'agentId')) call.params.agentId = principal.agent_id;
          if (Object.hasOwn(call.params, 'agent_id')) call.params.agent_id = principal.agent_id;
        }
        const rpc = identity.signRequest({ requestId: call.requestId, method: call.method, params: call.params }, principal, identityPolicy);
        const authorization = identity.verifyRequest(rpc, identityPolicy);
        if (!authorization.ok) throw new ToolInputError(authorization.code);
        const result = await sidecar.call(rpc);
        const visible = call.readOnly ? removeLeaseTokens(result) : result;
        const text = JSON.stringify(visible);
        if (Buffer.byteLength(text) > MAX_RESPONSE_BYTES) {
          return { isError: true, content: [{ type: 'text', text: 'Coord result exceeded the response limit.' }] };
        }
        return { content: [{ type: 'text', text }] };
      } catch (error) {
        return { isError: true, content: [{ type: 'text', text: toolErrorText(error) }] };
      }
    });
    return server;
  })();
}

function oauthConfig(options) {
  const issuer = options.issuer || 'https://lieyanbang.com/app/CoordMCP';
  const allowedEmails = options.accessAllowedEmails
    ?? process.env.COORD_MCP_ACCESS_ALLOWED_EMAILS?.split(',').map((value) => value.trim()).filter(Boolean)
    ?? [];
  return {
    issuer,
    resource: `${issuer}/`,
    authorizationOrigin: options.authorizationOrigin,
    accessTeamDomain: options.accessTeamDomain ?? process.env.COORD_MCP_ACCESS_TEAM_DOMAIN ?? '',
    accessApplicationAud: options.accessApplicationAud ?? process.env.COORD_MCP_ACCESS_APPLICATION_AUD ?? '',
    accessAllowedEmails: allowedEmails,
    oauthStatePath: options.oauthStatePath || '/var/lib/coord-mcp/oauth.json',
    oauthDiagnosticLogPath: options.oauthDiagnosticLogPath || '/var/lib/coord-mcp/oauth-register.jsonl',
    oauthAccessTokenTtlSeconds: options.oauthAccessTokenTtlSeconds || 900,
    oauthRefreshTokenTtlSeconds: options.oauthRefreshTokenTtlSeconds || 2_592_000,
  };
}

async function registerCoordMcp(httpApp, options = {}) {
  const express = options.express || require('express');
  const oauthModule = options.oauthModule || require('../desktop-commander/oauth.cjs');
  const config = oauthConfig(options);
  const oauth = options.oauth || createCoordOAuthServer(oauthModule.OAuthServer, config, {
    oauthRoutePrefix: '/coord/oauth',
    authorizationFormActionPrefix: `${config.issuer}/oauth`,
    protectedResourceMetadataUrl: `${config.issuer}/.well-known/oauth-protected-resource/mcp`,
    accessJwksProvider: options.accessJwksProvider,
  });
  const sidecar = options.sidecar || new SidecarClient(options.sidecarOptions);
  const loadSdk = options.loadSdk || defaultSdkLoader(options.sdkResolutionBase);
  const identityPolicy = options.identityPolicy ?? await identity.loadPolicy(options.identityPolicyFile || process.env.COORD_MCP_IDENTITY_POLICY);
  if (typeof oauth.init === 'function' && options.skipOAuthInit !== true) {
    const privateDirectories = new Set([
      path.dirname(config.oauthStatePath),
      path.dirname(config.oauthDiagnosticLogPath),
    ]);
    for (const directory of privateDirectories) {
      if (!path.isAbsolute(directory)) throw new Error('Coord MCP OAuth state paths must be absolute.');
      await fs.mkdir(directory, { recursive: true, mode: 0o700 });
      if (process.platform !== 'win32') await fs.chmod(directory, 0o700);
    }
    await oauth.init();
  }
  if (typeof sidecar.init === 'function') await sidecar.init();

  oauthModule.installOAuthRoutes(httpApp, express, oauth, '/coord/oauth', 'coord');
  const route = '/coord/mcp';
  let activeRequests = 0;

  httpApp.options(route, (req, res) => {
    oauth.setCors(res);
    res.status(204).end();
  });

  const acquireRequestSlot = (req, res, next) => {
    if (activeRequests >= MAX_CONCURRENT) {
      oauth.setCors(res);
      res.set('Retry-After', '1').status(429).json({ error: 'concurrency_limit' });
      return;
    }
    activeRequests += 1;
    let released = false;
    const release = () => {
      if (released) return;
      released = true;
      activeRequests -= 1;
    };
    res.once('finish', release);
    res.once('close', release);
    next();
  };
  const parseJson = express.json({ limit: MAX_BODY_BYTES, strict: true, type: 'application/json' });
  const handleJsonError = (error, req, res, next) => {
    if (error?.type === 'entity.too.large') {
      oauth.setCors(res);
      res.status(413).json({ error: 'request_too_large' });
      return;
    }
    if (error?.type === 'entity.parse.failed') {
      oauth.setCors(res);
      res.status(400).json({ error: 'invalid_json' });
      return;
    }
    next(error);
  };
  const authenticate = async (req, res, next) => {
    oauth.setCors(res);
    res.set('Cache-Control', 'no-store');
    let authInfo;
    try {
      authInfo = await oauth.authenticateRequest(req);
    } catch {
      authInfo = null;
    }
    if (!authInfo) {
      oauth.challenge(res, Boolean(req.get('authorization')));
      return;
    }
    req.coordMcpAuthInfo = authInfo;
    try {
      identity.resolvePrincipal(identityPolicy, authInfo);
    } catch {
      res.status(403).json({ error: 'identity_binding_missing_or_expired' });
      return;
    }
    next();
  };

  httpApp.post(route, acquireRequestSlot, parseJson, handleJsonError, authenticate, async (req, res) => {
    if (!req.is('application/json')) {
      res.status(415).json({ error: 'application_json_required' });
      return;
    }
    if (jsonBytes(req.body) > MAX_BODY_BYTES) {
      res.status(413).json({ error: 'request_too_large' });
      return;
    }

    let server;
    let transport;
    try {
      const sdk = await loadSdk();
      server = await createCoordMcpServer(req.coordMcpAuthInfo, sidecar, async () => sdk, identityPolicy);
      transport = new sdk.transport.StreamableHTTPServerTransport({ sessionIdGenerator: undefined });
      await server.connect(transport);
      await transport.handleRequest(req, res, req.body);
    } catch {
      if (res.headersSent) res.end();
      else res.status(500).json({ error: 'coord_mcp_internal_error' });
    } finally {
      await server?.close().catch(() => undefined);
      await transport?.close().catch(() => undefined);
    }
  });

  httpApp.get(route, async (req, res) => {
    oauth.setCors(res);
    res.set('Cache-Control', 'no-store');
    let authInfo;
    try {
      authInfo = await oauth.authenticateRequest(req);
    } catch {
      authInfo = null;
    }
    if (!authInfo) {
      oauth.challenge(res, Boolean(req.get('authorization')));
      return;
    }
    res.set('Allow', 'POST').status(405).json({ error: 'method_not_allowed', message: 'Use POST for this stateless MCP endpoint.' });
  });

  return { oauth, sidecar, shutdown: async () => undefined };
}

module.exports = {
  COORD_AGENT_ID,
  MAX_BODY_BYTES,
  MAX_CONCURRENT,
  MAX_RESPONSE_BYTES,
  RelayOutcomeError,
  SidecarClient,
  TOOLS,
  createCoordMcpServer,
  defaultSdkLoader,
  registerCoordMcp,
  removeLeaseTokens,
  validateToolInput,
};

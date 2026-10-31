import { createConnection, isIP } from 'node:net';
import { readFile, stat } from 'node:fs/promises';
import { isAbsolute, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  MAX_FRAME_BYTES,
  MAX_REQUEST_BYTES,
  MAX_RESPONSE_BYTES,
  PROTOCOL_VERSION,
  RELAY_AGENT_ID,
  makeAgentProof,
  redactForeignLeaseTokens,
  validateRpcRequest,
} from './protocol.mjs';
import identity from './identity.cjs';

function log(event, fields = {}) {
  process.stdout.write(`${JSON.stringify({ time: new Date().toISOString(), component: 'coord-relay-agent', event, ...fields })}\n`);
}

function writeFrame(socket, value) {
  const frame = Buffer.from(`${JSON.stringify(value)}\n`, 'utf8');
  if (frame.length > MAX_FRAME_BYTES) return false;
  return socket.write(frame);
}

function attachLineReader(socket, onLine, onInvalid, maxBytes = MAX_FRAME_BYTES) {
  let partial = Buffer.alloc(0);
  socket.on('data', chunk => {
    let start = 0;
    while (start < chunk.length) {
      const newline = chunk.indexOf(10, start);
      if (newline < 0) {
        const tail = chunk.subarray(start);
        if (partial.length + tail.length > maxBytes) return onInvalid('frame_too_large');
        partial = Buffer.concat([partial, tail]);
        return;
      }
      const piece = chunk.subarray(start, newline);
      const line = partial.length ? Buffer.concat([partial, piece]) : piece;
      partial = Buffer.alloc(0);
      if (!line.length || line.length > maxBytes) return onInvalid('invalid_frame_size');
      try {
        onLine(JSON.parse(line.toString('utf8')));
      } catch {
        return onInvalid('invalid_json');
      }
      start = newline + 1;
    }
  });
  socket.on('end', () => {
    if (partial.length) onInvalid('truncated_frame');
  });
}

function exactKeys(value, allowed, required = []) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
  const keys = Object.keys(value);
  return keys.every(key => allowed.includes(key)) && required.every(key => Object.hasOwn(value, key));
}

async function readBounded(response, maxBytes) {
  const declared = Number(response.headers.get('content-length') || 0);
  if (declared > maxBytes) throw new Error('response_too_large');
  const reader = response.body?.getReader();
  if (!reader) return '';
  const chunks = [];
  let total = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    total += value.byteLength;
    if (total > maxBytes) {
      await reader.cancel();
      throw new Error('response_too_large');
    }
    chunks.push(Buffer.from(value));
  }
  return Buffer.concat(chunks, total).toString('utf8');
}

export async function createAgent(options = {}) {
  const config = {
    serverHost: options.serverHost ?? '100.116.134.114',
    serverPort: options.serverPort ?? 13012,
    agentId: options.agentId ?? RELAY_AGENT_ID,
    coordUrl: options.coordUrl ?? 'http://127.0.0.1:7777/',
    secret: options.secret,
    heartbeatMs: options.heartbeatMs ?? 10_000,
    reconnectMinMs: options.reconnectMinMs ?? 1_000,
    reconnectMaxMs: options.reconnectMaxMs ?? 30_000,
    coordTimeoutMs: options.coordTimeoutMs ?? 8_000,
    maxInflight: options.maxInflight ?? 4,
    identityPolicy: options.identityPolicy ?? await identity.loadPolicy(options.identityPolicyFile),
  };
  if (options.secretFile) config.secret = (await readFile(options.secretFile, 'utf8')).trim();
  if (typeof config.secret !== 'string' || !/^[0-9a-f]{64}$/i.test(config.secret)) {
    throw new Error('agent secret must be a 32-byte hex secret');
  }
  if (!isIP(config.serverHost) || !Number.isInteger(config.serverPort) || config.serverPort < 1 || config.serverPort > 65535) {
    throw new Error('server endpoint must be a literal IP and valid port');
  }
  const coordUrl = new URL(config.coordUrl);
  if (coordUrl.protocol !== 'http:' || coordUrl.hostname !== '127.0.0.1' || coordUrl.pathname !== '/') {
    throw new Error('coordUrl must use the local IPv4 JSON-RPC endpoint');
  }
  if (options.coordAuthKeyFile !== undefined) {
    if (typeof options.coordAuthKeyFile !== 'string' || !isAbsolute(options.coordAuthKeyFile)) {
      throw new Error('coordAuthKeyFile must be an absolute protected file path');
    }
    if ((await stat(options.coordAuthKeyFile)).size > 128) throw new Error('coord RPC key file is too large');
    config.coordAuthKey = (await readFile(options.coordAuthKeyFile, 'utf8')).trim();
    if (!/^[0-9a-f]{64}$/i.test(config.coordAuthKey)) throw new Error('coord RPC key must be a 32-byte hex secret');
  }
  if (!Number.isInteger(config.maxInflight) || config.maxInflight < 1 || config.maxInflight > 4) throw new Error('maxInflight must be 1..4');
  if (!Number.isInteger(config.coordTimeoutMs) || config.coordTimeoutMs < 100 || config.coordTimeoutMs > 60_000) throw new Error('coordTimeoutMs must be 100..60000');
  return new RelayAgent(config);
}

class RelayAgent {
  constructor(config) {
    this.config = config;
    this.stopped = true;
    this.socket = null;
    this.runPromise = null;
    this.inflight = new Map();
    this.calls = new Set();
    this.recentIds = new Set();
    this.connectedWaiters = new Set();
  }

  start() {
    if (!this.stopped) return;
    this.stopped = false;
    this.runPromise = this.run();
  }

  async waitUntilConnected(timeoutMs = 5_000) {
    if (this.socket?.authenticated) return true;
    return new Promise(resolve => {
      const timer = setTimeout(() => {
        this.connectedWaiters.delete(onConnected);
        resolve(false);
      }, timeoutMs);
      const onConnected = () => {
        clearTimeout(timer);
        this.connectedWaiters.delete(onConnected);
        resolve(true);
      };
      this.connectedWaiters.add(onConnected);
    });
  }

  async stop() {
    this.stopped = true;
    this.socket?.destroy();
    for (const controller of this.inflight.values()) controller.abort();
    await this.runPromise;
    await Promise.allSettled(this.calls);
  }

  async run() {
    let delay = this.config.reconnectMinMs;
    while (!this.stopped) {
      try {
        await this.connectOnce();
        delay = this.config.reconnectMinMs;
      } catch {
        log('connection_failed', { code: 'connect_or_protocol_error' });
      }
      if (!this.stopped) await new Promise(resolve => setTimeout(resolve, delay));
      delay = Math.min(delay * 2, this.config.reconnectMaxMs);
    }
  }

  connectOnce() {
    return new Promise((resolve, reject) => {
      const socket = createConnection({ host: this.config.serverHost, port: this.config.serverPort });
      this.socket = socket;
      socket.setNoDelay(true);
      let challenged = false;
      let authenticated = false;
      let settled = false;
      let heartbeat;
      const finish = () => {
        if (settled) return;
        settled = true;
        clearInterval(heartbeat);
        if (this.socket === socket) this.socket = null;
        resolve();
      };
      socket.setTimeout(15_000, () => socket.destroy());
      socket.once('connect', () => {
        socket.setTimeout(0);
      });
      socket.once('error', error => {
        if (!authenticated) reject(error);
        else finish();
      });
      socket.once('close', finish);
      attachLineReader(socket, message => {
        if (!challenged) {
          if (!exactKeys(message, ['type', 'version', 'nonce'], ['type', 'version', 'nonce'])
            || message.type !== 'challenge'
            || message.version !== PROTOCOL_VERSION
            || typeof message.nonce !== 'string'
            || !/^[A-Za-z0-9_-]{43}$/.test(message.nonce)) {
            socket.destroy();
            return;
          }
          challenged = true;
          const proof = makeAgentProof(this.config.secret, this.config.agentId, message.nonce);
          if (!writeFrame(socket, { type: 'auth', version: PROTOCOL_VERSION, agentId: this.config.agentId, proof })) {
            socket.destroy();
          }
          return;
        }
        if (!authenticated && message?.type === 'auth_ok' && message.version === PROTOCOL_VERSION) {
          authenticated = true;
          socket.authenticated = true;
          log('connected', { serverHost: this.config.serverHost, serverPort: this.config.serverPort });
          for (const wake of this.connectedWaiters) wake();
          heartbeat = setInterval(() => {
            if (!writeFrame(socket, { type: 'heartbeat', agentId: this.config.agentId })) socket.destroy();
          }, this.config.heartbeatMs);
          return;
        }
        if (!authenticated) {
          socket.destroy();
          return;
        }
        if (message?.type === 'call') {
          const call = this.handleCall(socket, message);
          this.calls.add(call);
          void call.finally(() => this.calls.delete(call));
          return;
        }
        if (message?.type !== 'heartbeat_ack') socket.destroy();
      }, () => socket.destroy());
    });
  }

  async handleCall(socket, message) {
    const request = { requestId: message.requestId, method: message.method, params: message.params,
      ...(message.identity ? { identity: message.identity } : {}) };
    const validation = validateRpcRequest(request, this.config.identityPolicy);
    if (!validation.ok) {
      writeFrame(socket, { type: 'result', requestId: message.requestId, outcome: { state: 'not_dispatched', code: validation.code } });
      return;
    }
    if (this.recentIds.has(request.requestId)) {
      writeFrame(socket, { type: 'result', requestId: request.requestId, outcome: { state: 'uncertain', code: 'duplicate_delivery' } });
      return;
    }
    if (this.inflight.size >= this.config.maxInflight) {
      writeFrame(socket, { type: 'result', requestId: request.requestId, outcome: { state: 'not_dispatched', code: 'agent_concurrency_limit' } });
      return;
    }
    this.recentIds.add(request.requestId);
    if (this.recentIds.size > 10_000) this.recentIds.delete(this.recentIds.values().next().value);
    const controller = new AbortController();
    this.inflight.set(request.requestId, controller);
    try {
      const response = await this.callCoord(request, controller.signal);
      writeFrame(socket, { type: 'result', requestId: request.requestId, outcome: response });
    } catch (error) {
      const code = error.message === 'response_too_large' ? 'coord_response_too_large' : 'coord_outcome_unknown';
      writeFrame(socket, { type: 'result', requestId: request.requestId, outcome: { state: 'uncertain', code } });
    } finally {
      this.inflight.delete(request.requestId);
    }
  }

  async callCoord(request, shutdownSignal) {
    const policy = this.config.identityPolicy;
    const auth = identity.verifyRequest(request, policy);
    if (!auth.ok) return { state: 'not_dispatched', code: auth.code };
    if (policy) {
      if (!request.identity && request.method === 'scheduler/claim' && !request.params.batch_id) {
        return { state: 'not_dispatched', code: 'identity_batch_required' };
      }
      if (!request.identity && identity.protectedBatch(policy, request.params.batch_id)) {
        return { state: 'not_dispatched', code: 'identity_batch_denied' };
      }
      if (request.params.id && request.method !== 'agents/heartbeat') {
        // batch_id is immutable in coord. Resolve the exact task on the trusted
        // loopback endpoint before allowing a scoped read or fenced mutation.
        const checked = await this.dispatchCoord({ requestId: request.requestId, method: 'tasks/get', params: { id: request.params.id } }, shutdownSignal);
        if (checked.state !== 'ok') return { state: 'not_dispatched', code: 'identity_task_lookup_failed' };
        const task = checked.response.result;
        if ((request.identity && task?.batch_id !== auth.batchId)
            || (!request.identity && identity.protectedBatch(policy, task?.batch_id))) {
          return { state: 'not_dispatched', code: 'identity_batch_denied' };
        }
      }
      const fresh = identity.verifyRequest(request, policy);
      if (!fresh.ok) return { state: 'not_dispatched', code: fresh.code };
    }
    const outcome = await this.dispatchCoord(request, shutdownSignal);
    if (request.identity && request.method === 'tasks/list' && outcome.state === 'ok') {
      const result = outcome.response.result;
      if (!Array.isArray(result) || result.some(task => task?.batch_id !== auth.batchId)) {
        return { state: 'not_dispatched', code: 'identity_list_scope_mismatch' };
      }
    }
    if (policy && !request.identity && request.method === 'tasks/list' && outcome.state === 'ok') {
      const result = outcome.response.result;
      if (!Array.isArray(result)) return { state: 'uncertain', code: 'invalid_coord_response' };
      outcome.response.result = result.filter(task => !identity.protectedBatch(policy, task.batch_id));
    }
    return outcome;
  }

  async dispatchCoord(request, shutdownSignal) {
    const body = JSON.stringify({ jsonrpc: '2.0', id: request.requestId, method: request.method, params: request.params });
    if (Buffer.byteLength(body) > MAX_REQUEST_BYTES) return { state: 'not_dispatched', code: 'request_too_large' };
    const abort = AbortSignal.any([AbortSignal.timeout(this.config.coordTimeoutMs), shutdownSignal]);
    let response;
    try {
      response = await fetch(this.config.coordUrl, {
        method: 'POST',
        headers: { 'content-type': 'application/json',
          ...(this.config.coordAuthKey ? { authorization: `Bearer ${this.config.coordAuthKey}` } : {}) },
        body,
        signal: abort,
        redirect: 'error',
      });
    } catch {
      return { state: 'uncertain', code: 'coord_outcome_unknown' };
    }
    if (!response.ok) return { state: 'uncertain', code: 'coord_http_error' };
    let payload;
    try {
      payload = JSON.parse(await readBounded(response, MAX_RESPONSE_BYTES));
    } catch (error) {
      if (error.message === 'response_too_large') throw error;
      return { state: 'uncertain', code: 'invalid_coord_response' };
    }
    if (payload?.jsonrpc !== '2.0' || payload.id !== request.requestId
      || (Object.hasOwn(payload, 'result') === Object.hasOwn(payload, 'error'))) {
      return { state: 'uncertain', code: 'invalid_coord_response' };
    }
    if (Object.hasOwn(payload, 'error')) {
      return { state: 'rpc_error', error: { code: payload.error.code, message: String(payload.error.message).slice(0, 512) } };
    }
    const readOnlyBound = request.identity && ['tasks/get', 'scheduler/agent_status'].includes(request.method);
    const sanitized = redactForeignLeaseTokens(payload.result, request.identity?.agent_id, !readOnlyBound);
    if (Buffer.byteLength(JSON.stringify(sanitized)) > MAX_RESPONSE_BYTES) {
      return { state: 'uncertain', code: 'coord_response_too_large' };
    }
    return { state: 'ok', response: { jsonrpc: '2.0', id: request.requestId, result: sanitized } };
  }
}

async function main() {
  const configPath = process.argv[process.argv.indexOf('--config') + 1];
  if (!configPath) throw new Error('usage: node agent.mjs --config <path>');
  const config = JSON.parse(await readFile(configPath, 'utf8'));
  const agent = await createAgent({ ...config, secretFile: config.secretFile });
  agent.start();
  process.once('SIGINT', () => void agent.stop().then(() => process.exit(0)));
  process.once('SIGTERM', () => void agent.stop().then(() => process.exit(0)));
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  main().catch(() => {
    log('fatal', { code: 'startup_failed' });
    process.exit(1);
  });
}

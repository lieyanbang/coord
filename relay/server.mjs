import { createHmac, randomBytes } from 'node:crypto';
import { createServer as createHttpServer } from 'node:http';
import { createServer as createTcpServer, isIP } from 'node:net';
import { mkdir, open, readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  MAX_FRAME_BYTES,
  MAX_PENDING,
  MAX_REQUEST_BYTES,
  MAX_RESPONSE_BYTES,
  PROTOCOL_VERSION,
  RELAY_AGENT_ID,
  makeAgentProof,
  normalizeIp,
  timingSafeHexEqual,
  validateRpcRequest,
} from './protocol.mjs';
import identity from './identity.cjs';

const MAX_JOURNAL_ENTRIES = 100_000;
const MAX_JOURNAL_LINES = 250_000;
const TERMINAL_STATES = new Set(['completed', 'rpc_error', 'uncertain']);

function log(event, fields = {}) {
  const safe = { time: new Date().toISOString(), component: 'coord-relay', event, ...fields };
  process.stdout.write(`${JSON.stringify(safe)}\n`);
}

function secretHex(value, name) {
  if (typeof value !== 'string' || !/^[0-9a-f]{64}$/i.test(value)) {
    throw new Error(`${name} must be a 32-byte hex secret`);
  }
  return value.toLowerCase();
}

async function loadSecret(file, name) {
  return secretHex((await readFile(file, 'utf8')).trim(), name);
}

function writeFrame(socket, value) {
  const frame = Buffer.from(`${JSON.stringify(value)}\n`, 'utf8');
  if (frame.length > MAX_FRAME_BYTES) {
    socket.destroy();
    return false;
  }
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
      if (line.length === 0 || line.length > maxBytes) return onInvalid('invalid_frame_size');
      let value;
      try {
        value = JSON.parse(line.toString('utf8'));
      } catch {
        return onInvalid('invalid_json');
      }
      onLine(value);
      start = newline + 1;
    }
  });
  socket.on('end', () => {
    if (partial.length) onInvalid('truncated_frame');
  });
}

function listen(server, port, host) {
  return new Promise((resolve, reject) => {
    const onError = error => {
      server.off('listening', onListening);
      reject(error);
    };
    const onListening = () => {
      server.off('error', onError);
      resolve(server.address());
    };
    server.once('error', onError);
    server.once('listening', onListening);
    server.listen(port, host);
  });
}

function closeServer(server) {
  return new Promise(resolve => {
    if (!server.listening) return resolve();
    server.closeAllConnections?.();
    server.close(() => resolve());
  });
}

async function readRequestBody(request, maxBytes) {
  const contentLength = Number(request.headers['content-length'] || 0);
  if (contentLength > maxBytes) throw Object.assign(new Error('request_too_large'), { statusCode: 413 });
  const chunks = [];
  let total = 0;
  for await (const chunk of request) {
    total += chunk.length;
    if (total > maxBytes) throw Object.assign(new Error('request_too_large'), { statusCode: 413 });
    chunks.push(chunk);
  }
  return Buffer.concat(chunks, total).toString('utf8');
}

function reply(response, statusCode, body) {
  const bytes = Buffer.from(JSON.stringify(body), 'utf8');
  if (bytes.length > MAX_RESPONSE_BYTES) {
    response.writeHead(500, { 'content-type': 'application/json', connection: 'close' });
    response.end('{"state":"error","code":"response_too_large"}');
    return;
  }
  response.writeHead(statusCode, {
    'content-type': 'application/json; charset=utf-8',
    'content-length': bytes.length,
    'cache-control': 'no-store',
    connection: 'close',
  });
  response.end(bytes);
}

function validJsonRpcEnvelope(envelope, requestId) {
  return envelope !== null
    && typeof envelope === 'object'
    && envelope.jsonrpc === '2.0'
    && envelope.id === requestId
    && (Object.hasOwn(envelope, 'result') !== Object.hasOwn(envelope, 'error'));
}

export async function createSidecar(options = {}) {
  const config = {
    agentHost: options.agentHost ?? '100.116.134.114',
    agentPort: options.agentPort ?? 13012,
    clientHost: options.clientHost ?? '127.0.0.1',
    clientPort: options.clientPort ?? 13013,
    allowedPeerIp: options.allowedPeerIp ?? '100.89.6.126',
    agentId: options.agentId ?? RELAY_AGENT_ID,
    requestTimeoutMs: options.requestTimeoutMs ?? 10_000,
    heartbeatTimeoutMs: options.heartbeatTimeoutMs ?? 35_000,
    maxPending: options.maxPending ?? MAX_PENDING,
    journalFile: options.journalFile ?? './coord-relay-state/requests.ndjson',
    agentSecret: options.agentSecret,
    clientSecret: options.clientSecret,
    identityPolicy: options.identityPolicy ?? await identity.loadPolicy(options.identityPolicyFile),
  };
  if (options.agentSecretFile) config.agentSecret = await loadSecret(options.agentSecretFile, 'agent secret');
  if (options.clientSecretFile) config.clientSecret = await loadSecret(options.clientSecretFile, 'client secret');
  config.agentSecret = secretHex(config.agentSecret, 'agent secret');
  config.clientSecret = secretHex(config.clientSecret, 'client secret');
  if (!isIP(config.agentHost) || config.clientHost !== '127.0.0.1') throw new Error('invalid bind address');
  if (!Number.isInteger(config.agentPort) || config.agentPort < 0 || config.agentPort > 65535
    || !Number.isInteger(config.clientPort) || config.clientPort < 0 || config.clientPort > 65535) {
    throw new Error('invalid port');
  }
  if (!isIP(config.allowedPeerIp)) throw new Error('invalid allowed peer address');
  if (!Number.isInteger(config.maxPending) || config.maxPending < 1 || config.maxPending > MAX_PENDING) throw new Error('maxPending must be 1..4');
  if (!Number.isInteger(config.requestTimeoutMs) || config.requestTimeoutMs < 100 || config.requestTimeoutMs > 60_000) throw new Error('requestTimeoutMs must be 100..60000');
  if (!Number.isInteger(config.heartbeatTimeoutMs) || config.heartbeatTimeoutMs < 1_000 || config.heartbeatTimeoutMs > 120_000) throw new Error('heartbeatTimeoutMs must be 1000..120000');

  const sidecar = new Sidecar(config);
  await sidecar.initialize();
  return sidecar;
}

class Sidecar {
  constructor(config) {
    this.config = config;
    this.records = new Map();
    this.pending = new Map();
    this.agent = null;
    this.agentServer = createTcpServer(socket => this.acceptAgent(socket));
    this.httpServer = createHttpServer({ maxHeaderSize: 8 * 1024 }, (request, response) => {
      void this.handleHttp(request, response);
    });
    this.httpServer.headersTimeout = 5_000;
    this.httpServer.requestTimeout = 15_000;
    this.journalChain = Promise.resolve();
  }

  async initialize() {
    await mkdir(dirname(this.config.journalFile), { recursive: true, mode: 0o700 });
    const handle = await open(this.config.journalFile, 'a+', 0o600);
    await handle.chmod(0o600);
    const content = await handle.readFile('utf8');
    const lines = content.split('\n').filter(Boolean);
    if (lines.length > MAX_JOURNAL_LINES) throw new Error('journal_capacity_exceeded');
    for (const line of lines) {
      let record;
      try { record = JSON.parse(line); } catch { throw new Error('journal_corrupt'); }
      if (typeof record.requestId !== 'string' || typeof record.requestHash !== 'string' || typeof record.state !== 'string') {
        throw new Error('journal_corrupt');
      }
      this.records.set(record.requestId, record);
    }
    await handle.close();
    for (const record of this.records.values()) {
      if (!TERMINAL_STATES.has(record.state)) {
        await this.appendRecord(record.requestId, record.requestHash, record.method, 'uncertain', 'process_restart');
      }
    }
  }

  async start() {
    const agentAddress = await listen(this.agentServer, this.config.agentPort, this.config.agentHost);
    try {
      const clientAddress = await listen(this.httpServer, this.config.clientPort, this.config.clientHost);
      log('started', { agentAddress: `${agentAddress.address}:${agentAddress.port}`, clientAddress: `${clientAddress.address}:${clientAddress.port}` });
      return { agentAddress, clientAddress };
    } catch (error) {
      await closeServer(this.agentServer);
      throw error;
    }
  }

  async stop() {
    this.agent?.socket.destroy();
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer);
      pending.resolve({ state: 'uncertain', code: 'service_stopping' });
    }
    this.pending.clear();
    await Promise.all([closeServer(this.httpServer), closeServer(this.agentServer)]);
  }

  acceptAgent(socket) {
    if (normalizeIp(socket.remoteAddress) !== this.config.allowedPeerIp || this.agent) {
      socket.destroy();
      log('agent_rejected', { reason: this.agent ? 'already_connected' : 'peer_denied' });
      return;
    }
    socket.setNoDelay(true);
    const nonce = randomBytes(32).toString('base64url');
    let authenticated = false;
    let finished = false;
    const authTimer = setTimeout(() => socket.destroy(), 5_000);
    writeFrame(socket, { type: 'challenge', version: PROTOCOL_VERSION, nonce });
    const closeConnection = reason => {
      if (finished) return;
      finished = true;
      clearTimeout(authTimer);
      if (this.agent?.socket === socket) {
        this.agent = null;
        log('agent_disconnected', { reason });
        for (const [requestId] of this.pending) void this.finishPending(requestId, { state: 'uncertain', code: 'agent_disconnected' });
      }
    };
    socket.on('error', () => closeConnection('socket_error'));
    socket.on('close', () => closeConnection('socket_closed'));
    attachLineReader(socket, message => {
      if (!authenticated) {
        const proof = message?.proof;
        const expected = makeAgentProof(this.config.agentSecret, this.config.agentId, nonce);
        if (message?.type !== 'auth'
          || message.version !== PROTOCOL_VERSION
          || message.agentId !== this.config.agentId
          || !timingSafeHexEqual(expected, proof)) {
          log('agent_auth_failed', { reason: 'invalid_identity_or_proof' });
          socket.destroy();
          return;
        }
        authenticated = true;
        clearTimeout(authTimer);
        this.agent = { socket, lastHeartbeat: Date.now(), agentId: this.config.agentId };
        writeFrame(socket, { type: 'auth_ok', version: PROTOCOL_VERSION });
        log('agent_connected', { agentId: this.config.agentId });
        return;
      }
      if (message?.type === 'heartbeat' && message.agentId === this.config.agentId) {
        this.agent.lastHeartbeat = Date.now();
        writeFrame(socket, { type: 'heartbeat_ack' });
        return;
      }
      if (message?.type === 'result') {
        void this.acceptResult(message);
        return;
      }
      log('agent_protocol_rejected', { reason: 'unknown_frame' });
      socket.destroy();
    }, reason => {
      log('agent_protocol_rejected', { reason });
      socket.destroy();
    });
  }

  async acceptResult(message) {
    const requestId = message.requestId;
    const pending = this.pending.get(requestId);
    if (!pending) {
      log('late_or_unknown_result', { requestId });
      return;
    }
    const outcome = message.outcome;
    if (!outcome || !['ok', 'rpc_error', 'uncertain', 'not_dispatched'].includes(outcome.state)) {
      await this.finishPending(requestId, { state: 'uncertain', code: 'invalid_agent_result' });
      return;
    }
    if (outcome.state === 'ok' && !validJsonRpcEnvelope(outcome.response, requestId)) {
      await this.finishPending(requestId, { state: 'uncertain', code: 'invalid_coord_response' });
      return;
    }
    const payload = JSON.stringify(outcome);
    if (Buffer.byteLength(payload) > MAX_RESPONSE_BYTES) {
      await this.finishPending(requestId, { state: 'uncertain', code: 'response_too_large' });
      return;
    }
    await this.finishPending(requestId, outcome);
  }

  async appendRecord(requestId, requestHash, method, state, code = undefined) {
    this.journalChain = this.journalChain.catch(() => {}).then(async () => {
      if (!this.records.has(requestId) && this.records.size >= MAX_JOURNAL_ENTRIES) throw new Error('journal_capacity_exceeded');
      const record = { requestId, requestHash, method, state, at: new Date().toISOString() };
      if (code) record.code = code;
      const line = `${JSON.stringify(record)}\n`;
      const handle = await open(this.config.journalFile, 'a', 0o600);
      try {
        await handle.writeFile(line, 'utf8');
        await handle.sync();
      } finally {
        await handle.close();
      }
      this.records.set(requestId, record);
    });
    return this.journalChain;
  }

  async finishPending(requestId, outcome) {
    const pending = this.pending.get(requestId);
    if (!pending) return;
    clearTimeout(pending.timer);
    this.pending.delete(requestId);
    const journalState = outcome.state === 'ok' ? 'completed'
      : outcome.state === 'rpc_error' ? 'rpc_error' : 'uncertain';
    try {
      await this.appendRecord(requestId, pending.requestHash, pending.method, journalState, outcome.code);
      pending.resolve(outcome);
    } catch {
      pending.resolve({ state: 'uncertain', code: 'journal_write_failed' });
      log('journal_error', { requestId, reason: 'append_failed' });
    }
  }

  isAgentReady() {
    return this.agent !== null && Date.now() - this.agent.lastHeartbeat <= this.config.heartbeatTimeoutMs;
  }

  async callAgent(request) {
    if (!this.isAgentReady()) return { state: 'not_dispatched', code: 'agent_unavailable' };
    if (this.pending.size >= this.config.maxPending) return { state: 'not_dispatched', code: 'concurrency_limit' };
    const requestHash = createHmac('sha256', Buffer.from(this.config.clientSecret, 'hex'))
      .update(JSON.stringify({ method: request.method, params: request.params, identity: request.identity }))
      .digest('hex');
    const previous = this.records.get(request.requestId);
    if (previous) {
      return { state: 'uncertain', code: previous.requestHash === requestHash ? 'request_id_already_used' : 'request_id_conflict' };
    }
    const inFlight = this.pending.get(request.requestId);
    if (inFlight) {
      return { state: 'uncertain', code: inFlight.requestHash === requestHash ? 'request_id_in_flight' : 'request_id_conflict' };
    }
    let resolve;
    const resultPromise = new Promise(res => { resolve = res; });
    const pending = { resolve, requestHash, method: request.method, timer: null };
    this.pending.set(request.requestId, pending);
    try {
      await this.appendRecord(request.requestId, requestHash, request.method, 'dispatched');
    } catch {
      this.pending.delete(request.requestId);
      return { state: 'not_dispatched', code: 'journal_write_failed' };
    }
    pending.timer = setTimeout(() => {
      void this.finishPending(request.requestId, { state: 'uncertain', code: 'relay_timeout' });
    }, this.config.requestTimeoutMs);
    if (!this.agent || !this.isAgentReady() || !writeFrame(this.agent.socket, {
      type: 'call', requestId: request.requestId, method: request.method, params: request.params,
      ...(request.identity ? { identity: request.identity } : {}),
    })) {
      await this.finishPending(request.requestId, { state: 'uncertain', code: 'dispatch_failed' });
    }
    return resultPromise;
  }

  authorized(request) {
    const header = request.headers.authorization;
    if (typeof header !== 'string' || !header.startsWith('Bearer ')) return false;
    return timingSafeHexEqual(this.config.clientSecret, header.slice(7));
  }

  async handleHttp(request, response) {
    const remote = normalizeIp(request.socket.remoteAddress);
    if (remote !== '127.0.0.1' && remote !== '::1') {
      reply(response, 403, { state: 'error', code: 'loopback_only' });
      return;
    }
    if (request.method === 'GET' && request.url === '/healthz') {
      const ready = this.isAgentReady();
      reply(response, ready ? 200 : 503, {
        state: ready ? 'READY' : 'DEGRADED',
        agentConnected: this.agent !== null,
        lastHeartbeat: this.agent ? new Date(this.agent.lastHeartbeat).toISOString() : null,
        pending: this.pending.size,
      });
      return;
    }
    if (request.method !== 'POST' || request.url !== '/v1/rpc') {
      reply(response, 404, { state: 'error', code: 'not_found' });
      return;
    }
    if (!this.authorized(request)) {
      reply(response, 401, { state: 'error', code: 'unauthorized' });
      return;
    }
    let body;
    try {
      body = await readRequestBody(request, MAX_REQUEST_BYTES);
    } catch (error) {
      reply(response, error.statusCode ?? 400, { state: 'error', code: error.message === 'request_too_large' ? error.message : 'invalid_body' });
      return;
    }
    let rpc;
    try {
      rpc = JSON.parse(body);
    } catch {
      reply(response, 400, { state: 'error', code: 'invalid_json' });
      return;
    }
    const validation = validateRpcRequest(rpc, this.config.identityPolicy);
    if (!validation.ok) {
      reply(response, validation.code === 'request_too_large' ? 413 : 400, { state: 'error', code: validation.code });
      log('request_rejected', { code: validation.code });
      return;
    }
    if (!this.isAgentReady()) {
      reply(response, 503, { state: 'not_dispatched', code: 'agent_unavailable', requestId: rpc.requestId });
      return;
    }
    if (this.pending.size >= this.config.maxPending) {
      reply(response, 429, { state: 'not_dispatched', code: 'concurrency_limit', requestId: rpc.requestId });
      return;
    }
    const started = Date.now();
    const outcome = await this.callAgent(rpc);
    if (outcome.state === 'ok') {
      const result = outcome.response.result;
      if (Buffer.byteLength(JSON.stringify(result)) > MAX_RESPONSE_BYTES) {
        reply(response, 502, { state: 'uncertain', code: 'response_too_large', requestId: rpc.requestId });
      } else {
        reply(response, 200, { state: 'ok', requestId: rpc.requestId, result });
      }
    } else if (outcome.state === 'rpc_error') {
      reply(response, 409, { state: 'rpc_error', requestId: rpc.requestId, error: outcome.error });
    } else if (outcome.state === 'not_dispatched') {
      reply(response, 503, { state: 'not_dispatched', requestId: rpc.requestId, code: outcome.code });
    } else {
      reply(response, 504, { state: 'uncertain', requestId: rpc.requestId, code: outcome.code ?? 'outcome_unknown' });
    }
    log('rpc_finished', { requestId: rpc.requestId, method: rpc.method, state: outcome.state, durationMs: Date.now() - started });
  }
}

async function main() {
  const configPath = process.argv[process.argv.indexOf('--config') + 1];
  if (!configPath) throw new Error('usage: node server.mjs --config <path>');
  const config = JSON.parse(await readFile(configPath, 'utf8'));
  const sidecar = await createSidecar({
    ...config,
    journalFile: config.journalFile,
    agentSecretFile: config.agentSecretFile,
    clientSecretFile: config.clientSecretFile,
  });
  await sidecar.start();
  const stop = async signal => {
    log('stopping', { signal });
    await sidecar.stop();
    process.exit(0);
  };
  process.once('SIGINT', () => void stop('SIGINT'));
  process.once('SIGTERM', () => void stop('SIGTERM'));
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  main().catch(error => {
    log('fatal', { reason: error.message === 'journal_corrupt' ? 'journal_corrupt' : 'startup_failed' });
    process.exit(1);
  });
}

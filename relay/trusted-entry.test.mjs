import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { resolve } from 'node:path';
import { test } from 'node:test';
import identity from './identity.cjs';

const config = () => ({ version: 3, mode: 'logical', enabled: true,
  signing_key_file: resolve('synthetic-key'), legacy_client_sha256: [],
  bindings: [{ id: 'shared-web', client_sha256: identity.hash('shared-client'), batch_id: '*',
    worker_ids: ['*'], scopes: ['coord:read', 'coord:claim'] }] });
const key = 'ab'.repeat(32);

test('trusted entry lasts across days, new batches and arbitrary workers but OAuth remains authenticated', () => {
  const policy = identity.validatePolicy(config(), key);
  for (const days of [0, 2, 365]) {
    const now = Date.now() + days * 86_400_000;
    const auth = { clientId: 'shared-client', expiresAt: now + 120_000 };
    assert.equal(identity.resolvePrincipal(policy, auth, now), null);
    for (const worker_id of ['writer-1', 'fresh-worker', 'aBcD']) {
      const batch_id = randomUUID();
      const principal = identity.resolvePrincipal(policy, auth, now, { worker_id, batch_id });
      const request = identity.signRequest({ requestId: randomUUID(), method: 'scheduler/trusted_claim',
        params: { worker_id: principal.worker_id, agent_id: principal.agent_id, batch_id } }, principal, policy);
      assert.equal(identity.verifyRequest(request, policy, now).ok, true);
      assert.equal(identity.verifyRequest(request, policy, now + 60_000).ok, false);
      assert.equal(identity.verifyRequest(request, { ...policy, bindings: [] }, now).ok, false);
      const changed = structuredClone(request);
      changed.params.batch_id = randomUUID();
      assert.equal(identity.verifyRequest(changed, policy, now).ok, false);
      const denied = identity.signRequest({ requestId: randomUUID(), method: 'scheduler/trusted_complete',
        params: { batch_id } }, principal, policy);
      assert.equal(identity.verifyRequest(denied, policy, now).ok, false);
    }
    assert.throws(() => identity.resolvePrincipal(policy, { ...auth, clientId: 'untrusted' }, now));
    assert.throws(() => identity.resolvePrincipal(policy, { ...auth, expiresAt: now }, now));
    assert.throws(() => identity.resolvePrincipal(policy, auth, now, { worker_id: '../invalid', batch_id: randomUUID() }));
  }
  assert.equal(identity.protectedBatch(policy, randomUUID()), true);
});

test('persistent policy rejects timed worker registration, overlapping grants and malformed scopes', () => {
  for (const change of [
    binding => { binding.expires_at = Date.now(); },
    binding => { binding.worker_ids = ['writer-1']; },
    binding => { binding.batch_id = 'invalid'; },
    binding => { binding.scopes = ['coord:admin']; },
  ]) {
    const invalid = config();
    change(invalid.bindings[0]);
    assert.throws(() => identity.validatePolicy(invalid, key));
  }
  const overlap = config();
  overlap.bindings.push({ ...overlap.bindings[0], id: 'second', batch_id: randomUUID() });
  assert.throws(() => identity.validatePolicy(overlap, key));
  const scoped = config();
  scoped.bindings[0].batch_id = randomUUID();
  const policy = identity.validatePolicy(scoped, key);
  assert.throws(() => identity.resolvePrincipal(policy, { clientId: 'shared-client', expiresAt: Date.now() + 120_000 },
    Date.now(), { worker_id: 'any-worker', batch_id: randomUUID() }));
});

// Run locally on the OAuth host as its operator. Never emits bearer, refresh,
// authorization-code, PKCE, cookie, owner identity or client ID values.
const fs = require('node:fs/promises');
const { hash } = require('../identity.cjs');

async function main() {
  const stateFile = process.argv[2];
  if (!stateFile) throw new Error('state_file_required');
  const state = JSON.parse(await fs.readFile(stateFile, 'utf8'));
  const now = Date.now();
  const report = { observed_at: new Date(now).toISOString(), counts: {}, fields: {}, live_token_bindings: [] };
  for (const name of ['clients', 'accessTokens', 'refreshTokens']) {
    const rows = Object.values(state[name] || {});
    report.counts[name] = rows.length;
    report.fields[name] = [...new Set(rows.flatMap(row => Object.keys(row)))].sort();
  }
  for (const [tokenHash, row] of Object.entries(state.accessTokens || {})) {
    if (!/^[a-f0-9]{64}$/.test(tokenHash) || typeof row.clientId !== 'string' || row.expiresAt <= now) continue;
    report.live_token_bindings.push({ client_sha256: hash(row.clientId), token_sha256: tokenHash,
      issued_at: row.issuedAt, expires_at: row.expiresAt });
  }
  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
}
if (require.main === module) main().catch(() => { process.stderr.write('OAuth store audit failed; no store values emitted.\n'); process.exitCode = 1; });

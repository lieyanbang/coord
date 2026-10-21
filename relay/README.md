# coord Tailscale Relay

This package contains a dependency-free Node.js sidecar and the Arasaka
connector. It does not extend FeathersJS or Socket.IO. The sidecar accepts the
connector only on the configured Tailscale address; its RPC API binds to IPv4
loopback only.

## Protocol and boundaries

- The connector opens an outbound TCP session and proves possession of its
  independent 32-byte secret with an HMAC-SHA256 challenge response. The
  sidecar also requires the configured Tailscale peer address and allows one
  authenticated connector at a time. Tailscale supplies transport encryption.
- The local `POST /v1/rpc` endpoint requires a separate 32-byte bearer secret.
  The unauthenticated `GET /healthz` endpoint is loopback-only and returns only
  connector liveness and pending count.
- The JSON-RPC allowlist is limited to `tasks/list`, `tasks/get`,
  `agents/heartbeat`, `tasks/claim`, `tasks/extend`, `tasks/complete`,
  `scheduler/claim`, `scheduler/extend`, `scheduler/submit`, and
  `scheduler/failure`. `tasks/list` requires `skip_reclaim: true`; the relay
  denies task creation, cancellation, arbitrary methods, unknown parameters,
  and caller-selected agent identities.
- Requests, frames, and results are bounded at 64 KiB, concurrency is limited
  to four requests, and a dispatched call is never retried. The client must
  reuse the same UUID `requestId` when reconciling a transport interruption.
  A repeated ID returns `uncertain` rather than being replayed. Calls that time
  out or lose their connector return `outcome_unknown`; coord's lease token
  fencing remains authoritative for claims and submissions.
- The append-only journal contains only request IDs, keyed HMAC-SHA256 of method and
  parameters, method name, state, and time. It never stores parameters, results,
  bearer secrets, or lease tokens. Journal corruption or its 100,000 unique-ID
  ceiling fails closed; no automatic pruning can reopen an old request ID.
- Responses redact `lease_token` for tasks not claimed by
  `coord-relay-agent`. Logs contain request ID, method, state, duration, and
  bounded reason codes only.

## Local test

Node.js 22 or newer is required. No npm install is needed.

```powershell
node --check relay/protocol.mjs
node --check relay/server.mjs
node --check relay/agent.mjs
node --test relay/relay.test.mjs
```

The tests use loopback listeners and temporary state; they do not reach the
production server or production coord database.

## Production layout

- Sidecar source: `/opt/coord-relay` (root-owned, read-only to the service).
- Sidecar configuration and separate agent/client credentials:
  `/etc/coord-relay` (root-owned; secret files group-readable only by
  `coord-relay`).
- Append-only request journal: `/var/lib/coord-relay/requests.ndjson`.
- Service logs: `journalctl -u coord-relay.service`.
- Sidecar agent socket: `100.116.134.114:13012/tcp`.
- Local client API: `127.0.0.1:13013/tcp`.
- Arasaka connector: `C:\ProgramData\coord-relay-agent`, run by the dedicated
  Servy entry `CoordRelayAgent` under `NT AUTHORITY\LocalService`.

The sidecar also checks that the peer source is exactly `100.89.6.126` before
sending its authentication challenge. The intended host firewall rule is
source `100.89.6.126`, destination `100.116.134.114`, TCP port `13012`, and
interface `tailscale0`, but do not assume a UFW rule enforces this on
`tailscale0`: on the current production host, `INPUT` jumps to Tailscale's
`ts-input` chain first, and that chain accepts `tailscale0` before UFW. An
unauthorized tailnet peer can therefore complete TCP but must receive no
challenge. The tailnet administrator ACL is not exposed by the node-side CLI;
verify it independently and do not claim network-layer isolation from UFW or
`tailscale ping` alone.

## Rollback

Stop and disable only `coord-relay.service`, then stop and uninstall only the
Servy entry `CoordRelayAgent`, and remove only the exact UFW rule created for
this sidecar. Keep `/var/lib/coord-relay` and both credential directories until
their contents have been separately archived or explicitly discarded. Existing
FeathersJS, OpenResty, DesktopCommanderMCP, and port 13011 are outside this
package's ownership.

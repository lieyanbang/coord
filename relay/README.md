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

The sidecar checks that the peer source is exactly `100.89.6.126` before
sending its authentication challenge. Network-layer source restriction is
enforced separately by `relay/host-acl/coord-relay-acl.nft`: an inet input
chain at priority `-10` drops other IPv4 sources on `tailscale0` only when the
destination is `100.116.134.114:13012/tcp`. This runs before the iptables-nft
filter hook where Tailscale's generated `ts-input` chain accepts Tailnet
traffic. The isolated systemd unit is `coord-relay-acl.service`; see
`relay/host-acl/README.md` for deployment, verification, and narrow rollback.

The Tailnet policy was inspected in the admin console and has a broad
`src: ["*"]` grant to Tailnet IP ranges. Grants are additive, so an extra
allow would not deny BT-7274. That broad grant and all other grants were left
unchanged. BT-7274's fresh TCP connect to 13012 timed out and incremented the
dedicated drop counter; Arasaka's new TCP connect and authenticated
Sidecar-to-coord read-only RPC both succeeded. UFW rule 48 remains present but
is not treated as the effective filter because `INPUT` reaches `ts-input`
first. The existing 13011 channel remains outside this boundary.

## Rollback

For an ACL-only rollback, use the scoped procedure in
`relay/host-acl/README.md`; it removes only `inet coord_relay_acl` and leaves
the sidecar running with its application-level peer/HMAC checks. For full
relay removal, first stop and disable `coord-relay.service`, then stop and
uninstall only the Servy entry `CoordRelayAgent`, then run
`systemctl disable --now coord-relay-acl.service` and remove only the exact
UFW rule created for this sidecar. Keep `/var/lib/coord-relay` and both
credential directories until their contents have been separately archived or
explicitly discarded. Existing FeathersJS, OpenResty, DesktopCommanderMCP,
and port 13011 are outside this package's ownership.

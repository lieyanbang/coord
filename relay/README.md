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
  `scheduler/failure`, `scheduler/progress`, `scheduler/extend_progress`, and
  `scheduler/agent_status`, plus the seven fixed `scheduler/trusted_*` methods.
  `tasks/list` requires `skip_reclaim: true`; the relay denies task creation,
  cancellation, arbitrary methods, unknown parameters, and caller-selected
  authenticated agent identities.
- The `coord_claim`, `coord_progress`, `coord_complete`, `coord_fail`, `coord_release`,
  `coord_resume`, and `coord_status` MCP tools use OAuth/Cloudflare Access as
  the owner boundary. Core persists attempt/checkpoint state and never returns
  a lease token to these tools. The default trusted-owner path relies on the
  authenticated OAuth request, the private local Relay bearer and the
  authenticated connector. If a v2 logical identity policy is enabled, each
  call also carries a signed worker/batch/scope/expiry binding that the sidecar
  verifies. Worker labels match `^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$`; generic
  strings such as `writer-1` are case-sensitive and keep their names. Four-hex
  labels and historical `emote-XXXX` IDs keep their canonical recovery behavior.
  Use `worker_ids: ["*"]` for any valid collaboration label within
  that binding, or an explicit list for a stricter profile. The label does not
  identify a separate conversation. See
  [`IDENTITY_BINDINGS.md`](../docs/phase-g/IDENTITY_BINDINGS.md) for the legacy
  strict policy. Real ChatGPT conversation isolation remains unavailable.
- Logical `coord_tasks_list` requires `worker_id` and `batch_id`. The OAuth
  client must have a live binding for that batch, worker and `coord:read` scope.
  SQL filters before LIMIT, and the Windows Agent independently verifies every
  returned task belongs to the signed batch. Multiple authorized batches are
  queried separately. Missing declarations never fall back to a global list;
  explicitly approved legacy clients retain their existing list permissions.
- Progress-required batches can call `scheduler/progress` with the current
  task, lease token, attempt number, next sequence and a short checkpoint, then
  `scheduler/extend_progress` with that unconsumed sequence. The batch's
  cumulative lease budget is finite. Checkpoints are self-reported work notes,
  not proof that a model is alive or will wake later; an idle conversation must
  be allowed to lose its lease. `scheduler/agent_status` returns no lease
  tokens.
- Trusted batch calls use `scheduler/trusted_claim` to claim only from the
  requested batch. `scheduler/trusted_progress` generates sequence/request IDs
  and persists the checkpoint; `scheduler/trusted_complete` is fenced by
  `task_id` and integer `attempt_no`; `scheduler/trusted_release` requeues a
  live attempt without consuming business failure budget;
  `scheduler/trusted_fail` uses the existing failure budget/backoff and stores
  a receipt, so identical retries do not count another failure;
  `scheduler/trusted_resume` reads durable checkpoint/disposition without
  reclaiming; and `scheduler/trusted_status` distinguishes state totals from
  live `active_claims`. Resume never wakes a model or transfers a task.
- Requests, frames, and results are bounded at 64 KiB, concurrency is limited
  to four requests, and a dispatched call is never retried. The client must
  reuse the same UUID `requestId` when reconciling a transport interruption.
  A repeated ID returns `uncertain` rather than being replayed. Calls that time
  out or lose their connector return `outcome_unknown`; coord's lease token
  fencing remains authoritative for claims and submissions.
- The journal contains only request IDs, keyed HMAC-SHA256 of method,
  parameters and authenticated identity, method name, state, and time. It never
  stores parameters, results,
  bearer secrets, or lease tokens. It compacts to at most 100,000 recent records
  (configurable as `maxJournalEntries`, 16..100000), rather than stopping after
  that many cumulative requests. Retired IDs are recorded in a fixed 8 MiB,
  seven-hash conservative filter persisted before replacing the journal.
  A retired ID returns `uncertain/request_id_retired_reconcile`, including for
  uncertain calls; it is never dispatched again. Hash collisions can reject a
  genuinely new ID conservatively. Reconcile authoritative Core state before
  choosing a new request ID; external effects must never be blindly replayed.
  Keep `requests.ndjson` and `requests.ndjson.retired` together in backups.
  A missing/truncated filter after compaction fails closed. Old Relay versions
  reject the compaction marker: rolling back Core is compatible, but a compacted
  journal requires this Relay version or a compatible forward fix.
- Legacy responses redact `lease_token` for tasks not claimed by
  `coord-relay-agent`; bound reads strip all lease-token fields and legacy
  bound claims expose only that worker's lease. Trusted responses strip all
  lease-token fields. Logs contain request ID, method, state,
  duration, and bounded reason codes only.

## Protected main coord RPC

When the daemon enables the external Action Runner, its ordinary RPC endpoint
requires an independent private bearer credential from
`external_runner.daemon_rpc_key_file`. Set the Windows Relay Agent's optional
`coordAuthKeyFile` to the absolute path of that same protected key file. The
trimmed UTF-8 file must contain exactly 64 hexadecimal characters (256 bits)
and must differ from the Action Runner broker key. The Agent supplies it on
every core RPC, including immutable task-scope preflight. Without this option,
the legacy Agent sends no bearer header.

Only the daemon, trusted operator/CLI and Relay Agent may read the main RPC key;
the Action Runner account must be denied access. Keep it outside task and Agent
workspaces, prompts, logs and Git. The Agent accepts only an IPv4 loopback core
URL (`http://127.0.0.1:<port>/`) and refuses HTTP redirects, so it cannot forward
the private credential to a redirect destination. Production ACLs and the
protected main-RPC path still require the G6-B/D host acceptance checks.

## Local test

Node.js 22 or newer is required. The independent HTTP MCP client uses the
existing Coord MCP test dependencies.

```powershell
node --check relay/protocol.mjs
node --check relay/server.mjs
node --check relay/agent.mjs
npm ci --prefix relay/coord-mcp --ignore-scripts --no-audit --no-fund
$env:COORD_TEST_BIN = (Resolve-Path target/release/coord.exe).Path
node --test relay/relay.test.mjs relay/live-coord.test.mjs relay/logical-workers.test.mjs relay/generic-workers.test.mjs
```

The tests use loopback listeners and temporary state; they do not reach the
production server or production coord database.

## Production layout

- Sidecar source: `/opt/coord-relay` (root-owned, read-only to the service).
- Sidecar configuration and separate agent/client credentials:
  `/etc/coord-relay` (root-owned; secret files group-readable only by
  `coord-relay`).
- Compacted request journal and retired-ID filter: `/var/lib/coord-relay/requests.ndjson{,.retired}`.
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

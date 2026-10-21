# Phase F3 Incremental Production Evidence

Recorded 2026-10-21 21:49-21:52 UTC (2026-10-22 Australia/Brisbane). Direct
SSH was used for host checks; no MCP or Remote Control tool was used. Commands
below omit private-key values and all OAuth/Sidecar/lease credentials.

## Host and Public Route Checks

- `ssh -o BatchMode=yes -o ConnectTimeout=10 ... ubuntu@instance-20260424-1840.tailc57112.ts.net hostname` — exit 0; host `instance-20260424-1840`.
- `systemctl show feathersjsapp.service openresty.service coord-relay.service tailscaled.service --property=Id --property=ActiveState --property=SubState --property=NRestarts --property=MainPID --property=ExecMainStartTimestamp` — exit 0; all four active/running, `NRestarts=0`.
- `curl -sS http://127.0.0.1:13013/healthz` — exit 0; `READY`, `agentConnected=true`, `pending=0` before and after the batch write.
- `ss -H -ltnp` — exit 0; expected listeners remain: Feathers `127.0.0.1:13010`, Sidecar `127.0.0.1:13013`, relay `100.116.134.114:13011` and `:13012`.
- `curl -sS -D - -o /dev/null -w ... https://lieyanbang.com/app/CoordMCP/` — exit 0, HTTP 401 with `WWW-Authenticate` resource metadata and `coord` scope.
- GET `https://lieyanbang.com/app/CoordMCP/.well-known/oauth-protected-resource/mcp` — HTTP 200; CoordMCP resource, issuer and `coord` / `offline_access` scopes.
- GET `https://lieyanbang.com/app/CoordMCP/.well-known/oauth-authorization-server` — HTTP 200; correct issuer, authorization/token/registration endpoints and `S256` PKCE.
- GET `https://lieyanbang.com/app/RemoteControlMCP3/` and `/mcp` without credentials — each HTTP 401.
- `sudo -n openresty -t` — exit 0; syntax and configuration successful. No reload was performed during this continuation.
- A request to the wrong public metadata URL with a trailing backend-only `/coord` suffix returned 404. The correct issuer-root public URL above returned 200; no route change was needed.

## OAuth and Live Sidecar Tests

- `npm test` in `relay/coord-mcp` — exit 0, 10/10 tests.
- `node --check host-oauth.integration.cjs` — exit 0.
- Fork Windows Actions run [39682968742](https://github.com/lieyanbang/coord/actions/runs/39682968742) for commit `8c86342b36ecd7c5a00b4b6123be806f62c99b18` completed with `conclusion=success`; the Windows job and all 29 steps succeeded, including `npm ci`, CoordMCP tests, and artifact upload. Artifact `coord-windows-2022` (`12385511806`, 4,226,358 bytes) is present and not expired.
- The temporary host harness ran as `coord-relay`, using Node `v22.23.2`, Express `4.21.2`, MCP SDK `1.30.0`, and the deployed DesktopCommander OAuth implementation. With the default 2-second token TTL and the isolated Sidecar stub, DCR, S256 PKCE, scope/resource failures, refresh, expiry, MCP initialize/tools/list and tasks/list passed; exit 0.
- A single production test batch was created through Arasaka JSON-RPC `batches/create` and `batches/add_tasks`; both calls returned successfully (SSH process exit 0). Batch ID: `ee52bcae-f269-498b-b7ed-d740e1b137ab`. Its only task ID: `30aaca96-badb-45cd-a1e7-00ff51ef0800`.
- The live harness ran with `COORD_MCP_USE_LIVE_SIDECAR=1`, a 60-second temporary access-token TTL, and the two IDs above. It used a loopback-only temporary HTTP server and the real production Sidecar client. DCR, PKCE, refresh/expiry, initialize, tools/list, live tasks/list, and the one claim/submit/readback sequence all passed; process exit 0.
- Harness output: `host OAuth and MCP integration: PASS (... live Sidecar to coord tasks/list, result_type=array, array_lengths=1)` and `production batch tool round-trip: PASS`.
- A separate read-only Arasaka `tasks/get` + `batches/status` check exited 0: the task is `completed`, `attempt_no=1`, the test result marker is `PASS`, and the batch is `completed`. The harness verified task readback omitted lease tokens. No dispatched write was retried.

## Deployment Identity and Remaining Boundary

At `2026-10-21T21:52:21Z`, deployed SHA-256 values were:

- Feathers host entry: `b204299f2a10d868b21dd710bf6cce3522ee48147f1d1a830f09d537479372bd`
- Active OpenResty config: `8bcba6f3543623e4ee53aab748e7d6b4fcc33b98b3aaf3eeeb6ef3d01fc7ef7c`
- CoordMCP `index.cjs`: `d0438d074a9ee7f03c8171f719e2d0a63cfeaa8195bf04f29321872f5d1bb3e1`
- CoordMCP `oauth.cjs`: `9e4d3a2adf595149b3eb672250add56975c5c434d754fd434db23c7576479a8d`

The pre-deployment backup remains at
`/var/backups/coord-mcp-f3-038c52fb82844b9897325acabda7b41a/` (`700 root:root`).
The original deployment's Feathers restart at `2026-10-21 17:36:03 UTC` was
not repeated. This continuation did not restart any service, reload OpenResty,
or change ACL/firewall configuration.

**NOT_RUN:** ChatGPT Web custom-server OAuth consent and an authenticated
`initialize`/tool call through the public OpenResty route. The live write test
used a temporary local OAuth host and the production Sidecar; it does not
substitute for a public authenticated call. Phase F3 remains **PARTIAL**.

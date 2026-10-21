# Phase F3: Public MCP and OAuth

Date: 2026-10-22 (Australia/Brisbane)
Result: **PARTIAL**. The isolated CoordMCP endpoint is deployed and its OAuth,
MCP, and production Sidecar paths pass the checks below. Actual ChatGPT Web
authorization and an authenticated tool call through the public route are
**NOT_RUN**, so Gate F3 is not closed.

## F2 Entry Gate

- F2 closeout commit `2be6ae12c01e2e7d18159dcf80f1e82f09004039` passed Fork
  Windows Actions run [39636977435](https://github.com/lieyanbang/coord/actions/runs/39636977435).
- The Windows job and all steps succeeded, including relay protocol and lease
  fencing tests and artifact upload (`coord-windows-2022`, `12362625678`).
- F2 evidence is in [`PHASE_F2.md`](PHASE_F2.md) and
  [`phase-f2-acl-deployment-20261022.md`](phase-f2-acl-deployment-20261022.md).

## Production Deployment

The original read-only baseline was recorded at `2026-10-21T16:57:53Z` by
direct SSH. FeathersJS, OpenResty, coord-relay, and Tailscale were active;
Feathers listened on loopback `13010`, the Sidecar on loopback `13013`, and the
existing authenticated relay listeners remained on `100.116.134.114:13011` and
`:13012`. No existing RemoteControlMCP route was changed.

Before deployment, the exact host entry and active OpenResty file were backed
up under `/var/backups/coord-mcp-f3-038c52fb82844b9897325acabda7b41a/`. The
directory is `700 root:root`; it contains the original `feathers-index.js`
(3169 bytes) and `lieyanbang.conf` (24047 bytes). The backups were retained.

The deployed hashes were rechecked over SSH at `2026-10-21T21:52:21Z`:

| File | SHA-256 |
| --- | --- |
| `/opt/app/feathers/index.js` | `b204299f2a10d868b21dd710bf6cce3522ee48147f1d1a830f09d537479372bd` |
| `/usr/local/openresty/nginx/conf/conf.d/lieyanbang.conf` | `8bcba6f3543623e4ee53aab748e7d6b4fcc33b98b3aaf3eeeb6ef3d01fc7ef7c` |
| `.../lib/coord-mcp/index.cjs` | `d0438d074a9ee7f03c8171f719e2d0a63cfeaa8195bf04f29321872f5d1bb3e1` |
| `.../lib/coord-mcp/oauth.cjs` | `9e4d3a2adf595149b3eb672250add56975c5c434d754fd434db23c7576479a8d` |

The Feathers shared service was controlled-restarted once during the original
deployment at `2026-10-21 17:36:03 UTC`; this continuation did not restart it or
reload OpenResty. Current systemd state at `2026-10-21T21:52:21Z`:

| Service | State | PID | Restarts |
| --- | --- | ---: | ---: |
| `feathersjsapp.service` | active/running | 86861 | 0 |
| `openresty.service` | active/running | 47581 | 0 |
| `coord-relay.service` | active/running | 81799 | 0 |
| `tailscaled.service` | active/running | 824 | 0 |

`sudo -n openresty -t` exited 0 with syntax/configuration success. The CoordMCP
OAuth state directory is `700 root:root`; `oauth.json` and
`oauth-register.jsonl` are `600 root:root`. Current Sidecar `GET
http://127.0.0.1:13013/healthz` returned HTTP 200 and
`{"state":"READY","agentConnected":true,"pending":0}`. Listener inspection
confirmed `13010`, `13011`, `13012`, and `13013` remain bound to their expected
addresses. Established connection counts are dynamic and are not used as a
pass/fail assertion.

## Public Route Checks

Read-only public probes from the relay host at `2026-10-21T21:49:15Z`:

| Request | Result |
| --- | --- |
| `GET https://lieyanbang.com/app/CoordMCP/` without credentials | HTTP 401; `WWW-Authenticate` points to the protected-resource metadata and requires `coord` |
| `GET https://lieyanbang.com/app/CoordMCP/.well-known/oauth-protected-resource/mcp` | HTTP 200; resource and authorization server identify the CoordMCP issuer; scopes are `coord` and `offline_access` |
| `GET https://lieyanbang.com/app/CoordMCP/.well-known/oauth-authorization-server` | HTTP 200; issuer/endpoints are under `/app/CoordMCP`, PKCE supports `S256`, scopes are `coord` and `offline_access` |
| `GET https://lieyanbang.com/app/RemoteControlMCP3/` without credentials | HTTP 401 |
| `GET https://lieyanbang.com/mcp` without credentials | HTTP 401 |

The public authorization-server metadata URL is at the issuer root. A probe
that appended the backend-only `/coord` suffix returned 404; it was a
misaddressed probe, not the public route. The exact OpenResty locations proxy
the public issuer-root metadata path to the existing Feathers metadata suffix.

## OAuth and MCP Verification

The isolated host harness used the deployed OAuth source, Express `4.21.2`,
MCP SDK `1.30.0`, and Node `v22.23.2`. OAuth state was held in a temporary
directory. The test owner password was random, process-local, and not logged.

- Local isolated host OAuth/MCP run used the Sidecar stub and a 2-second
  access-token TTL. It exercised DCR, S256 PKCE, resource/scope checks,
  refresh, expiration rejection, `initialize`, `tools/list`, and `tasks/list`;
  process exit 0.
- The production Sidecar run used the same deployed OAuth/MCP code in a
  temporary server bound only to `127.0.0.1`, as service user `coord-relay`.
  Access-token TTL was 60 seconds for the sequential live write path. It
  verified DCR, PKCE, resource binding, refresh/expiry, MCP initialization,
  tool discovery, and a live Sidecar `tasks/list`; process exit 0.
- One isolated production batch/task was created, then the MCP tools performed
  `coord_claim_next`, `coord_submit_result`, and `coord_get_task` once each.
  The harness did not retry writes. It exited 0 and reported
  `isolated_batch_write=PASS` for batch
  `ee52bcae-f269-498b-b7ed-d740e1b137ab`, task
  `30aaca96-badb-45cd-a1e7-00ff51ef0800`.
- Direct coord readback exited 0: task state `completed`, `attempt_no=1`,
  result marker `f3_acceptance=PASS`; batch state `completed`. The harness
  verified that readback contained no lease token. Tokens and credentials were
  not written to logs or documentation.
- Final Sidecar health after the write remained `READY`,
  `agentConnected=true`, `pending=0`; no service restart or OpenResty reload was
  performed for these tests.

The Windows-local CoordMCP suite passes 10/10. A test harness change now runs
the write probe before its expiration check and uses the refreshed access
token; `COORD_MCP_ACCESS_TOKEN_TTL_SECONDS` allows a bounded longer TTL for
live integration tests. Stage F3 commands and logs are in
`docs/verification/stage-f3-*`. Fork Windows Actions run
[39682968742](https://github.com/lieyanbang/coord/actions/runs/39682968742)
completed successfully for commit `8c86342b36ecd7c5a00b4b6123be806f62c99b18`;
all 29 job steps passed, including dependency install, CoordMCP OAuth/HTTP
tests, and artifact upload. Artifact `coord-windows-2022`
(`12385511806`, 4,226,358 bytes) is present and not expired.

## Remaining F3 Gate

- **NOT_RUN:** add the custom server URL
  `https://lieyanbang.com/app/CoordMCP/` in ChatGPT Web, complete the real
  browser OAuth consent, and verify ChatGPT discovers and calls the public MCP
  endpoint using the authorized token.
- **NOT_RUN:** an authenticated `initialize`/`tools/list` and tool call through
  the public OpenResty route. The live batch round-trip above used an isolated
  local OAuth server with the production Sidecar; it does not prove this public
  authenticated path.
- User/browser authorization was not performed in this continuation. No
  public client credentials, access tokens, or refresh tokens were created or
  recorded. Keep Phase F3 **PARTIAL** until these user-facing checks have real
  evidence; do not promote this result to PASS based on service health alone.

# Phase F3: Public MCP and OAuth

Date: 2026-10-22 (Australia/Brisbane)
Result: **IN_PROGRESS**. F2 is closed. F3 has completed a read-only
production baseline; no FeathersJS, OpenResty, DesktopCommanderMCP, OAuth, or
public route configuration has been changed.

## F2 Entry Gate

- F2 closeout commit `2be6ae12c01e2e7d18159dcf80f1e82f09004039` passed Fork
  Windows Actions run [39636977435](https://github.com/lieyanbang/coord/actions/runs/39636977435).
- The `test (windows-2022)` job and all steps succeeded, including Relay
  protocol and lease-fencing tests and artifact upload. Artifact
  `coord-windows-2022` (`12362625678`) is 4,226,357 bytes and has not expired.
- F2 evidence remains in [`PHASE_F2.md`](PHASE_F2.md) and
  [`phase-f2-acl-deployment-20261022.md`](phase-f2-acl-deployment-20261022.md).

## Read-only Production Baseline

Direct SSH was used as requested; no MCP/Remote Control tool was used. The
read-only SSH batch on `instance-20260424-1840` at `2026-10-21T16:57:53Z`
exited 0.

- `feathersjsapp.service`, `openresty.service`, `coord-relay.service`, and
  `tailscaled.service` were all active. Feathers main PID was 816, started as
  `/usr/bin/node /opt/app/feathers/index.js`; `User`/`Group` are unset, and
  `ps` confirmed the process runs as root.
- Feathers listens on `127.0.0.1:13010`; coord Sidecar health/API remain on
  `127.0.0.1:13013`; existing authenticated relay listeners remain on
  `100.116.134.114:13011` and `:13012`; HTTPS listens on TCP 443.
- `/app/CoordMCP/` returned HTTP 404. Existing `/app/RemoteControlMCP3/`
  returned HTTP 401 without credentials; its OAuth authorization-server
  metadata returned the expected issuer, endpoints, and `S256` PKCE support.
- Active OpenResty includes `/etc/openresty/conf.d/*.conf`. The active
  `lieyanbang.conf` resolves to
  `/usr/local/openresty/nginx/conf/conf.d/lieyanbang.conf`; both paths had
  SHA-256 `4c9c8ff884a004649bf8349b997cc24d441f28840a3357924c7e27e8a0e8c9a4`.
  `sudo -n openresty -t` exited 0. Targeted route inspection found the three
  existing RemoteControlMCP prefixes and no CoordMCP route.
- Feathers host entry `/opt/app/feathers/index.js` SHA-256 was
  `057a41549b669406d045d7c017e59e43d37056fd1752239e8f07c0ee6862eea2`.
  The host operator manual says new service projects are mounted by this
  shared host entry. Adding the requested module therefore requires a
  controlled restart of the shared Feathers process; this can briefly affect
  existing APIs and the 13011 DesktopCommander channel.
- The deployed DesktopCommander OAuth source SHA-256 was
  `505db886f21bcdecf800f5f297fa94af705cc0eda80262b55ce8feb9ebf65bd7`,
  matching the local reference source. The deployed MCP SDK version is
  `1.30.0`; the local reference lockfile records `1.25.1`, so runtime-specific
  MCP transport verification remains required before deployment.
- `node --test test/test-self-hosted-oauth.js
  test/test-self-hosted-entry-namespaces.js` in the reference worktree exited 0
  with 2/2 tests passing (S256 PKCE, resource binding, expiry, refresh-token
  rotation/reuse revocation, namespace isolation, metadata, and tool catalog).
  The reference worktree status was preserved.
- Relay secrets were not read. Metadata only confirmed the sidecar client
  secret is permission-restricted under `/etc/coord-relay`; the existing
  relay remains a fixed-identity, narrow RPC path. No production task was
  claimed or mutated by this audit.
- Installed operator manuals were rendered to PID-scoped `/tmp` scratch files
  and removed in the same SSH command; no service, production data, or
  configuration was changed by that read.
- The local DesktopCommanderMCP worktree contains existing user modifications
  and untracked files. It was inspected read-only and remains untouched.

## Deployment Guardrails

- Add only the `/app/CoordMCP/` public MCP/OAuth route and a dedicated module
  mounted by the existing Feathers host. Do not change any
  RemoteControlMCP routes, DesktopCommander device behavior, or port 13011.
- Keep the public MCP method list narrower than the Sidecar allowlist; no
  arbitrary JSON-RPC, task creation, task cancellation, or batch management.
  `tasks/list` must keep `skip_reclaim=true`; only the relay's fixed agent
  identity may claim, extend, or complete work.
- Keep OAuth issuer/resource, DCR clients, token state, audit log, and MCP
  request journals isolated under a dedicated data path. Never copy credential
  values into this repository or logs.
- Return transport uncertainty as an MCP error, never as a successful result;
  do not retry a dispatched mutating request automatically. Preserve the
  existing sidecar request ID on explicit reconciliation.
- Before deployment, save hashes and protected backups of the exact host entry
  and active OpenResty config. Validate module/OAuth/MCP behavior in isolation,
  use atomic same-filesystem installs, run `openresty -t`, and only then do a
  controlled Feathers restart and OpenResty reload. Verify 401 challenge,
  OAuth metadata/DCR/PKCE, MCP initialize/tools/list/call, existing route
  regressions, sidecar round-trip, and rollback health.

## Remaining F3 Gates

- Implement and test the standalone CoordMCP module with strict tool schemas,
  authorization, request/response bounds, timeouts, concurrency, and safe
  uncertain-result handling.
- Verify OAuth positive and negative flows, resource binding, token expiry,
  and separate token state using temporary test data.
- Verify authenticated public `initialize` and `tools/list`, then perform a
  controlled read-only Arasaka query. Create and use an isolated test batch
  only after the authorization scope is validated.
- Preserve all existing DesktopCommanderMCP routes and device connectivity;
  complete the actual ChatGPT Web connector authorization for the final E2E
  gate. Until then F3 cannot be marked PASS.

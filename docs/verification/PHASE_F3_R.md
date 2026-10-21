# Phase F3-R: Cloudflare Access identity authorization

Recorded 2026-10-22 09:55 Australia/Brisbane (2026-10-21 23:55 UTC).
Overall result: **PARTIAL**. R1 implementation and R2 local verification pass.
R0 is partial because the actual Access application/policy cannot be read with
the available Cloudflare credential. R3 production deployment and R4 ChatGPT
Web authorization remain **NOT_RUN** at this revision. Phase F3 stays PARTIAL.

`TASK.md` was read-only throughout; SHA-256 remained
`780ee0eabbee510cdd7ad6c33d561698b19075d550e20d4d91b2a9725775fef8`.
No production service, Access policy, OpenResty configuration, Feathers code,
Sidecar, coord database, or 13011/13012 listener was changed in this revision.

## R0: Read-only audit

Repository baseline:

- Branch `codex/phase-f-deployment`, HEAD
  `305dd539440950fbc6c387989fd83ff943025f50`.
- `fork` is `https://github.com/lieyanbang/coord`; `origin` is
  `https://github.com/DmarshalTU/coord`.
- Existing `.gitignore`, `TASK.md`, `.desktop-commander-artifacts/`, and
  `relay/coord-mcp/node_modules/` were preserved. `TASK.md` was not edited.
  The pre-existing F3-R addition in `docs/PLAN.md` was read before updating it.

Production audit used direct SSH, not an MCP tool. At
`2026-10-21T23:37Z`, `feathersjsapp.service`, `openresty.service`,
`coord-relay.service`, and `tailscaled.service` were active/running with
`NRestarts=0`. Feathers PID was `90625`; OpenResty PID `47581`; Sidecar PID
`90506`; Tailscale PID `824`. The Sidecar returned
`{"state":"READY","agentConnected":true,"pending":0}`. Listeners remained
`127.0.0.1:13010`, `127.0.0.1:13013`, and the existing Tailscale listeners
`100.116.134.114:13011`/`:13012`.

Read-only public URL matrix at `2026-10-21T23:51Z` (`curl.exe`, no redirects
followed):

| GET path | HTTP | Meaning |
| --- | ---: | --- |
| `/app/CoordMCP/` | 401 | OAuth Bearer challenge from CoordMCP |
| `/app/CoordMCP/.well-known/oauth-protected-resource/mcp` | 200 | Public protected-resource metadata |
| `/app/CoordMCP/.well-known/oauth-authorization-server` | 200 | Public authorization-server metadata |
| `/app/CoordMCP/oauth/authorize` | 400 | Feathers response; no Cloudflare Access challenge |
| `/app/RemoteControlMCP/` | 302 | Existing Cloudflare Access login challenge |
| `/app/RemoteControlMCP2/` | 302 | Existing Cloudflare Access login challenge |
| `/app/RemoteControlMCP3/` | 401 | Existing Feathers OAuth challenge; Access remains bypassed |
| `/mcp` | 401 | Existing Feathers OAuth challenge |

The existing RemoteControl Access challenge identifies login host
`lieyanbang.cloudflareaccess.com` and its existing app audience
`1734f817a08554ce3f4f093c7d39067164470afb519f1c8019b704da550a51a4`.
This is effective-route evidence only; the signed Access `meta` parameter and
session cookie were not saved. Existing project documentation names the app
`lieyanbang-auth`, but its exact configured paths, policies, allowed identity,
session settings, and current app object were not independently readable.

`wrangler whoami` exited 0 for account
`bc9e9df66acd8acb888bc9fb75837931`; its reported scopes do not include Access
Apps/Policies read or edit. A direct read-only
`GET /accounts/{account_id}/access/apps` returned `success=true` with zero
items, despite the active public Access challenge above, so this response is
not treated as authoritative policy evidence. The `lieyanbang.com` zone resolves
to the same account. The current server's
`/etc/desktop-commander/devices.json` has null `accessTeamDomain` and
`accessApplicationAud` fields; the CoordMCP baseline also did not pass Access
configuration to its OAuth server. Exact owner identity and policy scope remain
**UNVERIFIED**. No DCR POST was made during R0 because it would persist a
production client record.

Before any production change, an additional root-only backup was created at
`/var/backups/coord-mcp-f3-r-20261021T233725Z/` (directory `700 root:root`). It
contains the active Feathers entry, OpenResty file, and both deployed CoordMCP
files. Backup SHA-256 matched the live source at capture:

| File | SHA-256 |
| --- | --- |
| `feathers-index.js` | `b204299f2a10d868b21dd710bf6cce3522ee48147f1d1a830f09d537479372bd` |
| `lieyanbang.conf` | `8bcba6f3543623e4ee53aab748e7d6b4fcc33b98b3aaf3eeeb6ef3d01fc7ef7c` |
| `coord-mcp/index.cjs` | `d0438d074a9ee7f03c8171f719e2d0a63cfeaa8195bf04f29321872f5d1bb3e1` |
| `coord-mcp/oauth.cjs` | `9e4d3a2adf595149b3eb672250add56975c5c434d754fd434db23c7576479a8d` |

The parent Desktop Commander OAuth module was inspected locally and its hash
matched the deployed file exactly:
`505db886f21bcdecf800f5f297fa94af705cc0eda80262b55ce8feb9ebf65bd7`.
Its `validateAccessAssertion()` verifies RS256/JWKS, issuer, audience, expiry,
and not-before; the CoordMCP facade adds the owner identity allowlist and a
strict `nbf <= now` check. No RemoteControl route or shared OAuth source was
modified.

## R1: CoordMCP implementation

Only CoordMCP OAuth/configuration and its tests changed. The browser authorize
GET/POST now requires a verified `cf-access-jwt-assertion`; the source never
trusts the header without the parent signature/JWKS validation. It additionally
requires exact configured team issuer and application audience, valid `exp`
and `nbf`, and an allowlisted signed email. Identity is bound by `sub`, falling
back to the allowlisted signed email when an IdP omits `sub`.

The consent form has no password field and the overridden password verifier
always rejects. Authorize POST accepts only URL-encoded browser forms. A random
one-use CSRF value is stored only as a hash and bound
to the validated client, exact redirect URI, resource, scope, PKCE challenge,
and state. POST also requires same-origin `Origin`, the same Access identity,
and the same bound parameters. Authorization-code consumption is serialized
inside the OAuth state store so concurrent exchange can succeed only once.
Metadata, DCR, token/refresh endpoints, and MCP Bearer authentication keep their
separate protocol roles.

CoordMCP reads independent runtime values from:

- `COORD_MCP_ACCESS_TEAM_DOMAIN`
- `COORD_MCP_ACCESS_APPLICATION_AUD`
- `COORD_MCP_ACCESS_ALLOWED_EMAILS`

The first two must correspond to the actual Access application; the third is a
comma-separated exact email allowlist. Missing configuration fails closed with
HTTP 503 on authorize and does not block metadata or service startup. The
production values are not available yet and were not guessed.

Local source hashes:

| File | SHA-256 |
| --- | --- |
| `relay/coord-mcp/oauth.cjs` | `48406ef0f3fde42fcc4cf8cdc7def1d51f047845d38cce51f738c7c53a51423c` |
| `relay/coord-mcp/index.cjs` | `e2a4c125f6742f915662707ff123f3eb0a3e3ec6b64ae2818baf1c3ea7cd5db3` |
| `relay/coord-mcp/host-oauth.integration.cjs` | `54da566c6c4943d2ebb38a832607d10243a26d2de4a827789bae3839ccbb1b9f` |
| `relay/coord-mcp/oauth-access.test.cjs` | `85fda12faa2cfd4a0682d8b3ccbc7a5b5c13f912aac15cf052dca1705031b5d8` |

## R2: Automated verification

`npm test` in `relay/coord-mcp` exited 0: **12/12 passed**. The new mock-JWKS
RSA tests cover no assertion, wrong signature, issuer, AUD, expired and future
`nbf`, non-owner email, missing `sub`, bad callback, missing/foreign Origin,
password fallback rejection, CSRF and parameter substitution, and concurrent
one-time code redemption. Missing Access configuration returns 503.

The host integration ran against the exact Desktop Commander OAuth source hash
listed above, with an ephemeral RSA key/JWKS and an isolated OAuth state
directory. It exited 0 and covered Access signature/owner validation,
password-free consent, CSRF binding, DCR, PKCE, resource/scope binding,
refresh/expiry, MCP initialize/tools/list, and a one-time authorization code.
The Sidecar was an isolated stub; no production task was read or written. Output
is in [`stage-f3-r-host-oauth.log`](stage-f3-r-host-oauth.log).

`pwsh -NoProfile -File scripts/verify.ps1 -Stage f3-r` exited 0 on Windows at
`2026-10-21T23:54:34Z` for all five checks:

| Check | Exit | Log |
| --- | ---: | --- |
| `cargo fmt --all -- --check` | 0 | `stage-f3-r-checks.json` (no stdout log) |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | 0 | `stage-f3-r-clippy.log` |
| `cargo test --all-features --locked` | 0 | `stage-f3-r-test.log` |
| `cargo build --release --locked` | 0 | `stage-f3-r-build.log` |
| `npm test --prefix relay/coord-mcp` | 0 | `stage-f3-r-coord-mcp-tests.log` |

Fork Windows Actions has not yet run for this revision. Local checks do not
substitute for that run.

## R3 and R4

- **R3 NOT_RUN:** no production file was deployed and no service was restarted
  in this revision. The Access app/path/policy cannot yet be read or backed up,
  and the exact AUD/owner email are not verified. Deploying a positive login
  configuration with guessed values would not be safe. Existing production
  backup is ready for a later controlled deployment. Cloudflare policy writes,
  OpenResty reload, and Feathers restart were not attempted.
- **R4 NOT_RUN:** no ChatGPT Web login or public authenticated MCP call was
  performed. Simulated JWTs and local MCP calls are only automated evidence.

Next required evidence is an authorized Cloudflare credential with Access Apps
and Policies read/edit for the `lieyanbang.com` account (or an authenticated
dashboard session), so the exact `lieyanbang-auth` policy can be reviewed and
backed up, then the CoordMCP authorize path can be protected and the actual
owner identity confirmed. Only after that can R3 production verification and
the user-facing R4 browser flow proceed. F3 remains **PARTIAL** until both pass.

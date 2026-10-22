# Phase F3-R: Cloudflare Access identity authorization

Recorded 2026-10-22 10:19 Australia/Brisbane (2026-10-22 00:19 UTC).
Overall result: **PARTIAL**. R1 and R2 pass; R0 remains partial because the
actual Access application/policy is not readable with the available Cloudflare
credential. R3 code deployment and Feathers reload are complete, but the
authorize path is not protected by a verified Access application and returns
503 with missing Access configuration. R3 is therefore **PARTIAL**. R4 ChatGPT
Web authorization remains **NOT_RUN**. Phase F3 stays PARTIAL.

`TASK.md` was read-only throughout; SHA-256 remained
`780ee0eabbee510cdd7ad6c33d561698b19075d550e20d4d91b2a9725775fef8`.
The later controlled production follow-up restarted only
`feathersjsapp.service`. No Access policy, OpenResty configuration, Sidecar,
coord database, Arasaka service, or 13011/13012 listener was changed.

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

Fork Windows Actions run
[39696859131](https://github.com/lieyanbang/coord/actions/runs/39696859131)
completed successfully for commit
`a640ce8427ae096f2f5ec488a8d16d9e8c074219` at `2026-10-22T00:03:08Z`.
The Windows-2022 job completed with no failed steps. Artifact
`coord-windows-2022` (`12390883361`, 4,226,357 bytes) is present and not
expired. The run and artifact metadata were read from GitHub's public Actions
API at `2026-10-22T00:13Z`.

The complete local gate was rerun before this documentation closeout with
`pwsh -NoLogo -NoProfile -File scripts/verify.ps1 -Stage f3-r`, from
`00:17:31Z` through `00:18:10Z`. All five checks exited 0: fmt, clippy,
all-feature tests, release build, and CoordMCP tests (12/12). Fresh command
timings and output are in `stage-f3-r-checks.json` and the matching
`stage-f3-r-*.log` files.

## R3: Controlled production deployment

**Result: PARTIAL.** Deployment and runtime loading succeeded; actual Cloudflare
Access policy integration is still missing and is not inferred from mock tests
or the existing RemoteControl application.

The root-only rollback snapshot created before deployment remains at
`/var/backups/coord-mcp-f3-r-20261021T233725Z/` (`700 root:root`). Its original
CoordMCP source hashes were verified again before the reload:
`coord-mcp/index.cjs`=`d0438d074a9ee7f03c8171f719e2d0a63cfeaa8195bf04f29321872f5d1bb3e1`,
`coord-mcp/oauth.cjs`=`9e4d3a2adf595149b3eb672250add56975c5c434d754fd434db23c7576479a8d`.
The current deployed source hashes match the tested revision:
`index.cjs`=`e2a4c125f6742f915662707ff123f3eb0a3e3ec6b64ae2818baf1c3ea7cd5db3`,
`oauth.cjs`=`48406ef0f3fde42fcc4cf8cdc7def1d51f047845d38cce51f738c7c53a51423c`.

At `2026-10-22T00:09Z`, direct SSH commands
`node --check /opt/app/feathers/realtimetext/lib/coord-mcp/index.cjs` and
`node --check /opt/app/feathers/realtimetext/lib/coord-mcp/oauth.cjs` both
exited 0. At `00:10Z`,
`sudo -n systemctl restart feathersjsapp.service` exited 0. Read-back showed
`active/running`, MainPID `94312`, `NRestarts=0`, start time
`2026-10-22 00:10:00 UTC`. `systemctl is-active feathersjsapp.service
openresty.service coord-relay.service tailscaled.service` exited 0 and printed
`active` for all four units. The post-restart process-environment check
`sudo -n grep -aoE "COORD_MCP_ACCESS_(TEAM_DOMAIN|APPLICATION_AUD|ALLOWED_EMAILS)"
/proc/94312/environ | sort -u` exited 0 with no matching variable names.

The post-restart listener check (`sudo ss -ltnp`, exit 0) showed:

| Port | Binding / owner |
| --- | --- |
| 13010 | `127.0.0.1`, Feathers Node PID `94312` |
| 13011 | `100.116.134.114`, existing OpenResty workers |
| 13012 | `100.116.134.114`, Sidecar Node PID `90506` |
| 13013 | `127.0.0.1`, Sidecar Node PID `90506` |

The established-session check still showed BT-7274 and Arasaka sessions on
13011 and Arasaka `100.89.6.126` on 13012. OpenResty, Sidecar, Tailscale,
13011/13012, and Arasaka were not restarted or reconfigured. Sidecar
`GET http://127.0.0.1:13013/healthz` returned HTTP 200 with
`state=READY`, `agentConnected=true`, `pending=0` at `00:11:09Z`.

For the real Sidecar-to-coord path, a one-shot Node probe ran as `coord-relay`
over direct SSH. It loaded the bearer from the existing protected file in
memory and sent only `tasks/list` with `skip_reclaim=true`, `limit=1` to
`127.0.0.1:13013/v1/rpc`. It returned HTTP 200, `state=ok`, one item, exit 0.
The task database was not mutated; the Sidecar's normal request journal records
this read-only call. No bearer or task contents were emitted.

Arasaka read-only checks also passed at `00:14Z`: Servy CLI
`status --name CoordRelayAgent --quiet` returned Running, exit 0. The local
coord health command exited 0 and returned `READY`, zero consecutive failures,
4 ms total latency, `tasks_list=PASS` and `sqlite_read_only=PASS`; its temporary
status file was removed. No Arasaka service or machine restart occurred.

Public requests used
`curl.exe --max-time 25 --silent --show-error --dump-header - --output NUL`
without following redirects; the matrix was rechecked at approximately
`2026-10-22T00:15Z` (each curl exit 0):

| GET path | HTTP | Observation |
| --- | ---: | --- |
| `/app/CoordMCP/` | 401 | OAuth Bearer challenge |
| `/app/CoordMCP/.well-known/oauth-protected-resource/mcp` | 200 | Public metadata |
| `/app/CoordMCP/.well-known/oauth-authorization-server` | 200 | Public metadata |
| `/app/CoordMCP/oauth/authorize` | 503 | Access configuration missing; fail closed, no redirect |
| Same authorize URL with `cf-access-jwt-assertion: forged` | 503 | No authorization code issued |
| `/app/RemoteControlMCP/` | 302 | Existing Access login host |
| `/app/RemoteControlMCP2/` | 302 | Existing Access login host |
| `/app/RemoteControlMCP3/` | 401 | Existing Feathers challenge unchanged |
| `/mcp` | 401 | Existing Feathers challenge unchanged |

`wrangler whoami` exited 0 for Wrangler `4.128.0`; the active OAuth token
reported account/zone/Workers scopes but no Access Apps/Policies read or edit
scope. The prior read-only Access Apps API response contained zero items and
was not treated as authoritative. The exact app paths, policies, AUD, team
domain and allowed owner identity therefore remain **UNVERIFIED**. No Access
policy was changed and no positive Access JWT configuration was guessed. The
authorize URL currently has no Cloudflare Access redirect, so R3 cannot pass.
OpenResty was not reloaded; no RemoteControl route or shared OAuth logic was
changed.

## R4: ChatGPT Web end-to-end

**NOT_RUN.** No ChatGPT Web login/consent, real Access identity assertion,
public OAuth token exchange, or authenticated public MCP tool call was
performed. Mock JWT tests and the production Sidecar read-only probe do not
substitute for this gate.

Next steps: provide an authenticated Cloudflare dashboard session or a
credential with Access Apps/Policies read and edit for the `lieyanbang.com`
account. Read and back up the actual `lieyanbang-auth` application/policy,
confirm its team domain, AUD and allowed owner identity, then protect only the
CoordMCP browser authorize route and set the matching protected Feathers
environment values. Re-run the real positive/negative Access flow and R3
regression. Only then can ChatGPT Web complete the real OAuth consent, token
exchange, `initialize`, `tools/list`, read-only `tasks/list`, and isolated
write/readback. F3 remains **PARTIAL** until all required gates pass.

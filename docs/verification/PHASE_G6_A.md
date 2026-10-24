# Phase G6-A — authenticated connector bindings

Date: 2026-10-24 Australia/Brisbane. Result: **PARTIAL**. Local implementation
and isolated synthetic identity tests PASS. The real ChatGPT conversation
isolation gate remains **BLOCKED / PLATFORM_LIMITATION**; production enablement
and six real worker connections are **NOT_RUN**. No production source,
configuration, queue, task or service was changed by this subphase. The browser
exercise did perform a normal reconnection of the existing Primary account,
changing OAuth grant state; it was not a credential-isolation deployment.

## Current identity evidence

The direct SSH read-only audit of the actual Feathers host exited 0 at
2026-10-24T10:00:06Z. `/var/lib/coord-mcp/oauth.json` contained two registered
clients, 14 access-token records and 14 refresh-token records; no access token
was still live. All access-token records had the same client SHA-256
`eeb595ffaeec3cc157243f766c320f4133f1d9a6af05ad21ee8fb373850122ba`.
Only counts, field names, hashes and times were saved in
`stage-g6-oauth-readonly.json`; raw client IDs, bearer/refresh tokens, owner
identity, OAuth codes, PKCE values and lease tokens were never emitted.

The deployed base OAuth validator returns these authenticated access fields:
`issuer`, `audience`, `resource`, `clientId`, `scope`, `issuedAt`, `expiresAt`,
plus `tokenType`. It supplies no conversation identifier. Access tokens do not
carry a refresh family; `familyId` exists only in refresh-token records. Store
keys are SHA-256 indexes, so the store cannot reconstruct an existing bearer.

The [official OpenAI authentication page](https://developers.openai.com/plugins/build/auth)
was searched and opened in this run. It describes OAuth clients reused per
MCP connection, with tokens sent on subsequent MCP requests. It does not
establish a protected identity for each conversation. Owner identity, client
registration, connector account and conversation therefore remain distinct
concepts. A copied bearer credential remains usable by its holder; this work
does not claim to change the OAuth bearer model.

The operator created two real independent ChatGPT conversations and attempted
strictly read-only Coord list calls (urgent/limit 1 and high/limit 1). Both
initially displayed "Unable to load this reply"; one was refreshed and still
displayed the error. After a normal visible reconnection of the existing
Primary account, conversation A claimed success without confirming a row count
and conversation B claimed success with zero rows. Those UI statements did not
expose a per-request authenticated principal or independently prove isolation.
The existing plugin settings exposed one primary connected account and a
"connect another account" option; a globally selectable account is not a
protected per-conversation credential.

A second read-only SSH snapshot at 2026-10-24T10:06:08Z exited 0:
`stage-g6-oauth-after-browser.json` reports two clients, 15 access-token and 15
refresh-token records, one live access token, and the same single client hash
across access-token records. This shows that issuance occurred in that window;
it cannot attribute the new token to either failed UI request or prove that two
conversations used distinct credentials.

The final authoritative read-only server audit exited 0 at
2026-10-24T10:16:48Z (`stage-g6-browser-server-audit.json`). From 09:50Z onward,
the Sidecar recorded four `tasks/list` dispatches and four completed results,
with no other RPC methods. OpenResty recorded one authorization POST returning
302, two token POSTs returning 200, and four MCP POSTs returning 200. The store
then contained two clients, 16 access records and 16 refresh records; every
access record still referenced the same client digest above. These records
prove that read-only tool calls occurred, but cannot attribute each call to a
specific conversation or establish distinct protected credentials. No
conversation identifier was present. The account selector was globally
available. Six protected contexts, reconnect stability and actual
cross-conversation denial remain **UNVERIFIED / NOT_RUN**. This gate is blocked
by the available platform connection model, not by missing browser permission.

## Implementation and isolated checks

`relay/identity.cjs` adds an administrator-owned policy. The default is disabled
and preserves the fixed legacy worker and its existing tool schema. An enabled
policy pins authenticated client SHA-256 plus authenticated token SHA-256 to
exactly one A0–A5, batch, explicit scope set and bounded TTL (maximum 24 hours).
Duplicate clients/tokens/roles and legacy/new overlap fail startup. Token
rotation fails closed until the operator verifies and replaces the pin. The
OAuth facade computes the token digest only after normal token validation.

MCP arguments still cannot select `agent_id` or an arbitrary RPC. The host
derives the role from authenticated context. It signs the exact request UUID,
method, full parameters and bounded principal context (at most 60 seconds).
Sidecar and Windows Relay Agent independently check policy, signature, role,
batch, scope and expiry. Exact task operations first read the immutable batch
on trusted loopback and reject a foreign batch without returning task content.
Core lease fencing remains the atomic authority for task ownership.

The persistent Sidecar journal includes identity in its keyed request hash.
Same-ID and cross-identity collisions return only uncertain/conflict metadata;
no cached response body or foreign lease is returned and no RPC is redispatched.
Legacy requests cannot touch protected batches; legacy global reads filter
those batches, and legacy scheduler claims must name a batch while policy is
enabled. A real-daemon regression verifies that an unsigned batchless claim
leaks no payload or lease and leaves all protected tasks pending. Bound reads
remove lease-token fields; claims expose only their
authenticated worker's lease. Malformed proof objects, changed parameters,
wrong role/batch/scope, expired token/context, missing hop policy and forged
agent arguments are rejected. Nothing treats a progress string as liveness.

| Command | Exit | Evidence |
| --- | ---: | --- |
| `npm test --prefix relay/coord-mcp` | 0 | 14/14; `stage-g6-identity-mcp.log` |
| `node --test relay/relay.test.mjs relay/live-coord.test.mjs` | 0 | 20/20; `stage-g6-identity-relay-all.log` |
| Direct SSH Python OAuth/server field/count audit | 0 / 0 / 0 | Three JSON snapshots above |

These tests include an actual temporary Windows coord daemon, authenticated
Sidecar TCP session and Relay Agent. OAuth identities in the automated tests
are explicitly synthetic. They are not six real ChatGPT sessions and do not
close Gate A. Production ACLs for the new private policy/signing key are also
**NOT_RUN**. Full stage/Actions gates are recorded by G6-D.

The Relay Agent also supports the protected main-RPC key needed by G6-B using
`coordAuthKeyFile`. The tests verify legacy header absence, authenticated
loopback delivery, rejection of malformed/oversize/relative key files and
non-loopback URLs, and refusal to follow redirects. Neither the journal nor
the returned result contains the key. These tests do not prove production
Windows ACL isolation.

## Remaining acceptance prerequisite

Use the exact isolated setup and UI steps in
`docs/phase-g/IDENTITY_BINDINGS.md`. Create two independently registered test
connector contexts against an isolated endpoint only once the platform offers
protected authenticated contexts that other conversations cannot select or
reuse. Authorize them visibly, record only authenticated client/token digests
and prove that either conversation cannot choose the other's connection. Then
repeat for all six.
If ChatGPT exposes the same reusable account to both conversations, Gate A is
BLOCKED. The minimum safe alternative is separate protected ChatGPT user/profile
contexts or an explicitly trusted non-isolated logical-worker model; neither
can be silently substituted for the requested conversation security gate.
Do not deploy or create a production canary while A or B remains open.

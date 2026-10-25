# Phase R2 — MCP, Relay and compatibility

Date: 2026-10-25 Australia/Brisbane. Result: **LOCAL INTERFACE GATE PASS**.
This gate covers local Rust MCP and Relay/sidecar code. Cross-layer trusted
operations against an isolated Core daemon remain an R3 gate.

## Interface changes

- Added six named operations to the Rust MCP bridge and Coord MCP Relay:
  `coord_claim`, `coord_progress`, `coord_complete`, `coord_release`,
  `coord_resume`, and `coord_status`.
- Public schemas do not accept lease tokens, progress sequence numbers,
  request UUIDs, or arbitrary RPC method names. The Relay generates request IDs
  and keeps the existing bounded request/response rules. It removes any token
  fields from trusted results and never retries a dispatched write.
- Trusted calls use OAuth/Cloudflare Access for the owner by default, plus the
  existing private local Relay bearer and authenticated connector. The optional
  v2 logical identity profile signs and enforces exact batch, worker, scope and
  expiry bindings. A `worker_ids: ["*"]` binding is supported only inside that
  explicit profile; a worker label is not a separate-conversation identity.
- Kept existing token-based Relay tools and v1 identity policy behavior. Added
  the `coord:release` scope for the new operation. The earlier volatile
  `managed-leases.cjs` experiment remains untouched as untracked historical
  data and is not a runtime dependency.
- Updated API and Relay docs with the default trust path, migration behavior,
  token boundary and accurate signature behavior.

## Verification evidence

The Rust R2 stage gate passed all required commands:

| Command | Exit | Evidence |
| --- | ---: | --- |
| `cargo fmt --all -- --check` | 0 | [stage-r2-fmt.log](stage-r2-fmt.log) |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | 0 | [stage-r2-clippy.log](stage-r2-clippy.log) |
| `cargo test --all-features --locked` | 0 | [stage-r2-test.log](stage-r2-test.log) |
| `cargo build --release --locked` | 0 | [stage-r2-build.log](stage-r2-build.log) |

Node checks also passed:

| Command | Result | Evidence |
| --- | --- | --- |
| `npm test --prefix relay/coord-mcp` | 18 passed, 0 failed | [stage-r2-coord-mcp-tests.log](stage-r2-coord-mcp-tests.log) |
| `node --test relay/relay.test.mjs relay/live-coord.test.mjs` with local `target/release/coord.exe` | 21 passed, 0 failed | [stage-r2-relay-tests.log](stage-r2-relay-tests.log) |

The Relay suite starts temporary loopback listeners and the live Core cases use
an isolated database and local release executable. It does not contact the
production daemon. The first Relay run exposed a test-fixture error: the test
spread the `progress` checkpoint into a strict `trusted_complete` parameter
object. The allowlist correctly rejected that extra field. The fixture was
changed to use the exact completion schema and the complete suite passed on
rerun; no production code change was needed for that failure.

Coverage includes fixed-method/parameter allowlists, signatures and wildcard
scope bindings, cross-batch and mismatched-worker rejection, OAuth owner mode
without per-worker policy, old token-based calls, token redaction, request
limits, uncertain writes without retry, real local Core legacy lifecycle and
Relay disconnect behavior.

## Remaining gates

The tests do not yet send a Phase R trusted claim/progress/complete lifecycle
through the actual Node Relay into the actual local Core daemon. R3 must add and
run that isolated end-to-end path, along with Core/Relay restart recovery,
broader migration fixtures, expired-lease and race scenarios, Node regressions,
and Windows Fork Actions artifact evidence. No production endpoint, OAuth
grant, formal Batch, Action Runner or remote service was queried or modified.

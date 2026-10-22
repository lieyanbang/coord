# Phase G5: Canary, Deployment and Acceptance

Result: **PARTIAL**. Local Windows acceptance passes. Fork Actions artifact
verification and production deployment are not yet complete; production
deployment is **NOT_RUN** because G1 and G3 production gates remain open.

## Local Windows Gate

Command: `pwsh -NoProfile -File scripts/verify.ps1 -Stage g5`.
Started `2026-10-22T16:53:24Z`; all steps exited **0**:

| Check | Exit | Evidence |
|---|---:|---|
| `cargo fmt --all -- --check` | 0 | `stage-g5-fmt.log` |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | 0 | `stage-g5-clippy.log` |
| `cargo test --all-features --locked` | 0 | `stage-g5-test.log` |
| `cargo build --release --locked` | 0 | `stage-g5-build.log` |
| `node --test relay/relay.test.mjs relay/live-coord.test.mjs` | 0, 13/13 | `stage-g5-relay-tests.log` |
| `npm test --prefix relay/coord-mcp` | 0, 12/12 | `stage-g5-coord-mcp-tests.log` |
| `python -m unittest discover -s scripts/phase_g -p test_phase_g_tools.py -v` | 0, 6/6 | `stage-g5-phase-g-tools.log` |

The local Release executable SHA-256 is
`7e905b97831021114f319be8edcc644a33308cfb0ae52142eadc5908f1ee3fac`. This is
not a deployed binary or a GitHub Actions artifact. Fork Actions run and
artifact ID/hash are **PENDING** until the scoped commit is pushed and its
workflow completes.

## Production Boundary

Direct SSH read-only checks at `2026-10-22T16:37:12Z` (Arasaka) and
`16:37:22Z` (Sidecar host) found the existing services healthy; details are in
`PHASE_G0.md`. A later Arasaka `coord.exe --version` query at `16:43:50Z`
returned `0.4.1`, exit **0**. No Phase G file, database, Windows service,
Sidecar, Cloudflare policy, listener, queue, or production Agent was changed.
No production backup was needed because deployment did not start; no service
was restarted and Arasaka was not rebooted.

Production deployment and the six-agent Canary are withheld until: (1) an
independently protected conversation-bound identity exists and passes
positive/negative tests; (2) Action execution uses a verified least-privilege
account and ACLs with reviewed native publisher binaries; and (3) the current
production OAuth source hash discrepancy is reconciled while F3-R remains
PARTIAL. No formal 121-task Batch or Agent was started. Human/platform
intervention is required to resolve the identity and production-account gates.

# Phase G5: Canary, Deployment and Acceptance

Result: **PARTIAL**. Local Windows acceptance passes. Fork Actions artifact
verification and production deployment are not yet complete; production
deployment is **NOT_RUN** because G1 and G3 production gates remain open.

## Local Windows Gate

Command: `pwsh -NoProfile -File scripts/verify.ps1 -Stage g5`.
Latest local re-run started `2026-10-22T17:03:25Z`; all steps exited **0**:

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
`7e905b97831021114f319be8edcc644a33308cfb0ae52142eadc5908f1ee3fac`. It is
not a deployed binary.

## Fork GitHub Actions

Result: **PASS** for the automated Windows gate and artifact production.

- Fork branch: `codex/phase-g-agent-actions`.
- Commit: `8a71b0c7ae9d32491cde525640426b28069dccf0`.
- Workflow run: `39818335937`, push event, Windows 2022, conclusion `success`;
  run URL: https://github.com/lieyanbang/coord/actions/runs/39818335937.
- The job completed at `2026-10-22T17:01:32Z`. fmt, clippy, Rust tests,
  release build, Relay tests, Coord MCP tests, Phase G utility tests, and
  artifact upload all concluded `success`.
- Artifact `coord-windows-2022`, ID `12444295367`, was not expired when checked.
  Downloaded ZIP SHA-256:
  `8b31dc865e074c370872d8a4f148dbdd2c31657db89aaf1b4919c95f7682f9b5`.
  Its sole entry was `coord.exe` (10,760,192 bytes), SHA-256
  `15fcf44c34d299de43870a2e28c29efd0a006552d36e5200a566fbe72885327f`.
- The local Release binary hash differs from the Actions artifact hash. The
  source commit and both build results are identified above; reproducible
  byte-for-byte builds were not established.

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
PARTIAL. The CI and artifact sub-gate passes, but overall G5 remains
**PARTIAL** and production deployment is **NOT_RUN**. No formal 121-task Batch
or Agent was started. Human/platform intervention is required to resolve the
identity and production-account gates.

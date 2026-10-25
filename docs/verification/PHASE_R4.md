# Phase R4 — Fork artifact and delivery gates

Date: 2026-10-25 Australia/Brisbane. Result: **PENDING RETRY** after the first
Windows Actions run exposed a clean-runner dependency-order issue. The R0–R3 local gates are
recorded in [PHASE_R0_ARCHITECTURE.md](PHASE_R0_ARCHITECTURE.md),
[PHASE_R1.md](PHASE_R1.md), [PHASE_R2.md](PHASE_R2.md), and
[PHASE_R3.md](PHASE_R3.md).

## Implementation commit and Fork run

- Fork: `lieyanbang/coord`; branch: `codex/phase-r-execution-2026-10-25`.
- Implementation/test commit: `004b93a79cacac95bf04601c34c64a2a8558cb95`
  (`test: exercise Phase R recovery lifecycle`). The branch was pushed with a
  normal fast-forward push; `origin` was not changed.
- First Windows Actions run:
  [40194822262](https://github.com/lieyanbang/coord/actions/runs/40194822262),
  failed. Rust fmt/clippy/tests, Release build, health-monitor rehearsal and
  low-privilege Action Runner rehearsal succeeded. Relay tests then failed at
  module load with `MODULE_NOT_FOUND` for Express because `npm ci` ran later in
  the workflow. The remaining MCP/Python steps were skipped; only the
  always-uploaded G6 evidence was created, so there is no matching
  `coord-windows-2022` artifact to accept from this run.
- Corrective change in progress: move the existing `npm ci --prefix
  relay/coord-mcp` workflow step before Relay tests. A fresh Windows Actions
  run must prove the corrected order and all later steps.
- Run result, artifact name/ID, ZIP SHA-256, executable SHA-256, and downloaded
  artifact execution evidence will be recorded after the retry completes.

## Local R4 gate

The repository gate passed before the R4 documentation/workflow commit:

| Command | Exit | Evidence |
| --- | ---: | --- |
| `cargo fmt --all -- --check` | 0 | [stage-r4-fmt.log](stage-r4-fmt.log) |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | 0 | [stage-r4-clippy.log](stage-r4-clippy.log) |
| `cargo test --all-features --locked` | 0 | [stage-r4-test.log](stage-r4-test.log) |
| `cargo build --release --locked` | 0 | [stage-r4-build.log](stage-r4-build.log) |

The phase wrapper result and exact command/timing JSON are in
[stage-r4-gate.log](stage-r4-gate.log) and [stage-r4-checks.json](stage-r4-checks.json).

## Local artifact acceptance

Pending successful retry and exact Fork artifact download. The local R3 release executable is not a
substitute for the matching CI artifact. After download, run the artifact's
`coord.exe version` and the isolated Relay/Core lifecycle suite with
`COORD_TEST_BIN` set to that exact extracted executable; record all hashes and
exit codes.

## Stop line and remaining evidence

- The synthetic MCP test uses an isolated database, loopback services and a
  synthetic OAuth owner token. It is not a real ChatGPT browser session.
- No real ChatGPT OAuth grant/refresh, real conversation/context-loss flow,
  production route, production daemon, formal Batch, or production Canary was
  changed or tested. The TASK.md authorization excludes those production
  actions; real ChatGPT evidence is **NOT_RUN**, not inferred from local tests.
- Phase R production acceptance therefore remains **PARTIAL** until the
  separately authorized real-client and deployment gates can be reviewed.

# Phase R4 — Fork artifact and delivery gates

Date: 2026-10-25 Australia/Brisbane. Result: **FORK WINDOWS GATE AND ARTIFACT
ACCEPTANCE PASS; PHASE R OVERALL PARTIAL**. The R0–R3 local gates are recorded
in [PHASE_R0_ARCHITECTURE.md](PHASE_R0_ARCHITECTURE.md),
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
- Moved the existing `npm ci --prefix relay/coord-mcp` workflow step before
  Relay tests. The corrected run is
  [40195567861](https://github.com/lieyanbang/coord/actions/runs/40195567861)
  for commit `4e308edd2f8cd7c9852eb4a1eefd7e4bc935c7ed`; its Windows 2022 job
  completed successfully. All workflow steps passed: fmt, clippy, full Rust
  tests, Release build, health-monitor rehearsal, low-privilege Runner
  rehearsal, Relay protocol/lease-fencing tests, MCP OAuth/HTTP tests, Phase G
  Python utility tests, and both artifact uploads.
- Artifact `coord-windows-2022`, ID `12600096932`, size 6,571,823 bytes,
  matches run commit `4e308edd2f8cd7c9852eb4a1eefd7e4bc935c7ed`. The downloaded
  ZIP SHA-256 is
  `a54d3c62af79b148aaa1f2563e13ffa3f47b51c855b341aa83b93f9f65b641d7`.
- Downloaded executable SHA-256 values:
  - `coord.exe`: `e7c3695a21f88274cb84992f99a4ce9ce5b2f2bfb229a2768b3c6ccd85bdd13d`
  - `coord-action-runner.exe`: `114347d7024e08e4451745526023c750c6517d3a5d07f34065906ec63770566e`
  - `coord-emote-action.exe`: `4a61b9efa529a383c04fb66d5efabaa3e7364526b7eb4212d4321dbf132db7b5`
- The exact downloaded `coord.exe version` exits 0 and reports `coord 0.4.1`;
  both companion executables' `--help` commands exit 0. With `COORD_TEST_BIN`
  pointed at that downloaded `coord.exe`, the isolated Relay/Core/identity
  suite passes 27/27 (exit 0). See
  [stage-r4-artifact-manifest.json](stage-r4-artifact-manifest.json),
  [stage-r4-artifact-version.log](stage-r4-artifact-version.log),
  [stage-r4-artifact-coord-action-runner.exe.log](stage-r4-artifact-coord-action-runner.exe.log),
  [stage-r4-artifact-coord-emote-action.exe.log](stage-r4-artifact-coord-emote-action.exe.log),
  and [stage-r4-artifact-relay.log](stage-r4-artifact-relay.log).

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

The exact artifact was downloaded to the ignored local path
`target/phase-r-ci-40195567861/`. Its archive digest matches GitHub's artifact
metadata. The downloaded Core executable passed its version check and the
27-case isolated Relay/Core/identity suite; the CI workflow had already run the
same Core and companion binaries through its release and Runner acceptance
steps.

## Stop line and remaining evidence

- The synthetic MCP test uses an isolated database, loopback services and a
  synthetic OAuth owner token. It is not a real ChatGPT browser session.
- No real ChatGPT OAuth grant/refresh, real conversation/context-loss flow,
  production route, production daemon, formal Batch, or production Canary was
  changed or tested. The TASK.md authorization excludes those production
  actions; real ChatGPT evidence is **NOT_RUN**, not inferred from local tests.
- Phase R production acceptance therefore remains **PARTIAL** until the
  separately authorized real-client and deployment gates can be reviewed.

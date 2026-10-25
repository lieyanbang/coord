# Phase R1 — Core reliability

Date: 2026-10-25 Australia/Brisbane. Result: **LOCAL CORE GATE PASS**.
This is isolated local implementation evidence. It does not upgrade a deployed
daemon or establish production Batch status.

## Implementation

- Added durable `task_attempts` rows keyed by `(task_id, attempt_no)`, including
  worker, state, start/end times, latest checkpoint, progress sequence, result,
  completion request and disposition. Existing fenced active attempts and their
  token-keyed checkpoint are backfilled transactionally where the old task row
  proves its owner and generation. Rows without a provable owner do not get a
  fabricated attempt.
- Added independent `failure_count`, `interruption_streak` and
  `needs_attention` task fields. `attempt_no` and `attempt_count` remain
  monotonic/history fields. New business failures consume the retry budget;
  release and lease expiry do not. Interruption backoff is capped, progress
  resets the streak, and eight consecutive interruptions surface attention.
- Added Core trusted claim/progress/complete/release/resume/status operations.
  They use the existing SQLite transaction/fence paths, keep the lease token
  inside Core, limit claim to the requested batch, and reject stale attempt
  writes. Resume is read-only. Reclaimed attempts inherit the last durable
  checkpoint so the next worker can continue from it.
- Kept legacy token-based APIs available. Trusted A2A RPCs reject a network
  peer that is not loopback; the real server provides peer information to this
  guard. Trusted MCP/Relay surfaces are covered by R2.

## Migration rule and limit

The new counter is initialized once, in the same additive migration transaction.
For `claimed` and `submitted` rows it uses `max(attempt_count - 1, 0)`, since the
current attempt has not yet failed. Other states retain `attempt_count` as the
conservative legacy budget already consumed. Existing `attempt_no` and
`attempt_count` values are not reset. Old databases cannot distinguish
interruptions from business failures, so Phase R cannot reconstruct that
historical split; the migration preserves the old retry budget instead of
silently granting more retries. New interruptions after migration are
independent from `failure_count`.

## Verification evidence

The repository stage gate completed successfully:

| Command | Exit | Evidence |
| --- | ---: | --- |
| `cargo fmt --all -- --check` | 0 | [stage-r1-fmt.log](stage-r1-fmt.log) |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | 0 | [stage-r1-clippy.log](stage-r1-clippy.log) |
| `cargo test --all-features --locked` | 0 | [stage-r1-test.log](stage-r1-test.log) |
| `cargo build --release --locked` | 0 | [stage-r1-build.log](stage-r1-build.log) |

The full test run includes 8/8 `trusted_scheduler` integration tests and the
A2A unit test that rejects non-loopback trusted RPCs. The focused cases cover
SQLite restart and migration, preserved result/checkpoint data, automatic
progress sequence and replay, idempotent completion, changed-result conflict,
release/expiry failure-budget behavior, stale attempt rejection, six-worker
atomic claims, checkpoint handoff after reassignment, and the real HTTP JSON-RPC
path with lease-token redaction plus legacy claim compatibility. The migration
fixture currently covers active, pending and completed rows and runs
`PRAGMA integrity_check`; the wider state/receipt/action fixture remains in R3.

## Remaining gates

R1 proves local Core behavior only. R2 must finish the MCP/Relay tools, signed
identity profile and compatibility documentation. R3 still needs an isolated
Core-plus-Relay lifecycle, restart and WAL recovery, broader old-schema states,
Node regressions, uncertain response handling, and Windows Fork Actions
artifact evidence. OAuth refresh fixtures and actual ChatGPT conversations are
not exercised here. No production daemon, formal Batch, OAuth grant, Action
Runner, or remote service was queried or modified.

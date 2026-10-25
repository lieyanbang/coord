# Phase R3 — reliability, recovery and compatibility

Date: 2026-10-25 Australia/Brisbane. Result: **LOCAL ISOLATED R3 GATE PASS**.
This records local test evidence only. Fork Actions artifact verification,
real ChatGPT OAuth refresh/context-loss acceptance, and production deployment
remain separate gates; nothing here upgrades the deployed daemon or formal
Phase H Batch.

## Changes and isolated scenarios

- Added an actual local MCP HTTP → CoordMCP → Relay Sidecar → Relay Agent → Core
  trusted lifecycle test. It uses a synthetic owner token and an isolated
  database/loopback ports, and exercises three queued tasks, separate stateless
  MCP requests, token-free claim/progress/resume/complete/release/status,
  Core-plus-Relay restart with the same database and request journal, checkpoint
  recovery, idempotent completion replay, old-attempt rejection, bounded
  interruption backoff and reassignment. The test also checks that Relay
  journaling does not persist Core lease tokens.
- Expanded the pre-Phase-R SQLite fixture to cover active, pending, completed,
  submitted, failed, cancelled and unfenced claimed rows. It checks conservative
  failure-budget migration, monotonic attempts, checkpoint/progress receipts,
  completion receipt/action preservation, idempotent reopen and
  `PRAGMA integrity_check`.
- Added a SQLite online-backup test with the source Core store still open and
  non-empty WAL data. The restored snapshot retains the active attempt and
  checkpoint after the source database completes that attempt.
- Enabled rusqlite's existing `backup` feature solely to exercise its online
  backup API in the integration test. No application data or deployed runtime
  was changed.

## Verification evidence

| Command | Result | Evidence |
| --- | --- | --- |
| `scripts/verify.ps1 -Stage r3` | Exit 0; fmt, clippy, all Rust tests and locked Release build each exit 0 | [stage-r3-checks.json](stage-r3-checks.json), [stage-r3-gate.log](stage-r3-gate.log) |
| `cargo test --all-features --locked` via stage gate | Full suite passed; trusted scheduler integration tests 9/9 | [stage-r3-test.log](stage-r3-test.log) |
| Focused `trusted_scheduler` suite | 8/8 passed before the online-backup case was added | [stage-r3-trusted-scheduler-tests.log](stage-r3-trusted-scheduler-tests.log) |
| Online backup/restore case | 1/1 passed after the addition; final full suite also covers it | [stage-r3-wal-backup-test.log](stage-r3-wal-backup-test.log) |
| `npm test --prefix relay/coord-mcp` | 18 passed, 0 failed | [stage-r3-coord-mcp-tests.log](stage-r3-coord-mcp-tests.log) |
| `node --test relay/relay.test.mjs relay/live-coord.test.mjs` with local `target/release/coord.exe` | 22 passed, 0 failed, including the real local trusted lifecycle and legacy Core cases | [stage-r3-relay-tests.log](stage-r3-relay-tests.log) |
| `python -m unittest discover -s scripts/phase_g -p test_phase_g_tools.py -v` | 8 passed | [stage-r3-phase-g-tests.log](stage-r3-phase-g-tests.log) |

The six-worker Store concurrency test confirms six simultaneous trusted claims
produce six unique task IDs and one current claim per worker. The Rust suite
also covers release/complete atomic competition, twenty legacy cancel/submit
race iterations, stale-attempt fencing, expiry/release retry budget behavior,
request replay and legacy API compatibility. The lifecycle integration test
uses `target/release/coord.exe`; it force-stops only its own temporary Core
child after a durable checkpoint and starts a new process over the same
database, exercising WAL recovery. All listeners bind loopback and all test
data is temporary.

The first lifecycle-test run asserted that a task would be immediately
claimable after lease expiry. The implementation correctly returned `empty`
while bounded interruption backoff elapsed. The test now polls for the bounded
ready state; the final focused and full Relay runs pass. The initial and passing
outputs are retained in [stage-r3-phase-r-live.log](stage-r3-phase-r-live.log)
and [stage-r3-phase-r-live-pass.log](stage-r3-phase-r-live-pass.log).

## Remaining boundaries

- The MCP lifecycle uses a synthetic bearer token and does not exercise a real
  browser Access login, ChatGPT-issued OAuth token, refresh, or a real
  conversation/context compression. Those are **NOT_RUN**, not inferred from
  protocol unit tests.
- The local Python checks cover Phase G tool regressions; the full Rust gate
  covers repository Runner/action tests. This run did not invoke a real external
  publisher against a production path.
- Windows Fork Actions artifact matching/hash and execution are still required
  by the delivery contract. Until the pushed commit's Actions result and exact
  artifact are checked, Fork CI artifact acceptance is **PENDING**.
- No production endpoint, OAuth grant, formal Batch, public route, Action
  Runner operation, or remote service was queried or modified. No throughput or
  production performance claim is made.

# Stage E — final Windows regression, faults and capacity

Start: `0f4727f790916fc5cc42bd6be25bf64fbdc83157` (Stage D).
Environment: Windows x86_64 MSVC; Rust/Cargo 1.95.0. Single agent,
`ledger-otter`; no known shared daemon. Stage commits are the bootstrap ledger.
All test daemons use disposable databases and process handles owned by tests.
TASK.md remains unchanged. Linux/macOS: NOT_RUN per the user's platform override.

## Reproduced defects and minimal fixes

- Negative MCP requests emitted rmcp WARN diagnostics on stdout. The real stdio
  client failed JSON parsing (exit 101; `stage-e-mcp-stdout-before.log`). Route
  tracing to stderr. A deterministic rejected submission followed by a healthy
  claim now proves both responses remain machine-readable.
- Unknown stored task states made batch progress appear terminal. Regression
  failed (exit 101; `stage-e-progress-before.log`); explicitly reject corruption.
  Also force a late migration failure and verify earlier ALTERs roll back.
- `stage-e-faults-before.log` preserves the initial fault-run failure. These are
  historical red evidence, not current gate failures.

## Commands and observed acceptance

`cargo test --test validation --test faults --test quality --locked -- --nocapture`
exited 0 (`stage-e-faults.log`): four fault tests, six quality tests, four validator
tests. No core negative test is skipped.

- Eight independent MCP holder processes, seed `0xC00D`; four hard-killed.
  Restart daemon, expire/reclaim leases, replacement MCP completes all 20 tasks;
  four old results rejected, including same agent identity reuse.
- Drop HTTP response body after committed completion, restart daemon, repeat
  identical request: receipt returns success and exactly one ack exists.
- Twenty real cancellation vs MCP asynchronous submissions: observed 10 accepted
  before cancel and 10 refused; all 20 remain cancelled after the verdict window,
  with zero acks. The invariant is terminal consistency, not the racing split.
- Separate validation interruption simulation repeatedly expires a job fence;
  three errors exhaust the independent budget, execution count stays 1, stale
  jobs cannot commit, and no further validator job is available. Real daemon
  restart recovery, timeout ERROR and execution FAIL budgets also pass.

Capacity command (explicitly opt-in):

```powershell
$env:COORD_TEST_BIN=(Resolve-Path target/release/coord.exe).Path
cargo test --test capacity --locked -- --ignored --nocapture
```

Exit 0; real release daemon + six separate stdio MCP processes, JSON-RPC over
loopback to SQLite WAL. `stage-e-capacity.json` contains raw measured metrics;
`stage-e-capacity.log` contains test output. Times include bridge startup and
claim/submit calls, exclude enqueue; claim percentiles include empty claims.

| Tasks | Seconds | Tasks/s | Claim P50/P95 ms | Daemon peak WS bytes |
| --- | --- | --- | --- | --- |
| 100 | 0.1160 | 862.20 | 1.56 / 6.06 | 14,348,288 |
| 1,000 | 1.1406 | 876.71 | 2.13 / 19.91 | 19,005,440 |
| 10,000 | 46.5093 | 215.01 | 11.07 / 39.68 | 21,114,880 |

Every size has exactly N unique completed IDs, N durable completion receipts,
N terminal tasks and no active tasks. SQLite busy/locked **logged errors**: 0;
this does not measure internal busy-handler waits. Requested 200ms idle long-poll
measured 202.64/207.32/211.43ms. Sum of six worker peak working sets was
88,330,240/89,026,560/89,841,664 bytes; these are summed individual peaks,
not a simultaneous process-tree peak. Windows K32GetProcessMemoryInfo supplies
peak working set. No arbitrary performance target or other-machine guarantee.

## Real baseline migration and rollback

```powershell
python scripts/verify_migration.py --old-bin target/verification/coord-baseline.exe --new-bin target/release/coord.exe
```

Exit 0; report `stage-e-migration.json`. The preserved unchanged original binary
creates a real SQLite database with four task rows (including ack), one Agent,
six legacy Vault notes and an active legacy claim. A quiescent SQLite backup is
upgraded as a copy. IDs/name/payload/result/created_at/Agent identity/first_seen,
ack and the six note hashes survive. Legacy active claim becomes pending;
tokenless writes fail; new fenced claim survives repeated migration/restart.
Restoring the pre-upgrade backup to another copy lets the original binary read
exactly the original rows; immutable backup hash remains unchanged.

Operational rollback is backup restoration, not a reverse schema migration:
stop the owned daemon, snapshot SQLite with its backup API and back up Vault,
upgrade a copy first, then switch the owned service only after acceptance.
To roll back, stop it and restore the matched pre-upgrade DB/Vault and binary.
Do not copy only the main DB while WAL writes are live, mix old/new sidecar files,
or run the old binary against an upgraded DB. This test did not migrate any
operator/production database. Disposable evidence stays outside Git.

## Final gate

`pwsh -NoProfile -File scripts/verify.ps1 -Stage e` records exact commands,
UTC times, durations and exits in `stage-e-checks.json` plus four check logs.
All four checks exited 0. The ordinary suite reports 35 passing cases and
one intentionally ignored capacity case (run explicitly above).
The capacity test is the sole intentional ignored test in the ordinary suite
and was explicitly executed above. Final audit exits 0 with the same documented
rmcp transport exception and three retained warnings (`stage-e-audit.log`,
STAGE_A.md); warnings are not claimed fixed. `git diff --check` is required.

Gate E is PARTIAL while the current implementation's Windows Actions run and
downloaded artifact are pending. Final cloud SHA/run/artifact evidence and
the resulting gate decision are recorded in `FINAL_ACCEPTANCE.md`.

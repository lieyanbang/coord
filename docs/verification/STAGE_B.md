# Stage B — lease fencing

Starting commit: `f846279`. Windows, bootstrap Git ledger, no shared daemon.
**Gate B: PASS**; `scripts/verify.ps1 -Stage b`: fmt/clippy/test/release build
all exit 0, with commands/timing in `stage-b-checks.json`. No ignored tests.

The Stage A stale-completion test is now enabled and passes. Each claim writes
a random UUID lease_token and increments attempt_no/attempt_count. Claim and
readback share an immediate transaction. Renewal and completion require current
agent + token + live database-time lease; expired leases cannot revive and
shorter renewals cannot shorten the deadline. Reclaim snapshots/updates/readback
are one immediate transaction and invalidate tokens; exhausted attempts fail.

Completion request receipts and acks commit atomically. Exact duplicate requests
return the original ack after reconnect/restart; conflicting JSON/ack requests
fail. Cancellation only affects active tasks, so completed/failed outcomes stay
terminal. Old active tokenless claims are invalidated on migration. HTTP/MCP/CLI
require credentials; old clients receive invalid-params/required-argument errors.

Evidence includes:

- `tests/lease.rs`: strict old-agent completion assertion (previously proven red).
- `tests/fencing.rs`: expiry without sweep, renewal refusal, same-ID reclaim with
  new token/attempt, monotonic renewal, store reopen, exact duplicates and ack
  uniqueness; 30 cancel/complete races over independent SQLite connections.
- `tests/fencing_protocol.rs`: real daemon and independent CLI processes,
  tokenless HTTP/CLI rejection, expiry, forced daemon restart, re-claim by the
  same ID, stale token refusal, exact receipt replay.
- Original 16-process contention and two-tab atomic ack/watch tests still pass;
  all daemon tests use mandatory Cargo binary paths and execute on Windows.

Failure reporting/retry scheduling did not exist in the original surface; it
will be added with the same fenced predicate and receipts in Stage C. No unsafe
failure write endpoint remains available. API compatibility/backup/rollback
instructions are in `docs/API.md`. Optional POSIX demo examples now carry the
claim token but are NOT_RUN on the default Windows platform.

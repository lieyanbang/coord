# Stage D — optional validation and reference Worker

Starting commit: `a83d504`. Windows; no shared coordinator ledger; TASK.md stays
read-only. New validator module is necessary to keep the pluggable Validator
trait independent from MCP/HTTP; existing Store and protocol paths are reused.

Validation is opt-in per batch. Built-in JSON key/equality rules return
PASS/FAIL/ERROR. Submission persists a receipt and SUBMITTED result immediately;
four background slots use separate expiring job fences. PASS and requested ack
commit together. FAIL uses execution retry budget; ERROR and interrupted jobs
use their own finite validation budget. Unfinished durable jobs recover after
their old timeout+5s lease. Cancelled/newer submissions reject stale verdicts.
The shared ack writer preserves direct completion semantics and old receipts.
No external validator command execution or LLM dependency is introduced.

Real daemon + MCP acceptance in `tests/validation.rs`:

- Submit returns before a 400ms validator completes, next task is immediately
  claimable, no premature ack; PASS/FAIL/retry reaches correct terminal counts.
- Timeout ERROR exhausts validation budget with execution attempt_count still 1.
- Lease-interrupted job resumes after forced daemon restart; cancellation while
  validating cannot be overwritten. Independent job-token expiry/re-claim rejects
  the old verdict. FAIL exhausts execution attempts and becomes failed.
- Direct mode remains covered by the original and batch protocol suites.

`scripts/worker.py` is an optional Python 3.10+ standard-library runner with a
replaceable trusted JSON adapter, explicit workdir, bounded output/timeout,
heartbeat/renewal, reconnect/empty backoff and exact submission retries. Windows
Job Objects contain adapter trees on worker crash. The Worker is an example
runtime, not a promise of unattended support from every IDE MCP client.

`tests/worker.rs` runs actual Python worker/adapter processes: identity and
external JSON results, execution beyond the lease with renewal, timeout and
1MiB output rejection with error-message assertions, forced worker kill,
adapter PID disappearance via Windows Job Object, then lease recovery and a
replacement worker completing the task. Fixture adapters run only in temporary
directories. Python 3.12 setup is explicit in Windows Actions.

First focused E2E: `cargo test --test validation --test worker --locked --
--nocapture`, exit 0 (`stage-d-e2e.log`). Expanded cases are included in the final
full gate: `pwsh -File scripts/verify.ps1 -Stage d`; see `stage-d-checks.json`.
Gate D is PASS only after every required local check exits 0.

**Gate D: PASS.** All four full checks exited 0; four validation tests plus the
expanded real Worker acceptance passed, including adapter process disappearance
and a replacement worker completing the recovered task.

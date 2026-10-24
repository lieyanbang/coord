# Phase G6-B — Independent Action Runner and native publication

Result: **PARTIAL**. Local Windows source/protocol/native tests pass. The
dedicated-account acceptance script is ready for disposable Windows Actions;
actual production service identity, effective ACLs and RPC denial are **NOT_RUN**.
Production completion execution remains disabled. No production account, ACL,
service, database, Canary or formal classification task was changed.

## Implemented boundary

The trusted registry can select `external_runner`. The daemon then owns SQLite
and the existing transactional outbox but only serves a separate loopback
queue broker (`/claim`, `/authorize`, `/finish`); it never creates an action
child. A distinct `coord-action-runner.exe` validates its actual Windows SID,
non-administrator/elevation state and absence of dangerous assigned privileges.
It rejects SYSTEM, shared LocalService/NetworkService, a filtered administrator
token and the daemon SID. The administrator must configure a dedicated standard
local/domain service account; no production account was provisioned here.

The broker key is a dedicated random 256-bit secret in an ACL-protected file.
External mode additionally requires a different `daemon_rpc_key_file`: management
RPC POSTs reject missing credentials and the broker key with HTTP 401. The
runner must be unable to read that management key. CLI/MCP supports private
`COORD_RPC_AUTH_KEY` for loopback HTTP; the relay's optional key-file support is
documented with its own G6-A changes. Legacy deployments without external mode
retain their existing trusted-localhost behavior; NTFS denial alone is not
claimed to isolate an unauthenticated RPC listener.

Each command is fixed, hash-pinned and rechecked before execution. Native children
start suspended, enter the bounded kill-on-close Job Object, then resume their
unique primary thread. This closes the old spawn-before-assignment window.
The 360-second action fence, 300-second command limit, output/memory/process
limits, independent task/action states, idempotent bounded retry and
non-idempotent `uncertain` recovery are retained. A registry with neither runner
mode explicitly logs queue-only operation. `isolated_in_process: true` is used
only by disposable legacy regression fixtures.

Before spawn the broker reconstructs the stored event, requires at least 305
seconds of current action lease and verifies the current completed task attempt,
owner and exact accepted-result receipt. It supports both direct completion and
validator submission receipts. It forwards only group/ID input context, recursively
redacts known secret fields, and never forwards arbitrary task payload or lease
tokens. Agent-supplied action/profile/receipt fields cannot replace that event.

The real `coord-emote-action.exe` implements G4's strict required fields, evidence,
refs, sizes, coverage and worker/attempt checks, with the schema's additional
candidate icon-path restriction. It reads accepted inline database results rather
than Agent-chosen scratch filenames. Every path ancestor is checked for reparse
points. `results/<group>.json` and `.receipts/<group>.json` are individually
published through flushed temporary files and atomic no-replace NTFS links.
Byte-identical retries succeed; conflicts fail without replacement. An interrupted
result/receipt pair can only be repaired with matching bytes. Batch indexing waits
for successful task actions, rechecks their receipt/digest/attempt, includes group
120 and emits preliminary index/review candidates without global IDs or merges.

## Local evidence

Command `cargo check --locked --all-targets` exited **0** during implementation.
The final focused regression command and timestamp are recorded in
`stage-g6-b-focused.json`; raw output is `stage-g6-b-focused.log`:

| Command | Exit | Observed result |
|---|---:|---|
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | 0 | All targets, including both native binaries |
| `cargo test --locked --test action_runner --test agent_actions` | 0 | 10 new runner/native tests plus 11 action tests pass; 7 ignored child helpers are explicitly invoked by their parent tests |
| PowerShell AST parse of `scripts/windows/test-action-runner.ps1` | 0 | No parse errors; does not execute account operations |

The new tests exercise native task publication/indexing, identical replay,
conflict preservation, group 120 and candidate-only aggregation, simultaneous
atomic publication, registry/execution hash mismatch, malformed fields/path
traversal, real ancestor junction rejection, missing receipt, wrong owner/fence,
near-expiry rejection, arbitrary-payload/secret-sentinel exclusion, privileged
runner rejection, authenticated broker-only routes and main-RPC 401 rejection.
The first-effect child checks kernel Job Object membership before writing.
Existing tests retain daemon interruption after a side effect, timeout/output
limits, duplicate events, idempotent fencing/recovery and non-idempotent
uncertainty while completed tasks stay completed.

These protocol/native tests run under the local operator's current process
identity. The test that invokes the real runner observes its identity rejection;
it does not convert current-user fixture execution into low-privilege evidence.
The fixture's first junction creation attempt failed on mixed slash syntax;
using Windows path separators fixed the fixture and the actual junction rejection
then passed. No production path was involved.

## Disposable low-privilege gate

Run after building all release binaries on an administrative disposable Windows
GitHub Actions worker:

```powershell
pwsh -NoProfile -File scripts/windows/test-action-runner.ps1 `
  -BinaryDirectory target/release `
  -EvidenceDirectory docs/verification/stage-g6-low-privilege
```

The script refuses non-Actions hosts, creates a random standard local user and
an exact random temporary root, and uses `Start-Process -Credential` with hidden
windows. It checks the actual token, required task/refs reads, protected
DB/management-key/cross-Agent reads denied, executable/config/DB/system-directory
writes denied, approved scratch/log/results writes allowed, and main RPC 401 for
both no key and the broker key. It exercises actual low-privilege native publish
and index on a one-task isolated batch. It then kills only its own runner while
a fixed hash-pinned native loopback ping child is active, checks Job Object child
termination, forces lease expiry only in the disposable SQLite fixture and
verifies restart yields `uncertain`, attempt 1 and completed task state.

It preserves redacted identity/ACL/process/action evidence in the given output
directory, removes its exact random account by verified SID and cleans only its
verified temporary root. This test does not establish six production Agent
workspace principals or the real Servy service-logon policy. At this report's
local-source checkpoint the cloud run is **NOT_RUN**; a pending run must not be
reported as PASS. The final G6-D evidence records any later actual run.

## Production release gate and recovery

The existing Windows health monitor now accepts optional `-RpcAuthKeyFile` and
validates its protected absolute regular file, ancestor reparse points, size,
owner/allow entries and 64-hex contents before probing. Invalid configuration
exits 2 without Servy recovery. It temporarily sets the CLI's process environment
and restores the previous value in `finally` before any recovery command; legacy
invocations remain unchanged. `stage-g6-health-monitor-focused.log` records the
Windows PowerShell 5.1 regression: legacy recovery threshold, invalid-key
configuration rejection, real authenticated daemon/CLI `READY` with an incorrect
inherited key overridden, and restored environment observed by fake Servy.
Actual production SYSTEM scheduled-task execution remains **NOT_RUN**. The
runbook requires configuring a separate monitor-only protected key copy before
enabling RPC authentication, since the daemon's service-readable file may also
grant LocalService and is intentionally rejected by the monitor's stricter ACL.

`docs/ACTION_RUNNER.md` contains the precise account/ACL/key layout, runner
command, output directories, bounded restart and reconciliation procedure.
The example registry references the real build target and intentionally rejects
its placeholder SIDs/hashes until an administrator fills verified values.

Production requires actual artifact hashes, a dedicated independent non-admin
service identity, read/write/ACL-change denials including management-key access,
main-RPC denial, trusted-directory ownership and per-Agent attempt ACL checks.
The broker's bearer protocol depends on those key-file ACLs; it is not kernel
attestation of an HTTP caller. Hash checking depends on trusted non-writable
binary parents to prevent replacement races. These real-host requirements and
G6-A are not satisfied by the local fixtures, so deployment and production Canary
remain **NOT_RUN**.

If a runner or daemon is interrupted after an external effect, inspect exact
outbox state and receipt/output hashes first. Never auto-replay an uncertain
non-idempotent action or reopen its completed task. Rollback stops only the owned
runner, retains DB/outbox/receipts/results, and restores backed-up configurations
and verified prior executables. No production rollback was needed or performed.

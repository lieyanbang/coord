# Phase R5-1B architecture, recovery, and acceptance evidence

This append-only report tracks the authorized B0–B5 work. It preserves the
earlier R5 execution-boundary **FAIL** records, R5-1H **PASS**, R5-1K
**BLOCKED/INCONCLUSIVE** outcomes, and all prior probe deviations. The
historical reports remain unchanged.

## Gate status at 2026-10-26 12:40 UTC

| Gate | Status | Current evidence |
| --- | --- | --- |
| B0 design and live baseline | **PASS** | [`PHASE_R5_BROKER_DESIGN.md`](PHASE_R5_BROKER_DESIGN.md), B0 Windows snapshot and current rechecks |
| B1/B2 implementation and Windows acceptance | **PARTIAL** | Implementation is committed; local Rust/health and PowerShell 5.1 checks pass. Fork run `40321647040` failed only in the hosted low-privilege ACL fixture; the fix is applied locally and awaits a new Fork run. |
| B3 verified artifacts, backup, migration and production recovery | **NOT_RUN** | Production remains unavailable. Current protected backup is incomplete; a fresh protected backup and copy-only rehearsal are required. |
| B4 original R5 Web acceptance | **NOT_RUN** | No new R5 Canary or ChatGPT prompt exists. |
| B5 final evidence and acceptance | **IN_PROGRESS** | This report will be extended as each gate is reached. |

## Implementation and local verification

- B0 design commit: `c15368b`.
- B1/B2 implementation commit: `487315a3fc8b71d6703ae490081aa25e3f09ee8f`.
- The implementation separates Scheduler/Core readiness from the optional
  Action Broker, reports component health, verifies Runner Ed25519 signatures
  with Core-side public keys, persists nonce replay state, and keeps management
  RPC authentication separate from the legacy Broker key. Existing Action
  fencing and uncertain-result code paths were left in place; the local Rust
  regression suite passed. That suite includes signed-request tamper/expiry/
  replay-after-restart, key rotation, Broker bind/configuration failure,
  Broker-disabled Scheduler compatibility, additive nonce migration, and
  Action outbox/uncertain-recovery cases. The hosted relay and low-privilege
  suites did not finish in run `40321647040` because the ACL fixture step
  stopped the job.
- `scripts/verify.ps1 -Stage r5-1b`: fmt exit 0, clippy exit 0, all Rust tests
  exit 0, Release build exit 0, and health-monitor isolation exit 0. The
  resulting command/exit records are in `stage-r5-1b-checks.json` and the
  corresponding `stage-r5-1b-*.log` files.
- Windows PowerShell 5.1 parser check for
  `scripts/windows/test-action-runner.ps1`: **PASS**.
- The hosted Windows rehearsal uses a real LocalService token for Core and a
  separate dedicated local Windows account for the Runner. It verifies public
  key read/private key denial, ACL policy, signed positive actions, and an
  isolated non-idempotent Runner crash. Production SCM/Servy behavior still
  requires the later live recovery gates.
- `git diff --check`: exit 0.

## Fork Windows Actions failure and repair

Fork Actions run [40321647040](https://github.com/lieyanbang/coord/actions/runs/40321647040)
tested commit `487315a3fc8b71d6703ae490081aa25e3f09ee8f`. Fmt, clippy, Rust
tests, Release build, and the health-monitor authentication/recovery step all
passed. The dedicated low-privilege Runner rehearsal failed while applying a
fixture ACL. Windows PowerShell 5.1 passed two ACE strings as one native
`icacls` argument; the hosted runner reported `Invalid parameter` followed by
`Fixture ACL application failed`. Packaging was skipped, so this run has no
deployable artifact and its binaries must not be used.

The fix in `scripts/windows/test-action-runner.ps1` disables inheritance first,
then invokes `icacls /grant:r` once per ACE. Each invocation carries a single
SID/permission argument while retaining the SYSTEM and Administrators grants.
The hosted cleanup path ran; the failed artifact was diagnostic only. The new
Windows run must pass the complete low-privilege and subsequent relay/MCP
steps before its exact commit and packaged hashes can be accepted.

A local one-off `icacls` fixture command was rejected by the local execution
policy before process creation; no test directory or ACL was created or
changed. The Windows Fork rehearsal is the required end-to-end check for this
native ACL path.

## Current production and protected-data recheck

The direct SSH snapshot at
[`PHASE_R5_B1_LIVE_RECHECK_20261026T123931Z.json`](PHASE_R5_B1_LIVE_RECHECK_20261026T123931Z.json)
was collected read-only at 12:39:36Z. The Coord service and
`CoordActionRunnerG7` were Stopped, `Servy` (PID 3224) and
`CoordRelayAgent` (PID 5892) were Running, no `coord.exe` process existed, and
ports 7777/7779 had no listener. The real SYSTEM monitor's last result was 1;
health was FAIL because authenticated `tasks/list` was unavailable. SQLite
remained WAL mode, `integrity_check=ok`, `schema_version=32`, `user_version=0`,
with neither `task_attempts` nor `runner_nonces` present.

The formal Phase H Batch remains 121 tasks (105 completed, 16 pending), 105
completion actions (83 succeeded, 38 failed), 106 progress rows, and 145
progress receipts. Its B0 protected fingerprint and those of both old
Canaries match the 11:36Z and 12:16Z baselines. The 10-task old operator
Canary remains completed; the 4-task old browser Canary remains completed.
No Batch, task, receipt, Action, binding, or credential ACL was changed.
The exact sanitized counts and fingerprints are in the JSON audit.

At 12:36:26Z the backup-prerequisite read briefly observed the Coord Servy
wrapper Running as LocalService (PID 7976), while the fresh health task still
returned 1. The later 12:39:36Z audit again observed the wrapper Stopped, no
Core child, and no listener. A wrapper's transient Running state is not Core
readiness and does not pass the health gate.

The Linux relay hop was independently queried by direct OpenSSH at 12:35:24Z;
[`PHASE_R5_B1_LINUX_LIVE_RECHECK_20261026T123524Z.json`](PHASE_R5_B1_LINUX_LIVE_RECHECK_20261026T123524Z.json)
records active Coord Relay, Feathers, and OpenResty services with zero
restarts, Sidecar `/healthz=READY`, and the expected Tailscale/loopback
listeners. No Linux service, route, OAuth record, or configuration changed.

## Backup preconditions and next safe transition

The 12:36:26Z read-only backup prerequisite audit is
[`PHASE_R5_B1_BACKUP_PREREQUISITES_20261026T123622Z.json`](PHASE_R5_B1_BACKUP_PREREQUISITES_20261026T123622Z.json).
The existing backup parent inherits an operator ReadAndExecute ACE; it must
not be copied into a new backup root. The previous backups do not include all
required Runner binaries, current production registry, and ACL manifest.
Create a new run-specific directory with inheritance disabled and explicit
SYSTEM/Administrators FullControl before copying any service configuration or
other protected data.

The source-matched Servy snapshot checker was absent from Arasaka's backup
area. It was rebuilt locally from the repository and its `--self-test` passed
(SQLite Online Backup API, read-only integrity checks, source-compatible
collation, overwrite protection, and tamper detection). It has not yet been
copied to the remote backup. The next production transition is a fresh
protected backup, followed by a new-copy compatibility-Core rehearsal with
Broker **DEGRADED/UNAVAILABLE**. Do not restore the old G7 Core or grant it
access to the old Broker key.

## Fork Actions rerun and backup audit update — 2026-10-26

The protected G7 backup created under `C:\ProgramData\coord\backup\g7-20261026T124419Z-0380fb72` is now verified complete: online SQLite backup and Servy snapshot passed; the 48-object tree has only SYSTEM/Administrators FullControl at its root, no unexpected ACEs, and unchanged parent and legacy Broker-key ACLs. The DB audit found matching row counts, table/schema-object fingerprints, and `integrity_check=ok` between live and backup. The SQLite internal `schema_version` cookie is 32 in the live DB and 1 in the online backup, while both `user_version` values are 0. This metadata discrepancy is recorded for migration rehearsal; it is not yet treated as an application-schema difference.

Fork run [40324684634](https://github.com/lieyanbang/coord/actions/runs/40324684634) tested `949d4b36446a463a457e30be8ab4c9da570a4927`. Fmt, clippy, all Rust tests, Release build, and isolated health-monitor recovery/fault-domain tests passed. The dedicated Windows low-privilege Runner rehearsal still failed at fixture ACL setup (step exit 1); the log showed `icacls` received the Runner Modify ACE and LocalService Modify ACE as one space-joined argument. The workflow skipped Relay/MCP tests and package creation. Its 465-byte diagnostic artifact is not a deployable build and must not be used. The earlier claim that the ACL fix was sufficient is superseded by this observed hosted failure.

The repair now makes each grant a scalar `icacls /grant:r` invocation through `Add-FixtureGrant`, and each fixture directory is configured one grant at a time. The PowerShell parser check passes. A local `scripts/verify.ps1 -Stage r5-1b` rerun is in progress; hosted Windows execution remains the required validation of ACL semantics.

Recorded execution deviations remain: the one-off local ACL command was rejected by local execution policy before process creation; an earlier PowerShell command failed before SCP parsing completed; a separate incorrectly quoted remote PowerShell 7 invocation failed without producing a backup; and several audit wrapper commands failed before read-only substitute queries completed. None of these changed production services, the live DB, or the legacy Broker-key ACL. No real Batch, old Canary, or failed business Action was executed or mutated.

## B0-B2 continuation checkpoint — 2026-10-26 14:00 UTC

- The updated local gate `scripts/verify.ps1 -Stage r5-1b` completed with exit
  0 for fmt, clippy, all-feature Rust tests, release build, and the isolated
  SYSTEM health-monitor harness. The exact command records and sanitized logs
  are in `stage-r5-1b-checks.json` and `stage-r5-1b-*.log`.
- Windows PowerShell 5.1 parser checks passed for all eight changed deployment,
  backup, verifier, and rehearsal scripts. `git diff --check` passed; Git only
  reported the repository's LF-to-CRLF working-copy notices.
- Fork run [40324684634](https://github.com/lieyanbang/coord/actions/runs/40324684634)
  failed its dedicated low-privilege ACL fixture before Relay/MCP or packaging.
  The produced 465-byte diagnostic artifact is not deployable. The revised
  fixture applies one ACE per `icacls /grant:r` invocation; local parsing and
  Rust/monitor checks pass, but this repair still requires a new hosted Windows
  Actions run. No artifact from either failed run is accepted.
- A direct OpenSSH read-only recheck at 14:00:32Z is preserved in
  [`PHASE_R5_B1_LIVE_RECHECK_20261026T140032Z.json`](PHASE_R5_B1_LIVE_RECHECK_20261026T140032Z.json).
  Coord and `CoordActionRunnerG7` were stopped; `Servy` (LocalSystem PID 3224)
  and `CoordRelayAgent` (LocalService PID 5892) were running; neither 7777 nor
  7779 had a listener. The enabled SYSTEM task was Ready and its latest run at
  14:00Z returned `LastTaskResult=1`. The status file reported `FAIL` with 825
  consecutive failures; its older schema did not expose component fields.
  The legacy `broker.key` was neither opened nor hashed.
- The configured Coord MCP `tasks_list` call for pending urgent tasks returned
  `INVALID_ARGUMENT / logical_worker_required`; the exposed connector has no
  heartbeat or task-creation operation. No high-priority query, task claim,
  task ID, or ack was fabricated. No production or formal Batch operation was
  attempted. The local `coord` command is not installed, so stage commits remain
  the available repository ledger.

### Current gate disposition

| Gate | Status at 14:00 UTC | Evidence / next transition |
| --- | --- | --- |
| B0 | **PASS, outage persists** | Architecture and fresh sanitized Windows snapshot are recorded; recheck again immediately before deployment. |
| B1/B2 | **PARTIAL** | Local regression and monitor tests pass; Windows ACL rehearsal failed in run `40324684634`; the one-ACE fix awaits a passing Fork run. |
| B3 | **NOT_RUN** | No accepted build artifact. Do not deploy or use either failed-run artifact. After a green run, verify exact commit, ZIP, three EXEs, and health script; then perform the protected-copy rehearsal before production recovery. |
| B4 | **NOT_RUN** | No new Canary Batch or prompts exist. |
| B5 | **IN_PROGRESS** | Prior FAIL/BLOCKED/deviation records remain preserved; continue appending evidence. |

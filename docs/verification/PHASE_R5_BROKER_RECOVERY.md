# R5-1K Broker-Key Path Recovery — 2026-10-26

This append-only record covers the authorized R5-1K K0/K1 investigation. It
preserves the earlier failures and execution-boundary deviations in
[`PHASE_R5.md`](PHASE_R5.md) and [`PHASE_R5_HEALTH.md`](PHASE_R5_HEALTH.md).
`TASK.md` remained read-only. No production service, credential ACL, database,
Batch, receipt, action, Runner policy, or Core binary was changed in this run.

## Gate results

| Gate | Result | Evidence |
| --- | --- | --- |
| K0 live service and health audit | **COMPLETE — production FAIL** | At 10:06 UTC, `coord` (LocalService) and `CoordActionRunnerG7` were Stopped; `Servy` (LocalSystem, PID 3224) and `CoordRelayAgent` (LocalService, PID 5892) were Running. A separate post-probe service query confirmed those states and no listener on 7777 or 7779. The latest SYSTEM task query reported `LastTaskResult=1` at 10:12:12Z (next run 10:13:13Z). |
| K0 Broker path/ACL diagnosis | **PASS** | The metadata-only LocalService probe against `C:\ProgramData\coord\g7\config\broker.key` previously returned `LastTaskResult=1055`, mask `31`: metadata passed for the five ancestors through `g7\config`, failed on the Broker-key leaf, no reparse point was observed in the checked path, the content-read open was denied, and the task identity was LocalService. The production key DACL still has no LocalService ACE. Its observed SDDL is `O:BAG:S-1-5-21-3592388187-2353680085-815129118-513D:AI(A;ID;FA;;;BA)(A;ID;FA;;;SY)(A;ID;0x1200a9;;;S-1-5-21-3592388187-2353680085-815129118-1001)`. |
| K0 backup and protected-state review | **PASS** | The read-only post-rollback DB snapshot remains SQLite `integrity_check=ok` and matches the protected predeploy snapshot. Sanitized comparisons of the formal Phase H Batch and both old G7 Canaries matched their snapshot rows for batches, tasks, receipts, progress, and completion actions. Their recorded counts/fingerprints remain as documented in the preceding `PHASE_R5.md` entries. Credential ACLs were inspected without modification. |
| K1 source compatibility | **STOP GATE** | `safe_existing_path()` checks each path component with `symlink_metadata()` and canonicalizes it (`src/core/windows_security.rs`). However, `RunnerConfig::validate()` calls `key()`, which calls that path check, checks file metadata, then uses `read_to_string()`; `broker_router()` calls `key()` again to retain the credential for Broker request authentication (`src/core/action_runner.rs`). Therefore a metadata-only ACE or path-check-only change cannot make the current Core operate without reading Broker-key content. |
| K1 isolated minimum-ACE probe | **INCONCLUSIVE** | An empty synthetic file was given a protected DACL whose LocalService ACE was exactly `0x100080` (`ReadAttributes | Synchronize`); no production file was used. The LocalService Scheduled Task returned `LastTaskResult=1` and produced no probe result, so effective metadata success/content-read denial was not claimed. All temporary `R5K1AclProbe-*` tasks and `r5-1k-acl-*` directories were checked absent after cleanup. The production Broker-key SDDL was re-read and unchanged. |
| K2 live recovery acceptance | **NOT_RUN — BLOCKED by K1** | No production ACL repair, Core change, service restart, or deployment was performed. K2 cannot pass while the current Core requires key contents and the approved K1 boundary prioritizes keeping those contents unreadable by Core. |
| New R5-1 deployment continuation through R5-4 | **NOT_RUN** | The stop gate prevents deployment, new synthetic Batch creation, ChatGPT prompts/actions, and reconciliation. No new R5 Batch/task/receipt/action UUID exists. |

## K1 stop rationale

The production denial occurs on the Broker-key leaf: the current DACL grants
Administrators and SYSTEM full control and the existing dedicated Runner
account read/execute, but grants LocalService no access. The exact metadata ACE
would address the observed `symlink_metadata()` denial, but the unchanged Core
then must open and read the same file to initialize and authenticate the
Broker. Granting that content access to Core would cross the preferred
credential boundary. Replacing the current Broker trust flow with one that
keeps key material in a separate process is an architectural change; it is not
equivalent to a path-check fix and would require a separately reviewed design,
CI build, and artifact/hash acceptance.

Accordingly, K1 is a clear stop gate. No production ACL or code change is
authorized by implication. K2 and all later gates remain unstarted pending a
newly authorized compatibility design that preserves the Broker-key content
boundary, or explicit authorization to change that boundary.

## Data, credential, and execution-boundary evidence

- The only ACLs created during the isolated test were on a disposable child
  directory and empty synthetic files under `g7\config`; those objects and
  test tasks were removed. The production Broker-key SDDL is unchanged.
- K0 compared the live DB with the protected post-rollback snapshot using
  sanitized, noncredential columns. The formal Batch remains 121 tasks (105
  completed, 16 pending); the old operator Canary remains 10/10 completed;
  the old browser Canary remains 4/4 completed. Safe row comparisons for
  batches, tasks, progress, receipts, and completion actions matched. No
  production DB operation was issued in K1.
- No formal Phase H task, old Canary task, receipt, action, binding, or
  credential ACL was modified. No broker key contents or hash were read or
  emitted in this run.
- A first probe attempt tried to write its result into the LocalService-
  unwritable `g7\runtime\logs` path; it failed, its temporary task was removed,
  and no ACL was changed there. A later synthetic Scheduled Task registration
  failed with an invalid XML character; cleanup removed its disposable root.
  The file-based retry created and then removed its task/root, but returned
  `LastTaskResult=1` without a result artifact; this remains an inconclusive
  test-harness failure, not a compatibility PASS.
- During an earlier sanitized Servy-parameter check, encrypted
  `SERVY_ENC:v2` wrapper text was briefly displayed in tool output. It was not
  decrypted, the decryption key was not read, and the wrapper was not saved or
  copied into this report. The earlier Broker-key digest disclosure remains
  recorded once in the historical report and is not repeated here.
- One combined post-test live status command did not return within its bounded
  local wait and was interrupted; it had no remote write action. Later
  separate read-only queries confirmed the same four service states, no 7777
  or 7779 listener, and SYSTEM `LastTaskResult=1` at 10:12:12Z. The Broker-key
  SDDL and absence of probe tasks and temporary directories were also
  rechecked.

## Disposition

K0 is complete and confirms the production outage. K1 identifies the precise
path-metadata denial and the current Core's mandatory content-read requirement;
the isolated ACL effectiveness probe itself is inconclusive. The authorized
no-content-read path is therefore not demonstrated and the Core cannot be
recovered under that constraint with the current implementation. K2 is
**BLOCKED**. Do not proceed to R5-1 deployment, create a new Canary Batch, or
generate/send ChatGPT prompts until a safe compatibility design is authorized
and K2 passes.

## R5-1B recovery design supersession — 2026-10-26 14:00 UTC

The later user authorization in `TASK.md` supersedes the earlier instruction to
wait for a compatibility design; it does not erase the K0/K1 evidence or permit
access to the legacy Broker key. The replacement Core/Runner architecture and
the staged recovery plan are tracked in
[`PHASE_R5_BROKER_ARCHITECTURE.md`](PHASE_R5_BROKER_ARCHITECTURE.md).

At 14:00:32Z a new read-only OpenSSH observation again found Coord and
`CoordActionRunnerG7` Stopped, no listeners on 7777/7779, and SYSTEM
`LastTaskResult=1` with health `FAIL`. No B3 production deployment has started.
Fork run `40324684634` failed its hosted ACL fixture and produced no deployable
artifact; the follow-up fix requires a new green Windows run. Until then, do not
restore the old Core, modify production ACLs, or start the Runner. The database,
formal Phase H Batch, old Canary records, and failed business Actions remain
untouched. No new Canary Batch or ChatGPT prompt exists.

### B3 entry remains gated — 2026-10-26 14:11 UTC

The next Fork run `40336379565` passed Rust and SYSTEM-monitor checks but failed
before any hosted fixture ACL was applied: the local-path guard rejected the
exact synthetic root passed to its own setup function. Relay/MCP and packaging
did not run, and the uploaded 475-byte diagnostic ZIP is not deployable. The
path guard now accepts the exact test root and descendants only; a new hosted
Windows pass is still required before the protected-copy rehearsal or any
production change. The production outage, legacy key ACL, database, formal
Batch, old Canary, and existing Actions remain unchanged.

### B0 live baseline refresh and latest hosted failure — 2026-10-26 14:45 UTC

The direct read-only Windows/Linux and SQLite snapshot is
[`PHASE_R5_B1_LIVE_RECHECK_20261026T144506Z.json`](PHASE_R5_B1_LIVE_RECHECK_20261026T144506Z.json).
Coord and the legacy Runner are stopped, neither production port listens, and
the actual SYSTEM task returns 1 with health `FAIL`. The exact formal Batch
has 121 tasks (105 completed, 16 pending); both old Canary fingerprints match
the prior 12:49Z baseline. The old Broker-key ACL remains unchanged and the
file was not opened or hashed. Linux Relay/Feathers/OpenResty and Sidecar are
active/READY. One separate older Batch with one completed task was observed;
it remains untouched.

Fork run [40338070149](https://github.com/lieyanbang/coord/actions/runs/40338070149)
passed Rust and health-monitor gates but failed the hosted LocalService
Core/Broker readiness fixture. Its artifact has no deployment package. The
latest local update now records sanitized Core output, `/healthz`, task exit,
processes, and listener evidence; that revision still requires a new hosted
Windows run. No production deployment, service start, ACL change, or database
write has occurred. B1/B2 remain **PARTIAL**; B3 remains **NOT_RUN**.

The follow-up local command `pwsh.exe -NoProfile -File
scripts/verify.ps1 -Stage r5-1b` exited 0 at 14:49–14:50Z for fmt, locked
clippy/tests/build, and the SYSTEM monitor harness. The Windows PowerShell 5.1
parser and protected-fingerprint comparison also passed. These checks did not
exercise the hosted LocalService fixture. Production recovery remains gated on
a green Fork Windows run and a copy-only rehearsal.

### B0 recheck and hosted startup evidence — 2026-10-26 15:04–15:19 UTC

The direct SSH snapshot at
[`PHASE_R5_B1_LIVE_RECHECK_20261026T151613Z.json`](PHASE_R5_B1_LIVE_RECHECK_20261026T151613Z.json)
confirms the outage persists: `coord` and `CoordActionRunnerG7` are Stopped,
7777/7779 are closed, and the real SYSTEM task returned 1 with health `FAIL`.
SQLite integrity is `ok`; the formal Batch and both old Canary fingerprints
match the prior audit. The legacy Broker-key content and hash were not read,
and its ACL still excludes LocalService and the Runner SID. Linux Relay,
Feathers, and OpenResty remain active with zero restarts; Sidecar is
`READY/connected`, pending 0. No service, database, task, receipt, action, or
ACL was changed.

Fork run [40344079817](https://github.com/lieyanbang/coord/actions/runs/40344079817)
passed source and SYSTEM-monitor checks but failed the hosted LocalService
Core/Broker readiness fixture. Artifact `12660254058` is evidence-only and
has SHA-256
`877decc42c94c2e5f7ed8e860bc91d47c9bd7085c9a52c6aee12d2f8973d46b5`; it
cannot be deployed. The fixture proved LocalService SID and key isolation,
but recorded task result 1, no Core process/listener, no health response, and
empty Core output. Root cause remains undetermined. The fixture now captures
an explicit version probe, serve exit report, and bounded Windows application
error evidence. That diagnostic update passed local PowerShell 5.1 parsing and
the complete `r5-1b` local verification stage; the hosted proof is pending.

**Recovery remains gated:** do not start the old shared-key Core, reset Servy
recovery counters, install an unverified binary, start the Runner, or create a
Canary. B3 is **NOT_RUN** until a green Fork artifact and protected-copy
compatibility rehearsal are complete. B1/B2 remain **PARTIAL**.

# Phase R5-1H — SYSTEM health monitor repair

Observed 2026-10-25 22:55–23:19 UTC (2026-10-26, Australia/Brisbane).
Result: **SYSTEM health gate PASS**. This follow-up is separate from, and does
not revise, the earlier R5 execution-boundary **FAIL** or R5-1 **BLOCKED**
entries in `PHASE_R5.md`.

## Root cause

The configured `coord-health-monitor` task runs as `SYSTEM`. Its deployed script
hash matched the repository script (`FC3637CA6C537D3C237A9BF243F848EA8549D90985CC1AC18AC152A449CC5B37`).
The script's ACL policy at lines 39–44 exits with code 2 when an Allow ACE's SID
is outside SYSTEM, Administrators, or the current account. The monitor key
`C:\ProgramData\coord\g7\monitor-private\management.key` had an inherited
Allow ReadAndExecute + Synchronize ACE for `ARASAKA\lieyanbang` (SID ending
`-1001`). As SYSTEM, that ACE is outside the accepted set, so this exact policy
branch explains `LastTaskResult=2`; it was not an RPC, SQLite, key-format, or
task-launch failure.

The owner was Administrators; the key was 64 bytes and its value was not
changed. The Runner is the separate SID ending `-1004`, is not a member of
Administrators, and was not the unexpected ACE. The Task Scheduler Operational
log was disabled, so no historical event record was available. The script
branch, actual DACL, task principal/action, and prior result independently
identify the cause.

## Authorized minimum repair

The user explicitly authorized this exact R5-1H repair scope. Before changing
the key DACL, the prior key bytes, file and directory security descriptors,
effective ACEs, and identity/ownership evidence were saved under the restricted
backup directory:

`C:\ProgramData\coord\backup\g7-20261025T141317Z-65cf9ec3\r5-1h-20261025T225557Z`

Only the DACL on `management.key` changed. The file DACL now has inheritance
protected and contains only SYSTEM and Administrators FullControl. This removes
the one inherited operator `-1001` ReadAndExecute + Synchronize ACE from the
file's effective DACL. The directory DACL/SDDL is unchanged; the parent
`C:\ProgramData\coord` and all other credentials were untouched. Owner/group,
key bytes, and the key's 64-byte length are unchanged. The repair did not add a
Runner ACE or broaden access to Users/Everyone.

The Runner account's actual SID is not in the final DACL and is not in the
Administrators group, so Windows' effective ACL evaluation denies it access.
An additional direct process-token access-check attempt returned
`OpenProcessToken:5` (access denied); that protection was left intact and not
bypassed. The ACL/SID proof is the Runner-denial evidence; a direct read by the
Runner process itself is not claimed. The directory ACL, Runner service
identity, and all unrelated service identities/configurations were preserved.

## SYSTEM acceptance evidence

The existing task was manually triggered using `Start-ScheduledTask` (not an
administrator invocation of the health script). `schtasks /Query /TN
\\coord-health-monitor /V /FO LIST` reported the task principal as SYSTEM,
state Ready, and Last Result 0. The task's configured action and script path
were unchanged. The newly written status file reported `READY`,
`tasks_list=PASS`, `sqlite_read_only=PASS`, and zero consecutive failures.

Observed runs:

| Trigger/result | Task Scheduler evidence | Matching status evidence |
| --- | --- | --- |
| Manual trigger at 2026-10-25 23:01:31Z | Last Result 0 | Fresh `READY`; RPC and SQLite checks PASS |
| Scheduled run at 23:03:15Z | Last Result 0 | New status timestamp at 23:03:16Z; `READY` |
| Next scheduled run at 23:04:15Z | Last Result 0 | New status timestamp at 23:04:16Z; `READY` |
| Continued-run recheck at 23:15:15Z | Last Result 0; next run scheduled | Status timestamp 23:15:16Z; `READY`, `tasks_list=PASS`, `sqlite_read_only=PASS` |

The Coord/Servy/Relay/Runner services remained Running across these checks
(Servy PID 3224, Coord Servy wrapper PID 7444, Core PID 9260, Windows Relay PID
5892, Runner PID 9604). No service restart was triggered by the monitor. The
Runner SID, monitor task XML/action, script hash, and service configuration
remained unchanged.

## Commands and recorded return values

- `Start-ScheduledTask -TaskName coord-health-monitor`: trigger accepted; the
  following Scheduler query recorded Last Result `0`.
- `schtasks.exe /Query /TN "\\coord-health-monitor" /V /FO LIST`: exit 0;
  principal SYSTEM, task Ready, Last Result `0`.
- Read of `C:\ProgramData\coord\health\status.json`: `READY`,
  `tasks_list=PASS`, `sqlite_read_only=PASS`, `consecutive_failures=0`.
- Repeated Scheduler/status correlation for two consecutive minute runs:
  both Last Result `0`, with fresh status timestamps immediately afterward.
- The Windows monitor's automatic-recovery path was not invoked; service PIDs
  stayed stable. This verifies no unintended restart during the monitored runs.

## R5-1 predeployment recheck

The fresh Windows backup at
`C:\ProgramData\coord\backup\g7-20261025T230921Z-cc43635f` remains protected
by a DACL containing only SYSTEM and Administrators. Its SQLite snapshot hash
is `ceea40ca4d25b24a62a5d096e84489a16299c728664d94142320faa5cf53c4c3`,
`integrity_check=ok`, and it contains 145 tasks. The snapshot was captured by
SQLite Online Backup before the R5-1H ACL repair; no production database write
occurred during the repair.

A fresh read-only live audit written to
`rehearsal/r5-live-predeploy.json` still matches the Phase H Batch fingerprint
`7b375744e80233355ac07edafbdcd8a9aff0aa6e66e1b72d3bcdd4818a26c967`, the G7
operator Canary fingerprint
`1cfa89e56d7707ef1f3ed69c8bb77227a97b2636a1b1e5cf5dbbf9a4937f7e63`, and the
browser Canary fingerprint
`2477d3bf16a81de6b938df446c2b0c468c9428aaddf9ce991e1d90e6d88883e1`. The
formal Batch remains 121 tasks (105 completed, 16 pending). No task in the
formal Batch or either old Canary was operated on.

The R4 CI Core (`e7c3695a21f88274cb84992f99a4ce9ce5b2f2bfb229a2768b3c6ccd85bdd13d`)
was started against a new copy of the protected snapshot on isolated loopback
port 17779, using a backup-local test registry and synthetic test keys on
broker port 17780. The test-only Core completed the additive migration, served
an authenticated `tasks/list` (one row requested), and was stopped by its
recorded process ID; both ports were released. The migrated copy has
`integrity_check=ok`, 145 tasks, and 124 `task_attempts` rows. The old G7 Core
(`b23e43f30a076d3d3dc75e90a07a7a3f9de35ad33351bb2e9bf8938f9f220ed4`) then
started against that same disposable migrated copy and completed an
authenticated `tasks/list`; after it stopped, the copy remained
`integrity_check=ok` with 124 attempts. Thus the old program read this migrated
copy successfully; production rollback still requires restoring the preserved
pre-migration DB together with the old binary before any accepted R5 result.

The Linux Coord-only backup at
`/var/backups/coord-r5-20261025T141730Z-efb93fba` remains mode 0700, root-owned;
the recorded archive SHA check and `gzip -t` passed. `coord-relay.service`, the
shared `feathersjsapp.service`, and `openresty.service` remain active with
unchanged PIDs/restart counts at this checkpoint. Production deployment has not
started yet.

## Status and retained limitations

- R5-1H SYSTEM health gate: **PASS**.
- Runner key read denial: **PASS by effective DACL/SID proof**; direct read by
  the Runner process was not exercised because querying its protected process
  token was denied.
- Scheduler Operational history: **NOT_AVAILABLE** (channel disabled before
  this work); new runs were observed directly through Task Scheduler and the
  correlated fresh status file.
- Prior R5-0 execution-boundary **FAIL**: unchanged and retained in
  `PHASE_R5.md`; this authorized, scoped repair does not erase that history.
- R5-1 production deployment: **NOT_RUN at this report checkpoint**; it is
  authorized to continue under `TASK.md` after the live predeployment recheck.

## R5-1 post-repair runtime follow-up — 2026-10-26 00:08–00:20 UTC

This follow-up preserves the R5-1H ACL-repair and successful SYSTEM-run
evidence above. It distinguishes the original `LastTaskResult=2` ACL-policy
failure from the later service-unavailable result after the authorized R5-1
startup attempt.

- The `coord-health-monitor` task was restored to **Enabled** with its original
  SYSTEM principal, action and parameters. The monitor-key ACL repair remains
  in place; no additional monitor-key or other credential ACL change occurred.
- The R4 Core and then the restored old G7 Core both exited at
  `inspect trusted path component` / `Access is denied`. Source inspection
  identifies the failing configured credential path as `broker.key`:
  `RunnerConfig::validate()` accesses its path through `safe_existing_path()`.
  A temporary metadata-only LocalService task returned `Access is denied` for
  that file. Its evidence is `r5-broker-meta-20261026T0030Z.json` in the
  protected rehearsal folder; that diagnostic did not read key contents. The
  key DACL was not modified.
- The all-path LocalService probe recorded metadata access for 15
  noncredential paths, read access to the service executable, action registry,
  fixed action executable and database, and zero reparse points. It is
  preserved as `r5-localservice-path-probe-20261026T0020Z.json` in the
  protected rehearsal folder. All temporary probe Scheduled Tasks were
  removed, transient log files were deleted, and no probe task remains.
- After three fresh RPC failures, the real SYSTEM health task returned
  `LastTaskResult=1`; its new status reported `status=FAIL`,
  `tasks_list=FAIL`, `sqlite_read_only=PASS`, and four consecutive failures.
  Its configured, bounded recovery attempted one start of the Coord service;
  the Servy wrapper exhausted its three attempts and stopped it. The monitor
  did not restart or change any unrelated service.
- Current observed state: Coord service **Stopped**, no listeners on 7777 or
  7779, `CoordActionRunnerG7` **Stopped**, `Servy` and `CoordRelayAgent`
  **Running** with their prior PIDs. The health task remains enabled and will
  report the present RPC failure. The monitor's R5-1H ACL acceptance remains
  **PASS for the four earlier fresh SYSTEM runs**, while current production
  readiness is **FAIL** because Core cannot start under the unchanged Broker
  key ACL.
- The production database was independently backed up after rollback to
  `predeploy-post-r5-rollback-20261026T0027Z.db`; its SHA-256 still matches the
  protected predeploy snapshot, `integrity_check=ok`, schema remains 32,
  `user_version=0`, and `task_attempts` is absent. Protected Phase H and old
  Canary counts/fingerprints remain unchanged. No production Batch operation
  occurred.

R5-1H remains a **PASS** for the exact monitor-key ACL repair and its real
SYSTEM acceptance. This later result is a separate R5-1 deployment blocker,
not a regression in the monitor-key repair. The Broker key is outside the
authorized R5-1H ACL scope, so no change was made to it; production deployment
is **BLOCKED** pending an approved compatibility path.

### Scheduled follow-up — 2026-10-26 00:22:13 UTC

The enabled SYSTEM task completed its next scheduled check with
`LastTaskResult=1`; the fresh health file reported `status=FAIL`,
`tasks_list=FAIL`, `sqlite_read_only=PASS`, and six consecutive failures. Its
next scheduled run was 00:23:23 UTC. Coord and its Runner remained Stopped, no
Coord listener was present, and Servy/CoordRelayAgent PIDs were unchanged. This
is a current Core/RPC availability failure; it does not change the earlier
R5-1H ACL-repair acceptance evidence or the fact that the original
`LastTaskResult=2` ACL-policy cause was corrected.

### R5-1B current SYSTEM recheck — 2026-10-26 14:00:32 UTC

The real enabled `coord-health-monitor` task was read through direct OpenSSH;
the query exited 0 and performed no writes. Its latest scheduled run was
14:00:00Z with `LastTaskResult=1`; next run was 14:01:01Z. The fresh status
reported `FAIL` and 825 consecutive failures. Coord and
`CoordActionRunnerG7` were Stopped, `Servy` (PID 3224) and `CoordRelayAgent`
(PID 5892) remained Running, and no 7777/7779 listener existed. The current
legacy status JSON has no structured Core/DB/Broker/Runner fields. The exact
sanitized snapshot is
[`PHASE_R5_B1_LIVE_RECHECK_20261026T140032Z.json`](PHASE_R5_B1_LIVE_RECHECK_20261026T140032Z.json).

This remains a production health **FAIL**, separate from the earlier scoped
R5-1H ACL repair **PASS**. No monitor-key or Broker-key ACL, secret contents,
Core service, Runner service, or production database was changed during this
recheck. The newly authorized R5-1B architecture work is still awaiting a
passing Fork Windows run before any protected-copy rehearsal or production
recovery.

### R5-1B SYSTEM health recheck — 2026-10-26 14:45:06 UTC

Direct OpenSSH read-only query exit code: 0. The enabled task still runs as
SYSTEM, state Ready, with `LastTaskResult=1` at 14:44:44Z. The correlated
status file was fresh at 14:44:18Z and reported `FAIL`, `tasks_list=FAIL`,
`sqlite_read_only=PASS`, and 869 consecutive failures. It has no structured
Core/DB/Broker/Runner fields. Coord and its Runner remain stopped, with no
listener on 7777/7779; Servy and CoordRelayAgent remain running. The snapshot
is `PHASE_R5_B1_LIVE_RECHECK_20261026T144506Z.json`.

This is a continuing production health **FAIL**, not a regression of the
earlier R5-1H ACL repair. No task was triggered, no monitor or Broker
credential ACL was changed, and no production service or database write was
issued during this observation.

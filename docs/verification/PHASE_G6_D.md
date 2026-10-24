# Phase G6-D — build and controlled-deployment gates

Date: 2026-10-24 Australia/Brisbane. Overall Phase G6: **PARTIAL**.
Evidence closeout: 2026-10-25 Australia/Brisbane.
Production deployment and canary: **NOT_RUN**, blocked by the real identity
gate in G6-A and the unexercised production account/ACL gate in G6-B.
The user authorized deployment after the gates pass; permission does not
substitute for acceptance evidence.

## Protected baseline and scope

Work uses `codex/phase-g6-production-readiness`, starting at
`9e1543e9bddef06c6009edf09b8056b63fdff852`. The verified user Fork is
`lieyanbang/coord`; upstream `DmarshalTU/coord`, main/release branches and
history are protected. Pre-existing `.gitignore` changes and unrelated
untracked files are excluded from staging. `TASK.md` is read-only, SHA-256
`25afd40a7fc6ce9c552dcf3c36f9d64cf0e1abeea48db0fffb54878c90c188c9`.
No formal emote task, classification Agent or production canary was created.

The known production daemon was inspected without acquiring production
engineering leases or writing a ledger task. Stage commits and these reports
form the engineering ledger; no task/ack identifiers are invented. All
automated lifecycle and fault tests use disposable databases and processes.

## Current production observations

Direct SSH was used; Remote Control 4 was not used. At 09:56:11Z Arasaka
coord and CoordRelayAgent were Running under LocalService; Servy was Running
under LocalSystem. coord listened only at `127.0.0.1:7777`, installed binary
SHA-256 `b58d3620b1b6de8035bfcb97e99f2e4fe654f48b1cb5719ef9fca013f5652e0d`.
Read-only urgent/high counts were both zero. No actions configuration was
found in the inspected ProgramData coord top-level layout. Evidence:
`stage-g6-arasaka-readonly.log`, SSH exit 0.

At 09:58:17Z Feathers, OpenResty, Sidecar, relay ACL and Tailscale were active;
Sidecar reported READY, agent connected, zero pending. Existing 13011/13012
listeners remained present. Evidence: `stage-g6-linux-csp-readonly.log`, SSH
exit 0. The earlier `stage-g6-linux-readonly.log` contains useful observations
but its combined command exited 1 because of a CRLF heredoc terminator error;
it is retained as failed command evidence and not labelled PASS.

Public route probes each exited 0: CoordMCP root 401, both metadata routes
200, authorize 302, RemoteControlMCP/2 302, RemoteControlMCP3 and `/mcp` 401.
See `stage-g6-public-readonly.json`. These are route/challenge observations.

Production source, service configuration, Access policies, task database and
queues were not changed. Browser verification did perform normal same-account
OAuth reauthorization, changing OAuth grant/token records. That exception is
explicit; it was not a read-only credential-store operation. The subsequent
server audit itself was read-only.

At 10:33:17Z both direct SSH preservation snapshots exited 0:
`stage-g6-arasaka-preservation.json` and `stage-g6-linux-preservation.json`.
The installed hashes, loopback daemon PID 5368 and Linux Feathers/Sidecar PIDs
113349/90506 were unchanged; Linux restart counters stayed zero, all inspected
services stayed active and Sidecar stayed READY with no pending request.
Arasaka urgent/high pending counts remained zero. The service wrapper PID is
distinct from the daemon listener PID and is not used as daemon liveness proof.

A final read-only preservation check at 11:00:03Z also returned SSH exit 0 on
both hosts (`stage-g6-arasaka-final.json`, `stage-g6-linux-final.json`). Installed
hashes and the daemon/Feathers/Sidecar PIDs above still matched; services were
active, Linux restart counters remained zero, Sidecar was READY with zero
pending requests, and Arasaka urgent/high pending counts were still zero.
The task-count queries explicitly used `skip_reclaim: true`.

## F3-R evidence reconciliation

Production `oauth.cjs` SHA-256 is
`0dcc483bd3d6afd5f4625885aed70f5cdf1b30f6a44ced739549d382ce049c53`, matching
the local pre-G6 module. The source excerpt confirms `form-action 'self'`
plus only the validated callback origin. Production `index.cjs` SHA-256 is
`e2a4c125f6742f915662707ff123f3eb0a3e3ec6b64ae2818baf1c3ea7cd5db3`.
The old G0 hash discrepancy is reconciled for this observation.

At 10:16:48Z a read-only journal/access-log audit exited 0. Since 09:50Z,
Sidecar recorded four dispatched and four completed `tasks/list` calls and no
other method; OpenResty recorded authorize POST 302 once, token POST 200 twice,
and CoordMCP POST 200 four times. Only method, stripped path, status, counts and
times were emitted. OAuth state had two clients, 16 access and 16 refresh
records; all access records shared one client digest and had no conversation
field. See `stage-g6-browser-server-audit.json` and G6-A for UI details.
These observations establish real read calls and successful token exchange,
but do not attribute individual credentials to the two chats. Separate
`initialize`/`tools/list` responses and real non-owner Access rejection remain
**UNVERIFIED / NOT_RUN**. F3-R remains **PARTIAL**.

## Verification ledger

`pwsh -NoLogo -NoProfile -File scripts/verify.ps1 -Stage g6` ran from
10:19:34Z through 10:22:31Z and exited **0**. All seven checks exited 0:
fmt, strict all-target/all-feature locked clippy, all-feature locked tests,
locked release build, Relay/live-daemon tests, CoordMCP tests and Python tools.
Rust reported 64 passed, zero failed and eight ignored (seven explicit child
helpers plus the opt-in capacity test); Relay 20/20, CoordMCP 14/14 and Python
7/7. Commands, durations and raw logs are in `stage-g6-checks.json` and the
matching logs. These are local Windows source/isolated acceptance results.

After integration, `scripts/verify.ps1 -Stage g6-final` ran from 10:29:59Z to
10:31:18Z, exit **0**, with all **eight** checks passing. It adds the real
Windows PowerShell 5.1 health-monitor test: protected key -> authenticated CLI
READY; missing/malformed/oversized/relative/unprotected keys -> configuration
exit 2 with no recovery; previous environment restored before the fake Servy
recovery call. The fixture initially inherited broad TEMP permissions and was
correctly rejected; only its new random directory ACL was tightened for the
successful test. Actual SYSTEM task configuration and key-copy rotation remain
production gates. No real Servy call or existing ACL was changed by the test.
The release hashes below remained identical after integration.

Existing action-runner tests accept explicit `COORD_TEST_PUBLISHER_BIN` and
`COORD_TEST_RUNNER_BIN` overrides for later downloaded-artifact verification;
their defaults remain Cargo's own test executables. `cargo run --locked --
--version` also exited 0 after preserving `default-run = "coord"` alongside
the new auxiliary binaries. Raw log whitespace is retained; source/doc diff
whitespace checks pass with the raw `stage-g6*.log` evidence excluded.

Local release SHA-256 values:

| Binary | SHA-256 |
| --- | --- |
| coord.exe | `0386e67717694f59e066b542254f873340ca787fda72727beff2454dceeee875` |
| coord-action-runner.exe | `5dd0bcc04b37be5c46a39f6543f54f797a01839858dd572c5ce426fef85b4909` |
| coord-emote-action.exe | `0461bc1fb7007824099689214de74c21dfcf74ae3bc261d8dd1fbfff18f73a1f` |

The capacity harness was explicitly invoked with `COORD_TEST_BIN` pointing to
the release executable above and `--ignored --nocapture` (exit **0**). The
compiled harness from the full test gate ran in a separate temporary report
directory so historical Stage E evidence was not overwritten. Raw output and
JSON are `stage-g6-capacity.log` / `stage-g6-capacity.json`. Six independent MCP
processes completed 100, 1,000 and 10,000 generic tasks with equal unique task,
receipt and terminal totals, and zero busy/locked log lines in all sizes.
At 10,000 tasks: 94.482 seconds, 105.84 tasks/s, claim P95 47.36 ms and daemon
peak working set 22,089,728 bytes. These are bounded local observations, not a
controlled comparison with historical runs or a production performance promise.

Fork Actions and downloaded-artifact outcomes are recorded below; a pending
or partially exercised run does not establish PASS.

The first Fork run [40086336362](https://github.com/lieyanbang/coord/actions/runs/40086336362)
on implementation commit `2fccbd3dc853af07cc3a439914f87d4bd5602d79`
finished **FAIL**. Fmt, clippy, Rust tests, release build and health-monitor
checks succeeded; the dedicated-account rehearsal exited 1 during identity
validation. Its standard user could not inspect a trusted path ancestor under
runneradmin's private TEMP directory (`Access is denied`, OS error 5). Later
Relay/MCP/Python steps were skipped and no binary artifact was produced.
The failure artifact `12557665426` was downloaded: ZIP SHA-256
`3088ece0f354a99de3bacd899ad5d28aa97b7c9daa8d256641867d1ae9ad3468`,
517 bytes. Its summary and stderr are retained as `stage-g6-ci-first-summary.json`
and `stage-g6-ci-first-identity.log`; the disposable account was removed.

The rehearsal fixture now uses a random `coord-g6-<32hex>` directory directly
under the system volume root. Cleanup requires that exact volume-root parent
and fixed GUID leaf pattern. Rust ancestor/reparse validation, account token
checks, ACL denials, RPC authentication and crash gates remain unchanged.
This repairs a disposable fixture layout and does not establish a production
permission result. The replacement run's actual result is recorded below.

Before the fixture-fix commit, `scripts/verify.ps1 -Stage g6-cifix` exited
**0**, with all eight local checks passing. The full commands, timestamps,
durations and raw logs are retained in `stage-g6-cifix-checks.json` and its
referenced logs. The account-creating rehearsal is restricted to disposable
Actions hosts and was not run on the operator's workstation.

The second Fork run [40087182174](https://github.com/lieyanbang/coord/actions/runs/40087182174)
on `6dc3fcdd2a364fce13875269235ddd539dcbf79c` also finished **FAIL**.
It passed the actual standard-user identity check (`privileged: false`), ACL
probe (five reads, three denied reads, three denied writes, four denied write
directories and three writable directories), management RPC 401/401, and native
task publication/indexing. The crash path was not accepted: a `Stop-Process`
NullReferenceException while cleaning up the cross-user runner obscured the
original error and prevented summary/crash-stderr preservation. Crash recovery,
account cleanup and fixture cleanup therefore remain unaccepted for this run.
Later test steps were skipped; no binary artifact was produced.

Artifact `12557307856` was downloaded and hash-verified: 3,207 bytes, ZIP SHA-256
`5655c41eed66dc7ed9fecfa4c3b035625bfd3814a4bc7a89b76fd62b95e131ef`.
All nine retained files are in `stage-g6-ci-second/`; run metadata and the
failure excerpt are `stage-g6-ci-second-run.json` and
`stage-g6-ci-second-failure.log`. The final rehearsal now retains both stderr
sources, attempts each exact cleanup independently, always attempts summary
serialization, and marks cleanup errors FAIL while failing the job.

All three owned-process stop sites now explicitly use `-Force -Confirm:$false`
and bounded exit waits. PowerShell's documented/source-confirmed default asks
for confirmation for another user's process; Force skips that branch and still
executes the kill. This is scoped to fixture-created PIDs. The Job child-death
and non-idempotent no-replay assertions remain mandatory. AST, original-error
preservation and eight in-memory cleanup-fault scenarios passed; see
`stage-g6-ci-cleanup-focused.json` / `.log`. Those mocks do not replace a real
cross-user process-stop or crash-recovery run.

Before committing the final stop/cleanup repair, `scripts/verify.ps1 -Stage
g6-cihard` completed with exit **0** and all eight checks passing. Commands,
times and logs are in `stage-g6-cihard-checks.json` and its referenced files.

### Accepted Fork run and downloaded binaries

The third run [40088013915](https://github.com/lieyanbang/coord/actions/runs/40088013915)
on `7d08012a171f804edf3b8588b77c72720e76393d` completed **success** at
2026-10-24T11:06:14Z. All Windows job steps succeeded: fmt, clippy, 64 Rust
tests (eight intentional ignores), release build, health monitor, dedicated
low-privilege rehearsal, Relay 20/20, CoordMCP 14/14 and Python 7/7.
`stage-g6-ci-success-run.json`, `stage-g6-ci-success-metadata.json` and the
extracted `stage-g6-ci-success-tests.log` retain the commit, step outcomes,
artifact linkage, actual test counts and upload digests.

Both artifacts were downloaded and their ZIP SHA-256 matched GitHub metadata:

| Artifact | ID | Bytes | ZIP SHA-256 |
| --- | --- | ---: | --- |
| coord-windows-2022 | 12558245794 | 6,449,718 | `5ed14f0501b8bc59c47837c06f7d12a1eda37195b63ba82c6006658ce40c88b9` |
| g6-low-privilege-evidence | 12557333701 | 4,698 | `73985f6e870c745d448898e1e1fb4f4e6f7f7aebdd751df629542572231f0c4e` |

All 13 real low-privilege evidence files, including the summary, are retained
in `stage-g6-ci-success/`. Its five checks exit 0; actual runner identity is
nonprivileged and differs from the daemon; ACL/RPC denials and both native
publications pass. The interrupted runner's child dies, the non-idempotent
action recovers as `uncertain` without replay (attempt 1), and its task remains
completed. Both temporary account and fixture removal are verified true,
`cleanup_errors` is empty and `production_gate` remains `NOT_RUN`.

Downloaded executable SHA-256 values:

| Binary | SHA-256 |
| --- | --- |
| coord.exe | `50577ece7d072b026f053905afb74a9012c086dc2120e3e40cec450c28e50618` |
| coord-action-runner.exe | `4655168f1fe3a0e60cf178736858d445fe5dc7d0962a3f92d01dcff7d13b09a9` |
| coord-emote-action.exe | `ff539b0fb8777764440d0cade76be028ff6506d64f8c0687120651a32aa43884` |

All three differ from the local release hashes above, so byte reproducibility
is **NOT_CLAIMED**. Their downloaded entry points (`--version` / `--help`)
exit 0. With `COORD_TEST_BIN`, `COORD_TEST_RUNNER_BIN` and
`COORD_TEST_PUBLISHER_BIN` pointing to those exact downloaded files, the focused
`cargo test --locked --test action_runner --test agent_actions --test
agent_progress` exits 0: **27 passed**, zero failed, seven explicit child-helper
ignores. The harness still links the local Rust library; this is executable
integration validation, not a second standalone cloud build. Commands, exits,
logs and local/cloud hash comparison are in `stage-g6-artifact-recheck.json`,
`.log` and `stage-g6-artifacts.json`. The checked binaries remain under
`target/g6-artifacts/third-windows/`; they were not installed on production.

The final local `scripts/verify.ps1 -Stage g6-close` also exits **0**, all eight
checks passing. `stage-g6-close-checks.json` and its raw logs retain the result.
The evidence-only closeout commit uses `[skip ci]`; its unchanged implementation
is the exact cloud-tested source commit above. No claim is made that the
documentation commit itself produced another Actions artifact.

The initial parallel-edit `g6-baseline` invocation failed at fmt because an
in-progress module declaration preceded creation of its source file. Its
`stage-g6-baseline-checks.json`/fmt log are retained as intermediate failure
evidence, not a clean baseline. Focused before/after regression evidence is
documented by G6-B/C and the Python aggregate regression logs.

## Deployment disposition

The backup/rollback sequence is in `docs/phase-g/G6-DEPLOYMENT.md`. No production
backup, binary replacement, account/ACL provisioning, restart, identity-policy
enablement or canary was performed while A/B remain open. A dedicated CI
Windows account and disposable ACL fixtures can validate the mechanism but
cannot prove Arasaka ACLs. G6-C's real two-chat lifecycle is blocked by A.
ChatGPT background autonomous wake and 24-hour unattended operation remain
unverified. The 121 formal tasks and classification agents remain prohibited.

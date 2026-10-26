# R5 candidate F: Windows PowerShell 5.1 native capture

Scope: the user authorized candidate F on 2026-10-27. `TASK.md` and
`docs/PLAN.md` remain read-only. Baseline is `9d865cf0aa4ce67aaf930e84a25a7e1e6a053ed5`;
branch is `codex/ps51-runner-fixture-20261027`. Existing dirty/untracked work is
preserved. No production connection, shared ledger write, Batch operation,
deployment, or subsequent phase is part of this candidate.

The LocalService embedded script now uses `Start-Process` with separate stdout
and stderr files and waits for the actual process. It records and returns
`Process.ExitCode`, preserving PowerShell exceptions as a separate exit 98.
This prevents PowerShell 5.1 from converting ordinary Rust stderr into a
terminating `NativeCommandError`. Core/Runner Rust code and authentication
policy are unchanged. Existing diagnosis from run
[40348694762](https://github.com/lieyanbang/coord/actions/runs/40348694762)
was reused without repeating B0 or production inspection.

## Local evidence

Windows PowerShell **5.1.26100.9711**, `$ErrorActionPreference = 'Stop'`;
the test extracts the actual embedded startup wrapper from the before/after
script and runs a native console process with distinct stdout and INFO/DEBUG
stderr. Raw scripts, reports and streams are retained in the ignored directory
`target/ps51-fixture-20261027/`.

| Check | Actual result |
| --- | --- |
| Original wrapper, native exit 0 + INFO stderr | Reproduced exit 98, `NativeCommandError` |
| Fixed wrapper, native exit 0 + INFO/DEBUG stderr | PASS: exit 0; both streams preserved separately |
| Fixed wrapper, native exit 23 + INFO/DEBUG stderr | PASS: exit 23 in wrapper and startup JSON; diagnostics preserved |
| Real release Core through fixed PS5.1 wrapper | PASS: Core/database READY, discovery and JSON-RPC `tasks/list`, process alive over a 3-second observation, INFO stderr retained; exact owned process stopped afterward |
| Real invalid empty registry through fixed wrapper | Rejected with native exit 1 and `Error: unsupported or empty action registry`, without a PowerShell exception |
| `pwsh -NoProfile -File scripts/verify.ps1 -Stage r5-1b-f` | PASS: fmt/clippy/locked tests/release all exit 0; 88 Rust tests passed, 8 intentionally ignored; no skipped-executable messages |
| `node --test relay/relay.test.mjs relay/live-coord.test.mjs relay/logical-workers.test.mjs` with release `COORD_TEST_BIN` | PASS: exit 0, 27/27 |
| `npm test --prefix relay/coord-mcp` | PASS: exit 0, 18/18 |
| `python -m unittest discover -s scripts/phase_g -p test_phase_g_tools.py -v` | PASS: exit 0, 8/8 |

Gate commands, exit codes, timings and log paths are in
`docs/verification/stage-r5-1b-f-checks.json`; the four gate logs remain beside
it locally. Additional raw reports are `native-capture-results.json`,
`real-core-results.json`, `relay.log`, `mcp.log`, and `python.log` under the ignored
directory above. The Rust suite includes 22 Action Runner tests, including
signed tampering/expiry/restart-replay rejection and current receipt/attempt/fence
checks. No capacity claim is made from the ignored capacity test.

The local Core check ran as the operator. This workstation has no administrator
token; the existing account/ACL rehearsal is deliberately restricted to a
disposable GitHub runner. Real LocalService/private-key denial, dedicated Runner
publication and crash recovery are therefore assessed by the Hosted fixture,
separately from local readiness. Historical SYSTEM-monitor evidence is
reused locally; the Hosted workflow independently runs its monitor gate.

## First Hosted candidate: real checks passed, cleanup failed

Candidate `ab7a4641b928cb7c9627afaf91d71a1c21c56e6b`, run
[40375610957](https://github.com/lieyanbang/coord/actions/runs/40375610957),
job `121757730745`: fmt, clippy, Rust tests, release and monitor passed.
The low-privilege fixture reached `phase=completed`: real LocalService SID
`S-1-5-19`, public key readable/private key denied, Runner identity/ACL,
task/index signed publication and crash recovery all passed. Both publication
actions succeeded at attempt 1; the non-idempotent crash became `uncertain`
at attempt 1 and its Job Object child was killed.

Overall **FAIL** was solely `cleanup-localservice-core-process`:
`Cannot find a process with the process identifier 9720.` The task, account
and fixture were removed. Stopping the scheduled task also terminates its
new `Start-Process` child, which can disappear between the cleanup CIM snapshot
and `Stop-Process`. This is an adjacent fixture race exposed by this change,
not evidence of a Core/Runner implementation failure.

Diagnostic artifact
[12674997005](https://github.com/lieyanbang/coord/actions/runs/40375610957/artifacts/12674997005),
5304 bytes, was downloaded and its ZIP SHA-256 matched GitHub's digest:
`319cab8fc600a83ba4dc68566ab5edc0f3a34d470b4d544cccaa86c8e224b084`.
Raw ZIP, extracted `summary.json` and job log are under
`target/ps51-fixture-20261027/first-ci-*`. This is a diagnostic ZIP, not a
deployment package. Hosted Node/Relay/MCP/Python and packaging were skipped.

The same script now tolerates only `NoProcessFoundForGivenId` at both exact
Core stop sites. Other errors still propagate; exact executable/database
guards, bounded wait and final absence assertions remain. An AST-extracted
local test exercised both guards against an already exited process, a live
owned process and an injected unexpected error: all six cases passed
(`cleanup-race-results.json`). The startup wrapper and Node/Rust code did not
change, so their focused local evidence above remains applicable. A fresh
pre-commit gate is recorded in `stage-r5-1b-f-cleanup-checks.json`: fmt,
clippy, all-feature locked tests and release all exited 0 (88 passed,
8 intentionally ignored). Original protected files and every pre-existing
untracked path were verified unchanged.

## Final candidate: PASS (local and Hosted), production NOT_RUN

Tested implementation commit: `ea9783268e33e546572fe346335056e28bc4d435`.
Fork Windows run
[40377126693](https://github.com/lieyanbang/coord/actions/runs/40377126693),
job `121763119437`, completed **success**. All required steps passed:
fmt/clippy/88 Rust tests/release, monitor, real LocalService and dedicated
Runner rehearsal, Relay 27/27, MCP 18/18, Python 8/8, packaging and uploads.
Signed tampering/expiry/restart-nonce-replay and action receipt/attempt/fence
negative tests passed in that same Hosted run.

Downloaded low-privilege evidence
[12676791138](https://github.com/lieyanbang/coord/actions/runs/40377126693/artifacts/12676791138)
has `state=PASS`, `phase=completed`, five expected exit-0 checks, LocalService
public-key read/private-key denial, both signed actions succeeded, and crash
recovery `uncertain` at attempt 1 with its child killed. Task/account/fixture
removal is true and `cleanup_errors=[]`. Its 5246-byte ZIP SHA-256 matches
GitHub: `5167e65472656e7ba99551318fd03ab68a51e0151e17577f0c4c728f2be858e2`.

Downloaded build artifact
[12677086008](https://github.com/lieyanbang/coord/actions/runs/40377126693/artifacts/12677086008)
is 13,587,517 bytes. Its manifest commit and platform match the tested candidate.
The outer GitHub archive, inner deployable ZIP, three executables and health
script were independently hashed; inner ZIP contents match the manifest too.

| Object | Verified SHA-256 |
| --- | --- |
| GitHub artifact ZIP | `c9b13286f93c5623b3db2248517155484935dc96608f43eb073345f45b76f47e` |
| `coord-r5-1b-windows-2022.zip` | `c4edd881f2f52da702389fc37615c2eb1350d96728fa6bec82378e0064ba49a3` |
| `coord.exe` | `943959b12ce75f8de716320e9fe15e72e3ea2911dbf096bedc6dc4bcfc22f3bc` |
| `coord-action-runner.exe` | `72b46d25722842a30afd6cd3db32c9df5e15ae28fd0a9cf9a2ddb7a14dcb4baa` |
| `coord-emote-action.exe` | `a6679db9064f10bfa74008458853db069ce5b79f018a3b6685710546e13c4879` |
| `coord-health-monitor.ps1` | `efcee606cf5cb2483f0985b7e939fa661bf13b4035fb81a164a81b4cb7af6a25` |

Downloaded Core `--version` reports 0.4.1; Runner and publisher `--help` all
exit 0. Exact downloaded bytes and manifest are retained under
`target/ps51-fixture-20261027/hosted-artifact/`; final low-privilege evidence is
under `final-evidence/`, with `final-ci-run.json`, `final-ci-job.log`, and
`artifact-verification.json` alongside. This evidence-only report follow-up
reuses the completed gates; it does not change the tested implementation or
request another CI run.

Candidate F is complete with no remaining known in-scope blocker. This is a
candidate for separately authorized evaluation, not production readiness.
Production DB-copy migration/rollback/recovery (P/B3), real ChatGPT Web (W/B4),
formal Batch operations, capacity reruns and Linux/macOS remain NOT_RUN in this
task. Existing historical failure records and current PLAN/TASK were preserved.
Stop after this candidate; no subsequent stage is started.

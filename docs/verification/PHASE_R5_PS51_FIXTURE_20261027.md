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
publication and crash recovery are therefore assessed by that unchanged Hosted
fixture, separately from local readiness. Historical SYSTEM-monitor evidence is
reused locally; the Hosted workflow independently runs its monitor gate.

# Client onboarding optimization

Verified on Windows x86_64 MSVC, 2026-10-31 (Australia/Brisbane).
Branch: `codex/client-onboarding`; base: `a3faa56e66fd1ae3310458aa88a73baff7f366cd`.

`coord init` now embeds `src/cli/client-agents.md`, independently of repository
development instructions. The guide covers ID-selected tasks, generic claim-next,
trusted Worker batches, progress, failure, recovery, fencing and idempotency.
README/API document the same paths, including local/Relay schema differences.
Initialization hints respect skipped files and explain client MCP registration.
Arguments, generated MCP config and default overwrite protection are preserved;
no scheduler, storage, RPC/MCP interface or CI workflow changed.

## Verification

`pwsh.exe -NoProfile -File scripts/verify.ps1 -Stage client-onboarding`
completed with exit 0. [Machine-readable commands, exits and timings](stage-client-onboarding-checks.json):

| Check | Exit | Evidence |
| --- | --- | --- |
| `cargo fmt --all -- --check` | 0 | [log](stage-client-onboarding-fmt.log) |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | 0 | [log](stage-client-onboarding-clippy.log) |
| `cargo test --all-features --locked` | 0 | [log](stage-client-onboarding-test.log) |
| `cargo build --release --locked` | 0 | [log](stage-client-onboarding-build.log) |
| Release executable `init target/client-onboarding-smoke-20261031`, followed by guide/config readback | 0 | [log](stage-client-onboarding-release-init.log) |
| `git diff --check` | 0 | No whitespace errors |

Tests: 105 passed, 0 failed, no silent skip messages. Eight existing ignored
entries comprise seven Action child helpers and the opt-in capacity suite;
capacity was not run. All four new CLI integration tests passed: default
generation, nested directory/default overwrite protection, force/repeated init,
and each skip-option combination including force. Existing lifecycle, Batch,
MCP fencing, validation and trusted recovery regressions passed in the same run.
The generated Release guide matches the independent template and MCP command/args
remain `coord mcp`. Existing Windows CI discovers `tests/init.rs` through its
unchanged cargo-test job; no hosted run was triggered for this local commit.

## Scope and ledger

No usable local shared daemon was configured (`COORD_URL` unset; no listener on
default port 7777). This branch's Git commit is the work ledger; no task/ack IDs
were invented. Tests used isolated temporary daemons, not a shared or production
database. No production deployment, formal Batch/Action or Emote work was run.
The pre-existing `.gitignore`, `docs/PLAN.md`, root `TASK.md` and recovery-report
changes retain their starting SHA-256 values and are excluded from this commit;
other pre-existing untracked files remain in place. PLAN/TASK were not edited.

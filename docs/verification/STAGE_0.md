# Stage 0 — baseline and CI

Date: 2026-10-20. Starting SHA: `44fe390fac64ed4828d479896d7079da272c0152`.
Operator: `ledger-otter`; single Codex, no delegated agents.

## Bootstrap and preservation

No coord executable on PATH, registered coord MCP, repository `.mcp.json`, or
running coord process was available. TASK.md section 0.3 explicitly permits
Git/evidence bootstrap; no task or ack IDs are fabricated. Initial untracked
`TASK.md` and `docs/PLAN.md` are preserved, with TASK.md unchanged. No other
operator changes were observed across repeated status checks.

Work is isolated on `codex/staged-scheduler`. `origin` is the upstream and is
never a write target. GitHub authenticated identity and Fork metadata confirmed
`lieyanbang`, `lieyanbang/coord`, fork=true, push=true, Actions enabled=true.
The separate `fork` remote points to that repository. No tags or Releases.

The old planning-only sentence in PLAN section 8 describes the earlier planning
turn. The current user request and read-only TASK.md authorize implementation.

## Baseline evidence

Windows x86_64 MSVC; rustc 1.95.0 (59807616e), Cargo 1.95.0.
Commands and original outputs are in adjacent `stage-0-*.log` files.

| Command | Exit | Result |
| --- | --- | --- |
| cargo fmt --all -- --check | 0 | PASS |
| cargo clippy --all-targets --all-features --locked -- -D warnings | 0 | PASS |
| cargo test --all-features --locked | 0 | PASS with coverage limitation below |
| cargo build --release --locked | 0 | PASS |

Baseline tests report 13 integration cases, but five HTTP/process/TUI cases silently
return on Windows because they look for `coord` rather than `coord.exe`.
This is NOT evidence that their scenarios passed. Stage 0 fixes the test binary
lookup using Cargo's mandatory CARGO_BIN_EXE_coord and replaces shell-script
clients with 16 independent executable processes racing each task. The portable
suite and strict clippy both exit 0; no conditional test skips remain.
The stale completion test also permits unsafe completion; Gate B must replace it.

No deployed old database was supplied or found in the repository. Migration
acceptance will use a database created by the unchanged baseline executable,
on a disposable copy. Rollback is restore the pre-upgrade backup with the old
binary; never downgrade the only database in place.

## Workflow changes

Per the user's subsequent 2026-10-20 instruction, Windows is the primary and
only default supported platform. Linux/macOS are NOT_RUN and excluded from
both CI and Release build matrices. This supersedes TASK.md's original
three-platform requirements without editing that read-only contract.

CI push on all branches and PR: Windows 2022;
fmt, locked clippy/test/release build, Rust cache, per-platform artifacts
`coord-windows-2022` (missing files fail).
`cargo fmt` does not resolve dependencies and does not support `--locked`.
Release remains exclusively `v*` tag triggered, builds locked dependencies,
and no longer writes to any Homebrew repository. MIT copyright unchanged.

## Gate 0: PASS

Commit `572771a76a61851a2a11d32fea2a0a95441bf584`.
[Actions run 39428331459](https://github.com/lieyanbang/coord/actions/runs/39428331459)
completed **success**: fmt, clippy, tests, release build and upload all succeeded.
Artifact `coord-windows-2022`, ID `12266528775`, ZIP size 4,028,425 bytes,
was downloaded and extracted; its `coord.exe version` executed successfully
(exit 0, version 0.4.1). Executable SHA-256:
`2e92e9aaa42fb09f4950d74212642723f4616501fc84950ac79e3f711c40cd1c`.

An unchanged baseline executable created the isolated legacy database: completed
work, linked ack, pending work, one agent and Vault notes. A SQLite backup is
retained locally under ignored `target/verification/` for later migration tests.

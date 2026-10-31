# Repository Guidelines

## Project Structure & Module Organization

- `src/core/` owns SQLite persistence, task lifecycles, validation, and Actions/Runner logic.
- `src/a2a/` exposes JSON-RPC; `src/mcp/` provides stdio MCP; `src/cli/`, `src/server.rs`, and `src/vault/` cover commands, hosting, and optional projections.
- `tests/` contains Rust integration and capacity tests. `relay/` contains the Node Sidecar/Agent, identity policy, remote MCP facade, and colocated tests.
- `scripts/windows/` holds operational tooling; `scripts/phase_g/` contains business utilities. `docs/` contains API documentation and verification evidence.

## Build, Test, and Development Commands

Windows is the primary CI platform. Use stable Rust with rustfmt/Clippy, PowerShell, Node 22, and Python 3.12.

- `cargo build --locked`: build development binaries; add `--release` for optimized binaries.
- `cargo test --all-features --locked`: run Rust unit/integration tests.
- `cargo fmt --all -- --check`: check Rust formatting.
- `cargo clippy --all-targets --all-features --locked -- -D warnings`: enforce warning-free linting.
- `pwsh -NoProfile -File scripts/verify.ps1 -Stage local-check`: run formatting, Clippy, tests, and Release build; save evidence.
- `just serve`: run an isolated daemon on `127.0.0.1:7788` using `.coord/dev.db`.
- `npm ci --prefix relay/coord-mcp --ignore-scripts`, then `npm test --prefix relay/coord-mcp`: install locked dependencies and test MCP/OAuth.
- `node --test relay/relay.test.mjs relay/live-coord.test.mjs relay/logical-workers.test.mjs`: test Relay integration.
- `python -m unittest discover -s scripts/phase_g -p test_phase_g_tools.py -v`: test Python utilities.

## Coding Style & Naming Conventions

Use four-space Rust/Python indentation and two-space JavaScript indentation. Follow rustfmt; use Rust `snake_case` functions/modules and `PascalCase` types. Preserve existing `.mjs`/`.cjs` module formats and JavaScript `camelCase` naming. Keep changes focused and public interfaces compatible.

## Testing Guidelines

Use Rust's test harness, Node's `node:test`, and Python `unittest`. Name tests after observable behavior; add regressions for changed lease, fencing, retry, receipt, and authorization behavior. Use isolated databases and ports. Before live-Core or opt-in capacity tests, build Release and set PowerShell's `$env:COORD_TEST_BIN` to the resulting `coord.exe`. Ignored tests are not passing evidence.

## Commit & Pull Request Guidelines

Use concise imperative subjects, such as “Simplify init client guidance”; optional `fix:` and `docs(scope):` prefixes appear in history. Keep commits scoped. PRs should describe motivation, compatibility impact, linked issues, and exact validation commands/results; include screenshots for UI changes.

## Security & Execution Boundaries

Read the current `PLAN.md` and `TASK.md`, when present; leave them unchanged unless authorized. Preserve dirty worktrees and use `codex/` branches for new work. Keep RPC loopback-bound, real credentials out of Git, and MCP diagnostics on stderr. Production deployment and formal Emote processing require explicit authority.

## Task Operations

For creating custom batches, authorizing remote workers, and managing task lifecycles, read `docs/COORD_USAGE.md`. Worker MCP tools do not create batches; management uses trusted loopback JSON-RPC. Check the guide's version and live deployment permissions before writing.

Trust the authenticated shared entry, not individually registered workers. Version-3 logical policy grants persistent, revocable task-pool access to an approved OAuth client and accepts any valid stable worker label. New batches need no additional binding or 24-hour renewal. Task leases and attempt fencing bound execution ownership; OAuth expiry and short-lived Relay signatures remain transport protections. Do not introduce routine database freeze/unfreeze steps or grant publication authority through a worker label. Preserve version-1/2 consumers where still configured.

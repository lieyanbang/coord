# Working on coord

## Repository development (Codex)

- Read `TASK.md` as the read-only execution contract and `docs/PLAN.md` for
  scope, gates and current decisions. The latest user instructions take priority.
- Windows is the primary and only default supported platform. Linux/macOS are
  optional source ports, not default CI or release targets.
- Inspect Git status, preserve other work, and use a new local `codex/` branch.
  Work alone unless the user explicitly requests delegation. Do not start a
  shared daemon, alter an existing database, or kill another operator's process.
- A working deployed coord is **not** a bootstrap dependency. If no known shared
  daemon is available, record this in `docs/verification/` and use stage commits
  as the ledger. Never invent task/ack IDs. Isolated ephemeral test daemons do
  not become the operator's shared ledger.
- If a known shared daemon is available: heartbeat with a stable two-word handle,
  inspect pending urgent/high tasks before edits, then declare and atomically
  claim the work. Renew ownership while working; finish with an atomic ack.
  Resolve overlapping work before touching its files.
- Before each stage commit run `scripts/verify.ps1 -Stage <stage>` (fmt, locked
  clippy/tests/release build). Keep commands, exit codes and meaningful failure
  evidence in `docs/verification/`; update PLAN and operating documentation.
  Do not label pending cloud runs or unexercised scenarios PASS.
- Push only to the verified user Fork's non-release branch. No force pushes,
  history rewrites, release tags, Releases or upstream/tap writes.
- `TASK.md` stays unchanged. Platform change authorized on 2026-10-20 is recorded
  in PLAN section 9. Never lower a remaining gate to claim completion.

## Deployed product collaboration protocol

This section applies to clients connected to an intentionally deployed daemon.
The repository does not ship `.mcp.json`; `coord init` generates it for a target
project. Codex configures MCP through its own supported configuration.

1. Choose a stable handle such as `ledger-otter`, heartbeat every turn, and scan
   `tasks_list {state:"pending",priority:"urgent"}` and then high priority.
2. Before meaningful work: `tasks_send` (kind task/bug/feature), then
   `tasks_claim {id,agent_id,lease_seconds:600}`. If contested, pick other work.
3. Keep the returned ownership metadata. Renew before expiry, never after it.
   Completion and renewal require `agent_id` and `lease_token` from the claim;
   see `docs/API.md`. Never fetch a newer claim's
   token to submit work performed under an older claim.
4. Finish via `tasks_complete` with `post_ack:true`, `ack_name` and evidence.
   Include `agent_id` and `lease_token`. The transaction auto-injects `fixed_bug_id`; do not post a separate ack for
   work you performed. If blocked, explain the remaining work in the ledger.
5. Verification queries: `tasks_list {kind:"ack",limit:50}` and
   `tasks_list {state:"completed",limit:50}`, then `tasks_get` using the ack's
   `fixed_bug_id`. An empty pending queue is not evidence of inactivity.
6. Publish discoveries for other tabs as pending bug/feature/decision tasks;
   do not claim them unless taking responsibility for their implementation.
7. Watch with `coord wait --as ledger-otter --kind ack --timeout 600` or MCP
   long-poll. CLI claim/extend lease flag is `--lease 600`, not `--lease-seconds`.
8. Conclude with a short recap of changes, real task/ack IDs or bootstrap commits,
   verification state, and what the next tab should do.

Standalone ack publication is reserved for external events you did not perform;
include URLs/SHAs/context rather than inventing a source ID. HTTP RPC has no
identity authentication: use loopback and trusted local clients; lease ownership
is concurrency protection, not a security boundary between hostile tenants.

For generic batches, use `claim_task` to atomically receive the next task,
`submit_result` or `report_failure` with the returned fence, and `get_progress`
for a consistent snapshot. Empty does not mean terminal: tasks may be leased or
waiting for backoff. Only the manager's progress snapshot establishes completion.

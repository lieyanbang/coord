# Stage C — generic batches and scheduling

Starting SHA: `15f360a`. Windows; bootstrap Git ledger, TASK.md unchanged.

Added batches, batch-scoped idempotency keys, generic JSON inputs/results,
atomic claim-next, consistent progress, sticky cancellation, bounded exponential
retry and manager retry of failed tasks. Storage receipts fence/deduplicate
failure reports as well as completion. Six Agent MCP tools bridge to the same
store; manager APIs are JSON-RPC. Legacy single-task APIs remain fenced.
New columns and indexes migrate transactionally; no redundant ready_at column.

`cargo test --test batches --locked -- --nocapture`: exit 0, both tests passed.
Real daemon, four independent MCP bridge processes, 100 tasks: 99 unique
completions after one bridge exits holding a lease; force-restart the daemon,
reclaim the expired task, join a new bridge, complete the last task. Final
progress is exactly 100 total / 100 terminal / 100 completed.

The second E2E covers task-key replay/conflict, bounded execution failure,
duplicate failure receipts, manager retry, lifetime attempt monotonicity,
repeat cancellation, cancelled-batch enqueue rejection and stale submit refusal.
Renew/submit/get/progress all traverse real MCP/HTTP where applicable.

Initial regression exposed the newly introduced retry delay in old expiry tests;
their re-claim steps now wait beyond the configured 100ms backoff. Fencing
assertions remain strict; no skips or permitted stale result paths were added.
Administrative state changes also update the best-effort Vault mirror.

Full gate command: `pwsh -File scripts/verify.ps1 -Stage c`; individual commands,
exit codes and timings are in `stage-c-checks.json`. Gate is PASS only once all
four checks exit 0. All four exited 0: **Gate C PASS**. Cloud results are tied to the pushed SHA.

Previously verified cloud runs:
- Stage A `f846279`: [39430165894](https://github.com/lieyanbang/coord/actions/runs/39430165894), success.
- Stage B `15f360a`: [39431056581](https://github.com/lieyanbang/coord/actions/runs/39431056581), success.

Stage C cloud: `a83d50482f66cae2dcc5c05b1fb680d025deb128`,
[39432577257](https://github.com/lieyanbang/coord/actions/runs/39432577257), success.

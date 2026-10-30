# Using Coord

Connect to `coord mcp` (stdio). Claude Code uses `.mcp.json`; register the same
command in other clients' MCP settings. Default: `http://127.0.0.1:7777/`;
override with `COORD_URL` or `coord mcp --url URL`. Reuse the project's daemon,
or have the operator start `coord serve`. Keep RPC on trusted loopback.
Use the connected tools' schemas for arguments; Relay schemas may differ.

## Local tasks: choose an ID

1. Use a stable label in `agents_heartbeat {id, name}`. Find pending work with
   `tasks_list` (urgent/high first); `tasks_send {name, kind:"task"}` creates new work.
2. `tasks_claim {id, agent_id, lease_seconds:600}` claims that ID. Work only
   after success; keep its `lease_token`, `attempt_no` and `lease_until`.
3. Renew with `tasks_extend {id, agent_id, lease_token}` before expiry. Finish
   with `tasks_complete {id, agent_id, lease_token, result}`; add `post_ack:true,
   ack_name` for an atomic acknowledgement.
4. `tasks_get {id}` is authoritative. Discover completed work with `tasks_list
   {state:"completed"}` or `{kind:"ack"}`, following the ack's `fixed_bug_id`.

## Generic Batch: claim the next item

The manager creates/enqueues batches through HTTP management RPC. Use
`claim_task {agent_id, batch_id}`; each call can claim another ready item.
Omitting `batch_id` permits the global queue, including standalone tasks.
Use the returned fence for `submit_result {id, agent_id, lease_token, result}`
or `report_failure {id, agent_id, lease_token, error}`. Read `get_task {id}`
and `get_progress {batch_id}` for authoritative state.

## Trusted Worker Batch: Coord holds the token

`coord_claim {batch_id, worker_id}` returns this worker's live claim or the
next ready item from exactly that batch. Pass the returned `id` as `task_id`,
plus `batch_id, worker_id, attempt_no`, to `coord_progress` with a new checkpoint
(saves and renews), `coord_complete` with result, or `coord_release` with reason.
Coord manages tokens, sequences and request IDs. After reconnecting read
`coord_resume {batch_id, worker_id}`; it never claims or transfers work.
`coord_status {batch_id}` reads totals. Worker labels are trusted collaboration
labels, not separate authenticated identities.

## Progress, interruption and correctness

- Token-based checkpoints use `coord_report_progress` with the exact fence,
  `attempt_no`, sequence (start at 1, increment by one), UUID `request_id` and
  checkpoint. For `require_progress` batches renew with
  `coord_extend_lease_progress` using that fresh `progress_sequence`; otherwise
  use `extend_lease`. Heartbeats/checkpoints alone do not renew these leases.
- On lease loss stop writes/side effects. Resume only your same live fence;
  otherwise claim normally after reclaim/backoff. Never borrow a newer token.
  Before stopping, trusted workers release; token-only callers checkpoint and
  let expiry reclaim. Neither interruption consumes the failure budget.
- Execution errors use `report_failure`. Trusted-only workers checkpoint the
  error and release with a reason for manager review; release does not record
  a business failure. Never submit false success.
- Empty claims do not prove completion; read batch status. `submitted` awaits
  validation; read the eventual state/verdict.
- On uncertain writes read state first. Exact completion retries preserve the
  result/ack; changed content conflicts. Never blindly repeat claims or actions.
  External effects need their own idempotency key, even with Coord receipts.

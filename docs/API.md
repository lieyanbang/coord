# coord API and compatibility

Windows is the default supported platform. JSON-RPC 2.0 is served at POST `/`;
stdio MCP translates tools to the same daemon. Core A2A does not authenticate an
owner; keep it on loopback or behind an authenticated local proxy. New trusted
scheduler methods reject network peers that connect directly. Agent/worker IDs
are collaboration labels; they do not authenticate separate conversations.

## Fenced single-task lifecycle (Stage B)

`tasks/send`, `tasks/get`, filtered/long-poll `tasks/list`, announcement kinds,
heartbeats and watcher/ack discovery remain available. `tasks/list` accepts
`skip_reclaim: true` for read-only monitoring; it leaves expired leases for the
daemon's background reclaimer and does not change the default behavior.

| Operation | HTTP parameters | MCP tool / CLI |
| --- | --- | --- |
| tasks/claim | id, agentId, optional leaseSeconds | tasks_claim / claim ID --as AGENT --lease 600 |
| tasks/extend | id, agentId, leaseToken, optional leaseSeconds | tasks_extend / extend ID --as AGENT --token TOKEN --lease 600 |
| tasks/complete | id, agentId, leaseToken, result; optional postAck, ackName, ackPriority, ackPayload | tasks_complete / complete ID --as AGENT --token TOKEN --result JSON --ack NAME |
| tasks/cancel | id | tasks_cancel / cancel ID |
| tasks/reclaim | none | tasks_reclaim / reclaim |

MCP uses snake_case argument names (`agent_id`, `lease_token`, `lease_seconds`,
`post_ack`); HTTP legacy endpoints use the camelCase names above. HTTP accepts
`lease_token` as an alias. Claim returns the task including a fresh UUID token,
`attempt_no`, `attempt_count`, `max_attempts`, and `available_at`.

Claim, renew and completion use atomic conditional writes. Renew requires a
live lease and never shortens it. Complete requires the same agent and token
and database-time unexpired lease. Reclaim clears the fence and returns
interrupted work to pending with bounded backoff. Only a reported execution
failure or validator failure consumes the business failure budget (default 3).
Cancelled tasks cannot complete. Cancelling an already completed/failed task
does not rewrite its terminal outcome.

An accepted completion and optional ack/receipt commit together. Repeating the
same agent/token/result/ack request returns the accepted ack, including after
restart or the original deadline. This receipt replay does not create a new
transition. Changing the request is a conflict. Treat results as generic JSON;
external effects remain **at least once** and require business idempotency keys
or isolated workspaces. There is no exactly-once execution guarantee.

Errors: `-32602` invalid/missing parameters (upgrade tokenless clients), `-32601`
unknown method, `-32001` lease/state/conflicting-result conflict, `-32002` handler
limit (back off), `-32000` storage/internal error. Oversized bodies return HTTP
413. Browser Origin requests are rejected. Empty lists remain arrays.

## Database upgrade / rollback

Stop the operator-owned daemon and take a SQLite backup (or copy a quiescent DB
with its WAL/SHM handled correctly) before upgrade. Additive migrations are
transactional. Existing tasks, results, receipts, acks, and action records remain
readable. A claimed row without a lease token is invalidated back to pending; a
fenced live claim is retained and backfilled into `task_attempts` with its
checkpoint when one exists. Migration initializes the new `failure_count`
conservatively: claimed/submitted tasks use `max(attempt_count - 1, 0)` because
their current attempt has not yet failed; other states retain `attempt_count`.
Older databases do not distinguish business failures from lease interruptions,
so this preserves the old retry budget rather than trying to reconstruct
unknown history. New interruptions after migration do not consume that budget.

Legacy claim/write methods still require their lease token. The separate trusted
scheduler methods keep that token inside Core; direct Core calls are loopback
only, while remote clients use the authenticated Relay.

Rollback means stop the upgraded daemon, restore the pre-upgrade backup and run
the preserved old binary. Do not open the only upgraded database with an older
binary, and do not assume additive columns make old unsafe writers compatible.
Vault files are best-effort; new UUID-based filenames leave old notes intact.

## Local health probe

The operator-only `health` CLI command checks a valid JSON-RPC `tasks/list`
response and opens the configured SQLite database read-only. It queries at most
one task, discards the response, and stores no task payload or lease token.
`tasks/list` accepts `skip_reclaim: true` for this probe so expired leases remain
the responsibility of the daemon's background reclaimer. The status file records
READY, DEGRADED, or FAIL, latency, check names, consecutive failures, and the
last READY timestamp; critical failures return exit code 1.

```powershell
coord.exe --url http://127.0.0.1:7777/ health `
  --db C:\ProgramData\coord\data\coord.db `
  --status C:\ProgramData\coord\health\status.json
```

On Windows, `scripts/windows/coord-health-monitor.ps1` is intended for a
SYSTEM-owned one-minute Scheduled Task. It requests only a targeted `coord`
service start/restart after three consecutive failed probes, with a ten-minute
cooldown and a three-restarts-per-hour cap. Successful but slow checks become
DEGRADED and do not trigger recovery.

## Batches and scheduling (Stage C)

All new management/claim/failure methods use snake_case JSON parameters.

| HTTP method | Parameters | Response |
| --- | --- | --- |
| batches/create | name, optional description/config | batch object including id |
| batches/add_tasks | batch_id, tasks:[{key,name,payload}] | tasks array in request order |
| batches/status | batch_id | total, active, terminal, per-state counts and status |
| batches/retry_failed | batch_id | retried count |
| batches/cancel | batch_id | ok |
| scheduler/claim | agent_id, optional batch_id/lease_seconds | {empty,task}; empty queue has task:null |
| scheduler/failure | id, agent_id, lease_token, error | ok; exact accepted duplicates replay |
| scheduler/submit | same as tasks/complete | same as tasks/complete |
| scheduler/extend | same as tasks/extend | same as tasks/extend |

Agent MCP tools: `claim_task`, `submit_result`, `report_failure`, `extend_lease`,
`get_task`, `get_progress`. The original `tasks_*` tools remain. Management is
available via HTTP; an IDE runtime does not need to implement manager tools.

Batch config defaults: max_attempts=3, retry_backoff_ms=100, max_backoff_ms=30000.
Budgets are 1..100 failures and delay capped at one hour. `attempt_no` is the
lifetime-monotonic execution generation; `attempt_count` remains the historical
number of claims for compatibility. `failure_count` alone consumes the business
retry budget. On migrated databases, historical interruption/failure events
cannot be separated and are mapped conservatively from `attempt_count` as
described above. Release and expiry increment a separate capped interruption streak
and never consume `failure_count`. Eight consecutive interruptions set
`needs_attention`; the existing capped backoff prevents a hot loop. Explicit
manager retry preserves attempt history and failure count, granting one extra
failure slot only when the business failure budget was exhausted.

Enqueue is atomic, at most 1000 entries/request, keys 1..256 UTF-8 bytes. Reusing
the same key and input returns its existing task; changed input conflicts and
rolls back the entire request. Keys are unique within a batch. Batch progress
is a transaction-consistent snapshot. A drained batch reports completed; adding
more work to an active, drained batch is permitted. Cancellation is sticky,
blocks enqueue/claim/retry and invalidates active task fences. Terminal outcomes
remain counted; repeated cancel/retry is state-idempotent. Missing batch is
`-32004`. Legacy individual tasks have batch_id=null and still work.

## Optional asynchronous validation (Stage D)

Add `config.validator` when creating a batch to enable the built-in JSON
Validator. Supported rules: `required` (top-level keys), optional `equals`
(whole JSON equality). Omitting/null validator keeps direct completion.
The `Validator` trait is independent of MCP; this implementation does not run
external validator commands or LLM judges. Unknown config keys are rejected.

Config defaults: delay_ms=0, timeout_ms=1000, max_error_attempts=3. Timeout must
be 1..10000ms, delay <=10000ms, error budget 1..10, required keys <=128.
Delay is an optional bounded pacing/fault-test setting. Rule mismatch is FAIL;
timeout/interrupted validator lease is ERROR. No implicit command execution.

Submission atomically writes result + receipt and returns
`{ok:true,state:"submitted",submission_id}` quickly. It releases the execution
lease; agents can immediately claim more work. Four independent background
slots consume the durable submitted queue. Each job has its own expiring fence;
stale job tokens/submission IDs cannot update cancelled or newer work.

PASS completes and writes an optional requested ack in the same transaction.
FAIL backs off execution or fails on exhausted execution attempts. ERROR retries
validation with a separate bounded counter, then fails with validation_error;
it does not consume execution attempts. Abandoned validation leases count as
validation errors when recovered. Restart recovery waits for that old bounded
job lease to expire (timeout + 5 seconds), then safely reclaims it.
`validation_attempts`/`validation_error_count` describe the current submission;
`attempt_no`/`attempt_count` describe execution. Submission receipt replay always
returns the original acceptance response, even after its eventual verdict;
use get_task/get_progress for current state. Completion alone implies result
submission; business correctness is asserted only by configured checks.

## Reference Worker

`scripts/worker.py` (Python 3.10+, standard library) is an optional runner:

```powershell
python scripts/worker.py --batch BATCH_UUID --agent worker-one
python scripts/worker.py --batch BATCH_UUID --agent worker-two --workdir C:\isolated-work --timeout 60 --adapter python C:\adapters\execute.py
```

Default adapter echoes payload. Explicit external adapters read the full JSON
task from stdin and return one JSON result on stdout. They require a trusted
configuration and explicit workdir; no shell expansion. Both output streams
are bounded at 1 MiB, execution has a timeout, and Windows Job Objects contain
the process tree on worker crash. Lease renewal failure aborts the adapter and
suppresses stale submission. Empty/reconnect loops back off; receipt retries
reuse the same request. Ctrl+C stops the worker and its adapter. Workers exit
when the batch has no active tasks; start/join more workers when adding work.
The example does not imply every IDE MCP client supports unattended execution.

## Agent Progress (Phase G)

Batches may set `require_progress: true` and
`agent_lease_budget_seconds` (default 3600, maximum 86400). A claim remains at
most 600 seconds. The worker reports `scheduler/progress` with the exact task,
agent, lease token, attempt number and next sequence, then extends via
`scheduler/extend_progress` using that unconsumed sequence. Checkpoints are
limited to 1024 bytes and accepted for an extension only while no older than
600 seconds. One sequence can authorize at most one extension. The cumulative
budget is measured from the current claim's `claimed_at`; a later retry is a
new attempt with a new budget window.

Progress is self-reported text, not a liveness proof and not a background
timer. It cannot establish that a ChatGPT conversation will wake later. A
progress-required batch rejects the legacy `scheduler/extend` method. The
read-only `scheduler/agent_status` response contains current claims and recent
checkpoints but never lease tokens. A timeout or uncertain write must be
reconciled by reading the exact task and attempt; do not replay side effects.

## Trusted scheduler API (Phase R)

The trusted APIs keep the Core lease token private and allocate progress
sequence/request IDs inside the daemon. They still carry the returned task ID
and integer `attempt_no` on writes so an old conversation cannot write into a
new generation that uses the same worker label.

| Method | Parameters | Result |
| --- | --- | --- |
| `scheduler/trusted_claim` | `batch_id`, `worker_id`, optional `lease_seconds` | Current live claim for that worker or one ready task from exactly this batch; empty queue returns `{state:"empty"}`. |
| `scheduler/trusted_progress` | `task_id`, `batch_id`, `worker_id`, `attempt_no`, `checkpoint`, optional `lease_seconds` | Durable checkpoint, generated sequence, accepted time and lease deadline. Identical latest checkpoint is idempotent. |
| `scheduler/trusted_complete` | `task_id`, `batch_id`, `worker_id`, `attempt_no`, `result`, optional `ack` | `submitted` or `completed`; an exact retry returns the current task state and stored submission ID without re-enqueueing completion actions; changed content conflicts. |
| `scheduler/trusted_release` | `task_id`, `batch_id`, `worker_id`, `attempt_no`, optional `reason` | Requeues only the exact live attempt with interruption backoff and no business failure. |
| `scheduler/trusted_resume` | `batch_id`, `worker_id` | Read-only `active`, `released`, `expired`, `reassigned`, terminal state, or `none`, with latest durable checkpoint when available. |
| `scheduler/trusted_status` | `batch_id` | Batch state totals plus distinct live `active_claims`, interruption alerts and total business `failure_count`. |

These six methods are exposed as `coord_claim`, `coord_progress`,
`coord_complete`, `coord_release`, `coord_resume`, and `coord_status` by the
Coord MCP Relay. Relay OAuth/Cloudflare Access authenticates the owner. The
default trusted-owner path uses the private local Relay bearer and authenticated
connector; a v2 logical identity policy additionally signs and restricts each
call to a batch, worker label, operation scopes and expiry. Set
`worker_ids: ["*"]` for any four-hex collaboration label within that exact
owner/batch/scope/expiry binding, or keep an explicit worker list for a stricter
profile. The default trusted owner path does not treat the worker label as a
credential. Direct Core trusted RPCs are loopback-only; a remote MCP client must
use the authenticated Relay/proxy.

`resume` never reclaims or transfers work. If the stored lease has expired but
the background reclaimer has not run yet, it reports `expired` without changing
the task. A later `claim` performs normal expiry reconciliation before claiming.
Completion and acknowledgement still pass through the existing atomic receipt,
validator and Completion Action outbox; external effects remain at least once.

## Trusted Completion Actions (Phase G)

Completion actions are disabled unless the operator starts the daemon with a
trusted JSON registry:

```powershell
$env:COORD_ACTIONS_CONFIG = 'C:\ProgramData\coord\actions\registry.json'
coord serve --actions-config $env:COORD_ACTIONS_CONFIG
```

Only registry profiles can select `on_task_completed` or
`on_batch_completed`. `batches/create.config.action_profile` selects a profile
ID; the daemon pins its version and action policy in the batch. A profile must
be changed under a new version/ID. Agent payloads cannot supply executables,
working directories, arguments, shell fragments or registry changes. Arguments
are static strings with only `{event_id}`, `{task_id}`, `{batch_id}`,
`{action_kind}` and `{attempt_no}` placeholders; process creation uses
`shell=false`. Results are passed on stdin only when the trusted profile sets
`include_result: true`. Known token/secret/password/credential fields are
removed recursively first. Process output is drained, bounded, discarded and
never written to logs.

The Windows executor accepts only regular native `.exe`/`.com` files and a
working directory canonicalized beneath the registry's `allowed_root`. It uses
at most two concurrent actions. Each action is limited to 1–300 seconds, 64 KiB
combined stdout/stderr, 4 MiB event input, eight processes and 256 MiB Job
Object memory. The child starts suspended, is placed in its Job Object, then
resumes; descendants are killed on timeout, output overflow or worker exit.
Production execution requires `external_runner` with a dedicated independent
standard Windows account and verified ACLs, plus SHA-256 pins on every command.
The daemon runs only the authenticated queue broker and never spawns an action
in that mode. Without an execution mode it explicitly logs queue-only operation;
`isolated_in_process: true` is reserved for disposable regression tests.

The external configuration contains `broker_addr` (loopback only), `key_file`
(32 random bytes as 64 hex characters), `runner_sid`, `daemon_sid` and a separate
administrator-protected `daemon_rpc_key_file`. The latter enables bearer
authentication on all daemon RPC POSTs; the runner cannot read that key, and its
broker key cannot call management RPC. CLI/MCP can use `COORD_RPC_AUTH_KEY` for
loopback HTTP. Existing deployments without external mode retain their current
trusted-localhost behavior. Actual production identity/ACL/RPC-denial acceptance
is required before activation; see [ACTION_RUNNER.md](ACTION_RUNNER.md).

Completion and the outbox row commit in one SQLite transaction, both for direct
completion and validator `PASS`. Task actions are queued once per task/profile
version. The batch action is queued once only when every task is `completed`;
failed, cancelled or merely submitted work does not qualify. Once that batch
event exists, adding new tasks is rejected. Action state is independent from
task state and can be read through loopback JSON-RPC:

```json
{"jsonrpc":"2.0","id":1,"method":"actions/status","params":{"batch_id":"BATCH_UUID"}}
```

The response contains bounded state/attempt/error-code/exit-code/byte-count
metadata, never the event body or lease token. It is intentionally not included
in the Relay agent RPC whitelist. Action attempts have their own owner/token
fence and a 360-second lease. Idempotent actions may retry up to the trusted
profile's limit (1–5); non-idempotent profiles are limited to one attempt. If a
worker disappears after an external side effect may have happened, a
non-idempotent action becomes `uncertain`, not `succeeded`; idempotent replay is
at-least-once, not exactly-once. Action failure never reopens a completed task.
Batch actions wait for task actions to succeed. The queue-only broker exposes
`POST /claim`, `/authorize` and `/finish` behind its independent bearer secret;
these are service endpoints, not MCP tools or generic JSON-RPC methods.
Authorization reconstructs the stored event and rechecks current action fence,
completed task attempt and matching accepted-result receipt immediately before
spawn. Its publisher context contains only group/ID input fields and known
secrets are redacted. Native publication uses the accepted inline database
result rather than an Agent-selected staged path, atomically refuses conflicting
existing results, and writes receipt-checked preliminary indexes.

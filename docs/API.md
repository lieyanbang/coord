# coord API and compatibility

Windows is the default supported platform. JSON-RPC 2.0 is served at POST `/`;
stdio MCP translates tools to the same daemon. Core A2A does not authenticate an
owner; keep it on loopback or behind an authenticated local proxy. New trusted
scheduler methods reject network peers that connect directly. Agent/worker IDs
are collaboration labels; they do not authenticate separate conversations.

## Default MCP workflows

For local use, start `coord serve` and register `coord mcp` in your MCP client.
The bridge connects to `http://127.0.0.1:7777/`; override with `COORD_URL` or
`coord mcp --url URL`. No Relay, OAuth or Action Runner is required. Batch
creation/enqueue remains a manager operation over HTTP JSON-RPC.

| Scenario | Local MCP claim | HTTP method | Ownership and scope |
| --- | --- | --- | --- |
| Select one task by ID | `tasks_claim {id, agent_id, lease_seconds?}` | `tasks/claim` | Caller keeps the returned token; claims exactly that ID. |
| Receive next ready generic task | `claim_task {agent_id, batch_id?, lease_seconds?}` | `scheduler/claim` | Caller keeps the token; explicit batch restricts selection, omitted batch permits the global queue (including standalone tasks). Repeated calls can claim additional work. |
| Trusted Worker Batch | `coord_claim {batch_id, worker_id, lease_seconds?}` | `scheduler/trusted_claim` | Core keeps the token; returns the worker's live claim or one ready task from exactly that batch. |

Local tools use snake_case arguments. A claim returns a task with `id` and
`attempt_no`; trusted writes call that ID `task_id`. For ID-selected tasks use
`tasks_send` only for new work, `tasks_list` to find work, `tasks_complete` to
finish, and `tasks_get` to read state. Add `post_ack:true, ack_name` to completion
when watchers need an atomic acknowledgement; discover it with
`tasks_list {kind:"ack"}` and follow `fixed_bug_id` to the source task.

Generic workers use `submit_result {id, agent_id, lease_token, result}` or
`report_failure {id, agent_id, lease_token, error}`. Read `get_task {id}` for
one task and `get_progress {batch_id}` for a consistent batch snapshot. Trusted
workers use the six `coord_*` tools in [Trusted scheduler API](#trusted-scheduler-api-phase-r).
An empty claim is not terminal: other tasks may be leased, submitted or in
backoff. Only authoritative batch status establishes whether all work is done;
failed/cancelled work is not a successful completion.

**Remote Relay names are distinct.** `coord_claim_task` selects an explicit ID;
`coord_claim_next` requires an allowed `batch_id` and never draws from the global
queue. Both use the Relay-authenticated agent identity and require a UUID
`request_id`; they are token-based calls, separate from trusted `coord_claim`.
Legacy Relay writes such as `coord_submit_result` also require `request_id`.
The local `claim_task`/`submit_result` schemas do not take that Relay parameter.
Use the connected tool schema: logical Relay mode additionally requires its
authorized `batch_id` and `worker_id` on worker calls. Remote read tools redact
tokens; a worker label is not a credential or independent conversation identity.

## Progress, failure and recovery

- Token-based callers retain the exact `id`, `agent_id`, `lease_token`,
  `attempt_no` and deadline from their own claim. Renew using `tasks_extend` or
  `extend_lease {id, agent_id, lease_token, lease_seconds}` before expiry.
  `agents_heartbeat {id, name}` records presence only.
- To persist a token-based checkpoint, call `coord_report_progress` with
  `id, agent_id, lease_token, attempt_no, sequence, request_id, checkpoint`.
  Start sequence at 1 and increment by one for each checkpoint on that attempt;
  use a fresh UUID request ID for each new operation. This alone does not renew
  the lease. For `require_progress` batches follow it with
  `coord_extend_lease_progress {id, agent_id, lease_token, attempt_no,
  progress_sequence, request_id, lease_seconds}`, using that fresh sequence and
  a separate request ID. Ordinary `extend_lease` is rejected for these batches.
  `coord_agent_status {agent_id, batch_id}` reads claims/checkpoints without tokens.
- Trusted workers pass `task_id, batch_id, worker_id, attempt_no` on writes.
  `coord_progress` with a new checkpoint saves progress and renews the lease;
  repeating the identical latest checkpoint replays its prior response rather
  than renewing again. Core supplies request IDs and sequences. On reconnect,
  read `coord_resume {batch_id, worker_id}` and continue only an `active`
  attempt; released, expired or reassigned work must go through normal claim.
- On lease loss stop writes and external effects. Never read a newer claim's
  token to submit work from an older attempt. Token-based clients can continue
  only if they retained their exact still-live fence; otherwise allow normal
  reclaim/claim. When stopping voluntarily, trusted workers use `coord_release`
  with a reason; token-only callers save a checkpoint and let the lease expire.
  Neither interruption path consumes the business failure budget.
- A real execution failure uses token-fenced `report_failure` and bounded
  retry/backoff. The six trusted tools have no business-failure shortcut:
  trusted-only workers checkpoint the error and release with a reason for
  manager review. This does not count a business failure; do not submit a
  success result to hide the error. A configured validator can also reject a
  submitted result.
- After an uncertain write, reconcile task/attempt state and receipts before
  retrying. An exact completion replay preserves the accepted result and ack;
  changed content conflicts. Preserve request IDs and content when reconciling
  progress receipts. Do not blindly repeat claim-next or external effects.
  `submitted` releases the execution lease but awaits validation; read current
  task/progress for the verdict. External effects require business idempotency.

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
`coord_complete`, `coord_release`, `coord_resume`, and `coord_status` by both
local `coord mcp` and the optional Coord MCP Relay. The local tools use the
parameters above; the Relay also accepts `worker_id` on status and requires it
in logical mode. Relay OAuth/Cloudflare Access authenticates the owner. The
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

The signed external configuration contains `broker_addr` (loopback only),
`runner_sid`, `daemon_sid`, `runner_key_id`, a Core-readable
`runner_public_key_file`, and a separate administrator-protected
`management_rpc_key_file` at the registry top level. Legacy registries may
still place this management path at `external_runner.daemon_rpc_key_file`; Core
accepts it only as a migration fallback. New Runner configurations omit this
field. A configured external Runner without a separate management key path is
rejected before the RPC listener starts.
The Runner private key is passed only to the dedicated Runner process and
must be protected so that Core cannot read it. A compatibility registry may
keep `management_rpc_key_file` while setting `external_runner` to `null`; the
Scheduler remains authenticated and reports the Broker as `UNAVAILABLE`.
The old
`key_file` field may appear in a legacy registry; Core treats it as opaque and
never opens it. Management RPC keeps its independent bearer key, which the
Runner cannot read. CLI/MCP can use `COORD_RPC_AUTH_KEY` for loopback HTTP.
Existing deployments without external mode retain their current
trusted-localhost behavior.

Runner requests use Ed25519 signatures with no Bearer fallback. The canonical
UTF-8 envelope is the following fields joined by LF, in order: protocol label
`coord-runner-request-v1`, key ID, configured Runner SID, uppercase method,
exact origin-form path, decimal Unix timestamp in milliseconds, base64url
nonce, and lowercase SHA-256 of the exact body bytes. The nonce is 16 bytes;
the signature is canonical base64url Ed25519. Only `POST /claim`,
`POST /authorize` and `POST /finish` are accepted. Query strings are rejected,
the clock window is 30 seconds, the signed body is limited to 4 KiB, and an
accepted `(key ID, nonce)` is recorded transactionally in SQLite so replay is
rejected after a Core restart. Existing action ID, owner, lease-token and
expiry fencing remains in force. An invalid signature from the configured key
identity marks the Runner component `SECURITY_ERROR`; a later valid signed
request clears that state.

`GET /healthz` reports overall readiness plus `core`, `database`, `broker` and
`runner` components. Overall status is `READY` only when all components are
ready; Scheduler/RPC/SQLite availability with Broker or Runner degradation is
`DEGRADED`. The `coord health` command still publishes its compatible
`tasks_list` and `sqlite_read_only` checks and uses those checks to distinguish
Core/DB failure from Broker/Runner degradation. Broker setup and listener
failures do not stop Scheduler RPC; management-key or Core identity failures
remain fatal because they would invalidate the RPC authentication boundary.
Actual Windows service identity and ACL acceptance is required before
activation; see [G7 deployment](phase-g/G7-DEPLOYMENT.md).

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
`POST /claim`, `/authorize` and `/finish` behind Runner public-key verification;
these are service endpoints, not MCP tools or generic JSON-RPC methods.
Authorization reconstructs the stored event and rechecks current action fence,
completed task attempt and matching accepted-result receipt immediately before
spawn. Its publisher context contains only group/ID input fields and known
secrets are redacted. Native publication uses the accepted inline database
result rather than an Agent-selected staged path, atomically refuses conflicting
existing results, and writes receipt-checked preliminary indexes.

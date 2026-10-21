# coord API and compatibility

Windows is the default supported platform. JSON-RPC 2.0 is served at POST `/`;
stdio MCP translates tools to the same daemon. No identity authentication is
provided: agent IDs/tokens fence concurrent attempts, not hostile tenants.

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
and database-time unexpired lease. Reclaim clears the fence and returns work
to pending, or failed once its finite attempt budget (default 3) is exhausted.
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
transactional. Existing valid tasks, agents and acks remain readable. Old active
claims lack a token and are invalidated back to pending; clients must claim
again. New claims cannot use the old tokenless write path.

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
Budgets are 1..100 attempts and delay capped at one hour. On execution failure
or lease expiry, attempt n backs off by min(base*2^(n-1),cap); once exhausted,
the task fails. `attempt_no` is lifetime-monotonic even after explicit manager
retry; `attempt_count` is the current budget. Expired attempts cannot submit.
Failure replays cannot consume the retry budget twice; conflicting errors fail.

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

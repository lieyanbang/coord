# coord API and compatibility

Windows is the default supported platform. JSON-RPC 2.0 is served at POST `/`;
stdio MCP translates tools to the same daemon. No identity authentication is
provided: agent IDs/tokens fence concurrent attempts, not hostile tenants.

## Fenced single-task lifecycle (Stage B)

`tasks/send`, `tasks/get`, filtered/long-poll `tasks/list`, announcement kinds,
heartbeats and watcher/ack discovery remain available.

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

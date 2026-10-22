# Phase G2: Lease Progress and Recovery

Result: **PARTIAL**. Fenced progress and finite per-attempt lease mechanics
pass isolated Windows tests; authenticated identity and unattended operation do
not.

The implementation exposes `scheduler/progress`,
`scheduler/extend_progress`, and `scheduler/agent_status` through Coord MCP and
the Relay allowlist. A progress-required batch rejects legacy `scheduler/extend`.
Reports must match the live task, agent, lease token, attempt and next sequence;
an extension consumes one fresh sequence. One lease is capped at 600 seconds,
progress freshness at 600 seconds, and the default cumulative budget is 3,600
seconds from the current attempt's `claimed_at` (a retry starts a new budget
window). Status omits lease tokens. Uncertain mutations are not automatically
redispatched by Relay.

`cargo test --all-features --locked` in the final Windows gate exited **0**;
`tests/agent_progress.rs` reports 3 passed, 0 failed. It covers stale attempt
and token, sequence replay, idempotent receipt, legacy-extension rejection,
600-second lease cap, finite budget and six simultaneous synthetic claims.

The checkpoint is self-reported text, not proof of model liveness or useful
work. ChatGPT Web background wake is not guaranteed, and this path has no
independent session identity or verified supervisor. Therefore it cannot
promise 24-hour unattended work; an idle worker must lose its lease and stale
results must remain unpublished. No production lease was created or modified.

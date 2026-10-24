# Phase G6-C — progress, renewal and disconnect recovery

Date: 2026-10-24 Australia/Brisbane. Result: **PARTIAL**. Fenced renewal and
recovery PASS in isolated Windows/core/Relay tests. The two-real-ChatGPT
claim → image → progress → extend → submit → next-claim gate is **NOT_RUN /
BLOCKED** on G6-A. No production lease or task was modified.

## Correctness fixes and regression evidence

Two accepted-receipt replay defects were reproduced before repair:
`scheduler/progress` could return an old accepted receipt when the supplied
attempt changed; `scheduler/extend_progress` could return an old accepted
receipt when the supplied owner or attempt changed. Both regression tests
failed in `stage-g6-c-receipt-before.log` (cargo exit 101, 0 passed/2 failed).

The minimal repair joins the stored progress receipt to `task_progress` and
checks the original owner and attempt before accepting a receipt replay. It
does not change the database schema or remove legitimate identical receipt
reconciliation. `cargo test --test agent_progress --locked -- --nocapture`
then exited 0: 6 passed, 0 failed, 0 ignored, in
`stage-g6-c-progress-after.log`. The suite additionally verifies a checkpoint
older than 600 seconds cannot authorize renewal, elapsed attempt budget cannot
be extended, disconnected/expired attempts are reclaimed, stale fencing cannot
renew or submit, and successful submission preserves its exact receipt and
allows the next claim. The six-agent concurrency test remains explicitly
synthetic, not ChatGPT identity evidence.

## End-to-end identity binding

In the enabled isolated-policy tests, all progress, extend, failure and submit
tools use the authenticated binding added by G6-A. Callers cannot supply a
different worker. Both Relay hops verify
the signed request and the core still enforces current agent, token, attempt,
state and unexpired lease. The Relay never automatically retries an uncertain
mutation. A same request UUID never redispatches; inspect the exact task and
attempt before deciding whether the original fenced receipt needs reconciliation.

`node --test relay/relay.test.mjs relay/live-coord.test.mjs` exited 0, **20/20**,
with raw output in `stage-g6-identity-relay-all.log`. The new real-daemon test
uses a private temporary database and synthetic OAuth contexts A0/A1. It
exercises:

- A0 claims and reports progress without changing `lease_until`; after expiry
  the task returns to pending and A1 claims a higher attempt with a new fence.
- A0's stale progress and A0 using A1's token cannot mutate the task.
- A1 reports a fresh checkpoint, extends once, and cannot reuse its consumed
  sequence. A valid submit is read back as completed; the same Relay request ID
  is rejected, while exact accepted receipt reconciliation under a fresh RPC ID
  returns the original acceptance without creating a new completion.
- A1 claims another task. A0's valid failure is accepted and its old result is
  rejected. Stopping the Relay Agent yields a confirmed not-dispatched read;
  the unrenewed task expires, reclaims and clears its lease token.

The protocol rejects lease requests above 600 seconds. The intended batch keeps
`max_attempts=3`, `require_progress=true`, default attempt budget 3,600 seconds
and progress freshness 600 seconds. Policy TTL and per-request signature TTL
are independent authentication bounds and do not extend any task lease.

## Real ChatGPT and platform limit

Two actual independent ChatGPT conversations attempted harmless read-only Coord
list probes during G6-A. Both initially showed "Unable to load this reply";
after a visible normal reconnection of the existing Primary account, the UI
claimed successful reads. The authoritative server audit at
2026-10-24T10:16:48Z confirms four `tasks/list` dispatches and four completed
results and no other methods in the observed window. It cannot attribute those
calls to individual conversations or distinguish their credentials. All 16
access-token records still referenced one client digest and no authenticated
conversation ID existed. The reconnection changed OAuth grant state; no real
conversation was allowed to claim a task. The
full claim/image/progress/extend/submit/next-claim cycle, browser-close expiry,
refresh/reconnect stability and six real protected identities remain
**NOT_RUN / UNVERIFIED**. See G6-A and
`stage-g6-browser-server-audit.json` for redacted server observations. Real
protected contexts unavailable to other conversations are the prerequisite;
this is a platform isolation limitation, not missing browser permission.

A checkpoint is self-reported work, not proof of liveness. Page presence,
socket existence or old chat text must not drive automatic renewal. There is
no verified background-wake contract for this ChatGPT path, so 24-hour
unattended operation is **UNVERIFIED / PLATFORM_LIMITATION**. If a conversation
stops, allow the finite lease to expire; a human may need to resume a fresh
turn. Do not compensate with unlimited leases, forged progress or silent
supervisor renewals. Follow `docs/phase-g/IDENTITY_BINDINGS.md` for the isolated
real-connection acceptance once G6-A's protected contexts are available.

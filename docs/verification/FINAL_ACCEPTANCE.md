# Final acceptance — Windows coord MVP

Result: **PARTIAL** pending the final Stage E Windows Actions run and executable
artifact verification. Local measurements and fault evidence are real; this
report will be closed only after those cloud checks complete.

Scope: Windows x86_64 MSVC, trusted local clients, built-in JSON validators and
the optional Python Worker. Linux/macOS are NOT_RUN, excluded from both build
matrices and not default supported (user override dated 2026-10-20). Original
MIT attribution is unchanged; no Release/tag or upstream write was performed.

## Coverage

| Requirement | Evidence | State |
| --- | --- | --- |
| Baseline and per-push Windows build/artifact | STAGE_0.md; real downloaded original CI artifact | PASS |
| Strict decoding, transactional migration, long-poll/Vault/access boundary | STAGE_A.md; quality and service tests | PASS |
| Live owner/token fencing, expiry, monotonic attempts, replay/ack atomicity | STAGE_B.md; multi-connection/process negative tests | PASS |
| Generic batches, dedupe, claim-next, bounded failure retry/cancel/progress | STAGE_C.md; 100-task real daemon/MCP E2E | PASS |
| Optional PASS/FAIL/ERROR validation, independent bounded recovery | STAGE_D.md; validation.rs, Stage E interruption tests | PASS |
| Trusted Worker adapter, timeout/output bounds, renewal, crash containment | STAGE_D.md; actual Python processes and Windows Job Object | PASS |
| 100/1,000/10,000 tasks, six independent MCP processes | STAGE_E.md; stage-e-capacity.json/log | PASS |
| Process kills, daemon restart, dropped response, stale/cancelled submissions | STAGE_E.md; stage-e-faults.log | PASS |
| Actual baseline DB upgrade plus restored-backup old-binary read | stage-e-migration.json; STAGE_E.md | PASS |
| Final local fmt/clippy/tests/release build | stage-e-checks.json; all four exits 0 | PASS |
| Final pushed implementation Windows Actions/artifact | closure evidence below | Pending |
| Linux/macOS build/runtime | intentionally excluded by latest instruction | NOT_RUN |

At 10,000 tasks the measured throughput is 215.01 tasks/s, claim P95 39.68ms,
daemon peak working set 21,114,880 bytes, zero busy/locked log lines. All sizes
have exactly one completion receipt per task and matching terminal totals.
These are bounded local capacity observations; no production throughput promise.

## Commits and cloud runs

All pushes target `lieyanbang/coord`, branch `codex/staged-scheduler`. The
upstream origin `DmarshalTU/coord` was not modified. No Git history rewriting.

| Stage | Commit | Windows Actions |
| --- | --- | --- |
| 0 | `572771a76a61851a2a11d32fea2a0a95441bf584` | [39428331459](https://github.com/lieyanbang/coord/actions/runs/39428331459) success |
| A | `f846279be435f96a032715c86593e66f7f17ab04` | [39430165894](https://github.com/lieyanbang/coord/actions/runs/39430165894) success |
| B | `15f360a30957eaa5c4607d2bb11ebdcd43646846` | [39431056581](https://github.com/lieyanbang/coord/actions/runs/39431056581) success |
| C | `a83d50482f66cae2dcc5c05b1fb680d025deb128` | [39432577257](https://github.com/lieyanbang/coord/actions/runs/39432577257) success |
| D | `0f4727f790916fc5cc42bd6be25bf64fbdc83157` | [39434225476](https://github.com/lieyanbang/coord/actions/runs/39434225476) success |
| E | pending local gates | pending |

## Compatibility and operating boundaries

Old valid rows, agents, ack links, Vault notes and single-task flows survive
migration. Old active tokenless claims are invalidated. Writers must retain
the owner and token returned by their own claim; old tokenless clients receive
an explicit error. An exact receipt replay returns the original acceptance,
not a new claim/current verdict. Back up and restore using the quiescent
DB/Vault procedure in STAGE_E.md; no in-place production upgrade was performed.

Scheduling is at least once. External file/API effects need business idempotency
or isolated workspaces; exactly-once side effects are not asserted. Empty queue
is not terminal; inspect consistent progress. A submitted result is not yet a
validation PASS. Validators check configured JSON rules, not arbitrary business
correctness. All IDE clients' unattended behavior is NOT_RUN; use an explicitly
configured runtime such as the reference Worker.

RPC is unauthenticated and intended for loopback/trusted local callers. Tokens
protect against stale concurrent attempts, not hostile tenants who can read
the API/database. External command validators, LLM judges, multi-tenant auth,
long-term production soak and real service power loss are NOT_RUN/out of scope.
Controlled process termination and restart are tested, not physical power-loss.

Dependency audit exits 0 with the documented RUSTSEC-2026-0189 exclusion for the
disabled rmcp HTTP transport. paste unmaintained and lru IterMut/panic-safety
warnings remain, as documented in STAGE_A.md and stage-e-audit.log. Do not claim
an all-clear audit or enable rmcp HTTP without revisiting this exclusion.

No shared coordination service was available during bootstrap; no task or ack
UUIDs were fabricated. The stage commits and verification documents form the
work ledger. TASK.md remains byte-for-byte unchanged and outside these commits.

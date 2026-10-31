# Coord acceptance

## Generic Worker refactor — 2026-10-31

Scope: the final root `PLAN.md` and `TASK.md` supplied in the original
`D:\project\coord` checkout, implemented from `e6deb80` in the isolated
`codex/generic-workers-20261031` branch. Those contracts and the original dirty
worktree were preserved. Historical Emote work remains outside this refactor.

The implementation provides generic string Worker IDs, scoped task discovery
and trusted business failure through both MCP transports, signed Relay, RPC and
the existing SQLite state machine. It also fixes public read credential
exposure, selected-batch lease budgets, rejected current results and terminal
batch outcome reporting. Existing profile-controlled Emote validation, successful
completion Actions, announcement semantics and Broker isolation remain enabled.
No additional scheduler, identity database or service was introduced.

| Boundary | Current evidence | Result |
| --- | --- | --- |
| Windows Rust regression | 113 passed; 8 ordinary-suite ignores include the separately exercised capacity test and helper child processes; fmt and locked Clippy with warnings denied | PASS |
| Current Windows Release | All four executables built from this implementation; no old binary substituted | PASS |
| Relay / real Core / independent HTTP MCP SDK | 29 tests; generic `writer-1`/`writer-2`, same-worker recovery, progress, completion, fail, release, restart, old fences, multiple allowed batches, forged/missing declarations and read-only expiry checks | PASS |
| MCP schema / OAuth / Access regression | 19 tests; old tools retained and new `coord_fail` published | PASS |
| Host OAuth implementation | Retrieved only the deployed host's OAuth source module; isolated DCR, S256 PKCE, resource/scope binding, owner JWT/JWKS, CSRF, one-time code, refresh and expiry tests | PASS, isolated |
| Historical compatibility | Newly built `e6deb80` executable created active four-hex and Emote claims; two SQLite online backup copies survived candidate upgrade and baseline rollback with identical task/attempt/checkpoint history; new failure records remained readable | PASS, synthetic copies |
| Optional business consumers | Emote/recovery, Actions/Runner, Vault/TUI, announcements and cancellation/failure tests remain in the Rust regression; failed terminal batches never emit the batch success Action | PASS, automated |
| Windows health monitor | Authentication, bounded recovery, Runner security errors and Broker failure isolation tested against isolated fixtures | PASS |
| Business request/result helpers | Existing Python suite, 8 tests | PASS |
| Capacity | Current Release, 100 / 1,000 / 10,000 tasks and six independent MCP processes; unique claims and receipts match, no SQLite busy errors; raw measurements in `generic-workers-capacity.json` | PASS |
| Real deployment inspection | Arasaka Core/Broker/Runner and Linux reverse Relay READY; Core/Broker listeners remain loopback; existing Feathers/OpenResty/OAuth chain retained | READ ONLY, 2026-10-31 05:13 UTC |
| Public OAuth | HTTPS discovery returned `coord` / `offline_access`, S256 metadata and expected unauthenticated 401 resource challenge | PASS, read only |
| Actual ChatGPT Web | Logged-in Work UI lists the installed Coord plugin; deployed MCP source still has the four-hex schema and no trusted fail tool | Discovery only |
| New actual Web read/write flow | Requires a publicly reachable isolated candidate endpoint and its approved tool snapshot; the existing URL serves the previous production version | EXTERNAL DEPENDENCY |
| Production / formal Emote Batch | No deployment, task/Action write, binding/permission change or production migration performed | NOT RUN |
| Hosted low-privilege Runner rehearsal | CI-only existing Windows workflow; authoritative result and matched artifact belong to the delivered commit's Fork checks | See commit checks |

The Core schema remains backward compatible. Worker IDs use the bounded ASCII
contract in `docs/API.md`; existing four-hex ownership is recovered without
renaming persisted identities, and ambiguous historical aliases fail closed.
Scoped lists require an authorized single batch and `coord:read`, filter before
LIMIT in SQLite, and undergo an independent returned-scope check in the Agent.

Relay journal compaction retains recent exact records and a bounded 8 MiB
conservative retired-ID filter. Retired requests, including uncertain writes,
are rejected for authoritative reconciliation rather than replayed. Collisions
can conservatively reject new IDs. Preserve both journal files together; a
compacted journal intentionally fails closed with older Relay binaries, so
Relay rollback needs a compatible forward fix. Core rollback was exercised.

Full local logs remain in the isolated checkout's `docs/verification/` and
`target/`; GitHub Actions reruns the checked-in Windows regression, health
monitor, low-privilege Runner, Relay/MCP and Python checks on the delivered
commit. Simulated OAuth/SDK tests do not establish actual ChatGPT Web writes or
production acceptance. No Release, tag or upstream write is part of delivery.

## Historical Windows MVP acceptance — 2026-10-21

Result: **PASS** for the scoped Windows MVP (2026-10-21). Stages 0/A/B/C/D/E
passed their local gates and real Windows Actions runs. The final Stage E
artifact was downloaded, executed and used for additional functional acceptance.

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
| Final pushed implementation Windows Actions/artifact | stage-e-actions.json; downloaded exe functional acceptance below | PASS |
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
| E | `d4f41043c75a811ab756e2bc65a816eefef823a8` | [39436136707](https://github.com/lieyanbang/coord/actions/runs/39436136707) success |

Final implementation run: fmt, strict locked clippy, locked all-feature tests,
locked release build and artifact upload all completed success. The separate
closure commit only records the observed evidence; it does not rewrite a stage.
Artifact `coord-windows-2022`, ID `12272506068`, archive 4,199,540 bytes;
metadata and every job/step conclusion are preserved in `stage-e-actions.json`.

- ZIP SHA256: `31582c4e5d8cfc85658808ad3d9222f581d31cc3eb92904b5c01bd448457f675`.
- EXE SHA256: `6884df79dff810546db84cace765991b9ebec168701136ce0fa15fe51c43d3f6`.
- Downloaded exe `version` exited 0. With `COORD_TEST_BIN` set to that exe,
  faults/validation/Worker acceptance passed all 9 cases (exit 0,
  `stage-e-artifact-faults.log`). Actual baseline migration/backup rollback
  passed again (`stage-e-artifact-migration.json`, exit 0).
- The compiled capacity harness was rerun with the downloaded exe from a
  separate report directory: all three sizes passed, 100/1,000/10,000 unique
  completions and equal receipt counts (`stage-e-artifact-capacity.json/log`,
  exit 0). Its 10,000 tasks took 46.1853s (216.52 tasks/s), claim P95 37.74ms,
  daemon peak WS 23,060,480 bytes, zero busy/locked log lines. This independent
  repeat overlapped the artifact fault tests and is not a controlled benchmark.

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

## Phase G6 extension (2026-10-24)

The MVP PASS above remains scoped to its original trusted-local-client
contract. Phase G6 is **PARTIAL**, not production acceptance. Authenticated
connector bindings and an independent low-privilege native Action Runner are
implemented with isolated regression coverage; external-runner mode also
requires an independent management bearer on the main RPC. Default legacy RPC
still assumes trusted loopback callers.

Real ChatGPT per-conversation credential isolation remains **BLOCKED**. Two
real read-only probe chats and server logs confirm calls but do not prove
independent protected credentials. Real Arasaka Runner account/ACL validation
and the two-chat write/renewal cycle are **NOT_RUN**. Production deployment,
canary, formal emote task creation and classification Agent startup were not
performed. F3-R remains **PARTIAL** despite current CSP/hash and token-exchange
evidence. Full local, Fork Actions, downloaded-artifact and remaining-gate
details are in `PHASE_G6_A.md` through `PHASE_G6_D.md`; deployment and rollback
conditions are in `../phase-g/G6-DEPLOYMENT.md`.

The 2026-10-25 evidence closeout verifies Fork Windows run
[40088013915](https://github.com/lieyanbang/coord/actions/runs/40088013915)
on implementation commit `7d08012a171f804edf3b8588b77c72720e76393d`: all steps
succeeded, including actual dedicated-standard-user identity, ACL/RPC denials,
native publish/index, runner-crash Job cleanup, `uncertain` recovery without
replay and disposable account/directory removal. Both downloaded artifact ZIP
hashes match GitHub metadata. The three downloaded executable entry points and
27 focused integration tests pass; all three executable hashes differ from
the local build, so reproducibility is not claimed. The final local eight-check
`g6-close` gate also exits 0. These results close the isolated build/runner
mechanism gates; the real ChatGPT, real Arasaka account/ACL, F3-R and deployment
boundaries above remain unchanged. The first two cloud failures are retained
in G6-D rather than relabelled PASS.

## Phase G7 extension (2026-10-25)

**Controlled deployment, automated/site and real two-ChatGPT synthetic
acceptance PASS within the authorized trusted-owner G7 scope.** The user supersedes G6's
protected-conversation identity gate with trusted logical worker declarations
0000–0005 under the existing authenticated OAuth owner. The same owner can
impersonate these labels, and ChatGPT cannot autonomously wake; neither is
claimed as an implemented security/autonomy feature.

Implementation `f0aa7daed53a36f8c0c2418d935858bbf9f9fd9e`, Fork Windows run
[40167256908](https://github.com/lieyanbang/coord/actions/runs/40167256908), all
steps success. Three downloaded executables and both ZIP hashes were verified;
28 focused executable integrations and 25 Relay tests pass. The corrected
signed protocol tolerates at most five seconds future issuance skew between
hosts while preserving hard expiry and 60-second signed lifetime. No reproducible
build claim: executable hashes differ from the initial CI and local builds.

Actual Arasaka Runner standard-user SID, different from the daemon, NTFS/RPC
denials, SCM privilege restriction, SYSTEM authenticated READY, native publish/
index, Runner crash/Job cleanup/uncertain without replay and copied-DB old-binary
readback pass. Both remote restore points passed before replacement. Deployment
uses the verified CI executables, protected management/broker/signing keys and
fixed publisher profiles; Access policy and production oauth.cjs are unchanged.

Production operator synthetic Canary has 10/10 completed tasks and 11/11 actions
succeeded. Six unique claims, progress/renewal, bounded expiry/failure recovery,
stale/foreign tokens, unsigned/tampered requests, request replay, final index and
receipt outcomes were checked. Original eight tasks retain all 27 old fields;
unrelated services keep their identity/start mode/PIDs. 13011/13012, public
OAuth metadata/challenges and the actual installed legacy OAuth tool pass.
The final downloaded release explicitly passes 100/1,000/10,000-task capacity:
10,000 takes 98.90s, claim P95 42.875ms, zero busy log lines, exact unique
completion/receipt/terminal counts. This is local measurement, not production
performance or long-duration soak evidence.

Browser control recovered and the exact reconnect client binding was repaired.
Both original ChatGPT conversations completed two synthetic tasks each, with
actual marker inspection, progress, renewal, submit, exact readback and repeated
claim: 4/4 completed, active 0, 5/5 actions succeeded. SQLite receipt/progress
records, published-result equality and all index hashes pass. B's initial final
reply failed to render; a read-only follow-up confirmed its two exact completed
results without replaying writes. All eight g7-reconnect checks and final
production regression pass. See G7_DISCOVERY_REPAIR.md for exact IDs/evidence.
Current binding expiry is 2026-10-26T05:45:23.904Z. Operational recovery is in
`../phase-g/G7-DEPLOYMENT.md`. F3-R's existing non-owner/protocol evidence gaps
remain independently PARTIAL. No formal 121-task batch, classification agents,
old-task closure, Access-policy change, release/tag, history rewrite or machine
restart occurred. An accidental push to origin was denied; the verified Fork
working branch was then pushed normally. Full evidence and preserved failures
are in `PHASE_G7.md`.


## Formal initial Batch creation (2026-10-25)

**Creation/authorization PASS; classification NOT_RUN.** Explicit follow-on
authorization creates Batch `90ce2ca7-184e-4c3d-9d87-56386db25419`, 121 unique
group keys and 1,206 unique images. All tasks pending, zero claims/attempts,
zero formal action events/results. Six actual MCP status calls return
active_claims=0. Raw manager active=121 counts all nonterminal work, including
pending. No claim was dispatched; claim permissions were checked separately.

All 1,673 deployed input hashes and ACLs, actual non-admin Runner/RPC denials,
fixed native profile, old 22-task/progress/receipt/action preservation, health,
protected services and public routes pass. Server-UTC binding is valid exactly
2026-10-25T07:58:44.865Z–2026-10-25T19:58:44.865Z for workers 0000–0005 and only
this Batch. Three attempts, 3600s budget and progress required; the native 600s
lease cap remains, and future claims must explicitly request 600s.

Fresh backups and read-only rollback guards pass; no actual failure rollback,
classification or unattended execution is claimed. Four local stage checks and
eight Python tests pass. No binary/implementation, Access or Git history change.
See [INITIAL_EMOTE_BATCH.md](INITIAL_EMOTE_BATCH.md) for exact evidence and
safe rollback using an expired binding tombstone instead of removing protection.

## Phase R implementation (2026-10-25)

Phase R is a separate local implementation line from the historical G7/Formal
Batch evidence above. R0–R3 local isolated implementation and reliability gates
passed in commits `76cdcbc`, `16f40ba`, `b70a557`, and `004b93a`; details and
limits are in [PHASE_R0_ARCHITECTURE.md](PHASE_R0_ARCHITECTURE.md),
[PHASE_R1.md](PHASE_R1.md), [PHASE_R2.md](PHASE_R2.md), and
[PHASE_R3.md](PHASE_R3.md). The implementation branch
`codex/phase-r-execution-2026-10-25` was pushed only to the verified Fork
`lieyanbang/coord`; no push was made to `origin`.

The first Fork run
[40194822262](https://github.com/lieyanbang/coord/actions/runs/40194822262)
failed because Relay tests loaded before `npm ci`, leaving Express unavailable
on the clean runner. The dependency install was moved before Relay tests. The
corrected Windows Actions run
[40195567861](https://github.com/lieyanbang/coord/actions/runs/40195567861)
for `4e308edd2f8cd7c9852eb4a1eefd7e4bc935c7ed` passed every workflow step,
including fmt/clippy/tests/Release, health monitor, low-privilege Runner,
Relay/lease tests, MCP OAuth/HTTP, Python utility checks, and artifact uploads.

Artifact `coord-windows-2022` ID `12600096932` (6,571,823 bytes) matches that
commit; ZIP SHA-256 is
`a54d3c62af79b148aaa1f2563e13ffa3f47b51c855b341aa83b93f9f65b641d7`. The
downloaded `coord.exe` SHA-256 is
`e7c3695a21f88274cb84992f99a4ce9ce5b2f2bfb229a2768b3c6ccd85bdd13d` and reports
version 0.4.1. The downloaded Runner and publisher executables' hashes and
help checks, plus the 27/27 isolated Relay/Core test run using this exact
downloaded Core, are recorded in [PHASE_R4.md](PHASE_R4.md). No production
deployment or real ChatGPT session was performed.

Phase R overall remains **PARTIAL**. No real ChatGPT OAuth refresh or
conversation/context-loss run was attempted because Phase R is not deployed;
production routes, OAuth grants, formal Batch state, services, Canary, and
remote Runner were not touched. That boundary is **NOT_RUN**, not inferred as
PASS from the synthetic local MCP lifecycle. `TASK.md` remains byte-for-byte
unchanged.

## Phase R5 B3 production recovery (2026-10-27)

**B3 PASS / PRODUCTION READY; business remains FROZEN.** This subsequent
authorized deployment supersedes the earlier R4 non-deployment observation for
B3 only. A2 closed the isolated Servy/SCM restore, real retry-budget semantics
and old publisher compatibility gates. The verified Core/Runner artifact from
`ea9783268e33e546572fe346335056e28bc4d435` is now deployed on Arasaka with
independent management authentication and a locally generated signed Runner
identity; the original publisher and all three profile pins remain unchanged.

At 03:39:43Z, actual LocalService Core PID 9844 and existing low-privilege
Runner PID 4616 report all components READY. Two fresh SYSTEM PS5 monitor runs
captured real native SID/exit 0 and READY contents; the restored direct PS5 task
and later scheduled run also passed. Authentication 200/401, loopback listener
ownership, existing authenticated Sidecar/Relay/Core read RPC, Linux services,
public OAuth metadata, exact service configuration and key ACL boundaries pass.
Same-version compatible-Core recovery was actually exercised on acceptance
failures without restoring an old database or old G7 executable.

All original columns of seven business tables, formal Batch 105 completed / 16
pending, two old Canaries and historical 38 failed / 83 succeeded Actions are
unchanged. Twenty-four verified business guards remain active. The original
four business bindings expired before B3; their existing authorization denials
were retained. No successful ChatGPT business query, OAuth refresh, B4 session,
formal claim, unfreeze or failed-Action replay is claimed. These require a new
task and valid authorized identity window. Phase R overall remains PARTIAL
until its remaining separately scoped gates pass.

No new product source/formal-script changes or full CI rerun: the existing
local locked gate and [Fork run 40377126693](https://github.com/lieyanbang/coord/actions/runs/40377126693)
were reused and the cloud results read back. TASK/PLAN, unrelated services,
old broker key and existing Git work were preserved. Exact evidence, retained
failures, backups and safe recovery are in
[PHASE_R5_B3_RECOVERY.md](PHASE_R5_B3_RECOVERY.md) and
[PHASE_R5_B3_B_EVIDENCE_20261027.json](PHASE_R5_B3_B_EVIDENCE_20261027.json).

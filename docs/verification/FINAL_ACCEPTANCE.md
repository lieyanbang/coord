# Final acceptance — Windows coord MVP

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

**Controlled deployment and automated/site acceptance PASS; real two-ChatGPT
lifecycle NEEDS_USER_ACTION, overall G7 PARTIAL.** The user supersedes G6's
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

The browser runtime cannot initialize (kernel-assets os error 3). Four separate
synthetic browser tasks remain pending/unclaimed, and no actual two-conversation
G7 visual/renew/submit/next-claim cycle is asserted. Handoff and binding expiry
are in `../phase-g/G7-CHATGPT-CANARY.md`; operational recovery is in
`../phase-g/G7-DEPLOYMENT.md`. F3-R's existing non-owner/protocol evidence gaps
remain independently PARTIAL. No formal 121-task batch, classification agents,
old-task closure, Access-policy change, release/tag, history rewrite or machine
restart occurred. An accidental push to origin was denied; the verified Fork
working branch was then pushed normally. Full evidence and preserved failures
are in `PHASE_G7.md`.

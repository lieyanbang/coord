# Phase G7 — trusted logical workers and controlled deployment

Date: 2026-10-25 Australia/Brisbane. Current result: **DEPLOYED; automated/site
gates and real two-ChatGPT synthetic lifecycle PASS**. This acceptance is
limited to the authorized G7 trusted-owner synthetic scope.
The user accepts logical-worker impersonation within one OAuth owner and lack
of autonomous ChatGPT background wake. No other safety/integrity gate is relaxed.
Formal emote-000 through emote-120 tasks and classification agents are prohibited.

| G7 gate | Current result | Evidence |
| --- | --- | --- |
| A logical protocol and owner/legacy compatibility | PASS; actual OAuth reconnect and both logical workers verified | version-2 policy, explicit canonical worker, scope/batch/signature/fencing tests; actual installed OAuth legacy read |
| B bounded lease and repeated claim | PASS operator synthetic and actual two-chat lifecycle | production six-worker trace, natural expiry/reclaim, progress/renewal, consumed sequence and stale token denials |
| C actual Runner and controlled deployment | PASS | real SID/ACL/RPC/SYSTEM tests, online backups, matched CI executable deployment, service crash/uncertain and copied-DB rollback |
| D production synthetic canary and regression | PASS automated and actual two-chat subgate | 10/10 synthetic completed, 11/11 actions succeeded, final signed denial probes, original DB/service/route comparison |
| Formal classification / 121 tasks | NOT_RUN; not authorized | no formal batch or classification agent |

Final implementation is `f0aa7daed53a36f8c0c2418d935858bbf9f9fd9e` on Fork branch
`codex/phase-g7-logical-workers`. Final Windows run
[40167256908](https://github.com/lieyanbang/coord/actions/runs/40167256908)
completed success; downloaded binaries were executed and installed. The earlier
checkpoints below retain their original pending/failure context; this table and
the final closeout section describe the resulting state.

## Baseline and contract

Branch `codex/phase-g7-logical-workers` starts at G6 closeout
`5776e149bb0dbb61c226fc85a579a93e3a2f5145`. User-supplied PLAN §13 is preserved;
pre-existing `.gitignore`, untracked TASK.md, artifacts, node_modules and bytecode
are preserved. TASK.md is read-only with SHA-256
`cfcd0770ac530af0634945b3a140da8cc06fd27f72d6eeaf91b6394b6071220f`.
Only the verified Fork `lieyanbang/coord` work branch may be pushed.

Direct SSH read-only baseline at 04:41Z: Arasaka coord remains listener PID 5368
on 127.0.0.1:7777, installed SHA-256
`b58d3620b1b6de8035bfcb97e99f2e4fe654f48b1cb5719ef9fca013f5652e0d`.
coord/CoordRelayAgent run as LocalService, Servy as SYSTEM; urgent/high pending
counts are zero using skip_reclaim. Linux services and 13011/13012 are active,
Sidecar READY/connected/pending=0. Evidence: `stage-g7-windows-baseline.log`,
`stage-g7-linux-baseline.json`, SSH exits 0. No production engineering ledger
record is created before the gated canary; stage commits form this engineering
ledger and no task/ack IDs are invented.

## G7-A/B implementation and local evidence

The version-2 logical policy authenticates the existing owner OAuth client,
allows exact batch/worker/tool scopes and retains bounded policy/request TTLs.
No per-worker token pin is required; token rotation remains supported. Explicit
worker_id/batch_id accompanies every logical operation; canonical worker XXXX
derives core agent_id emote-XXXX. The facade signs all RPC parameters plus the
worker/batch context; both Relay hops reverify before dispatch. Legacy clients
and version-1 strong policies retain their contract, and unsigned legacy calls
cannot access protected batches. A logical ID is not authentication.

coord_agent_status now also returns an authoritative batch_progress snapshot,
so an empty claim cannot be mistaken for completion. Lease/token/attempt fencing,
600-second leases, 3,600-second attempt budget, at most three attempts and
non-replayed uncertain writes remain unchanged. Start packets and result
validators/native publisher accept canonical emote-XXXX while retaining old IDs.
The CI low-privilege rehearsal now publishes a real emote-0000 result.

The new logical tests first failed with exit 1 (unsupported policy/input) in
`stage-g7-logical-before.log`, then passed in `stage-g7-logical-after.log`.
The full Relay focused test initially attempted a recovered claim before the
batch's real retry backoff; its failure is preserved in
`stage-g7-relay-fixture-before.log`. Waiting for the reported available_at fixed
the fixture without changing core backoff. The focused suite then passed 24/24:
six synthetic workers, actual temporary daemon plus both Relay hops, concurrent
unique claims, progress/extension, consumed sequence, next claim, failure,
expiry/reclaim, foreign live token, stale submit, request replay, unsigned
protected-batch denial and journal token exclusion. These are synthetic clients,
not real ChatGPT conversations. MCP/OAuth focused tests pass 15/15; actual HTTP
missing OAuth returns 401 and wrong worker/batch never reaches Sidecar. Native
publisher focused tests pass 11/11, including logical receipt owner and index.

`scripts/verify.ps1 -Stage g7` completed all eight local gates with exit 0:
fmt, strict locked clippy, locked all-feature tests, release build, real Windows
PowerShell 5.1 health-monitor tests, Relay, CoordMCP and Python. Full commands,
timestamps, durations and logs are in `stage-g7-checks.json` and referenced logs.
Fork Actions and downloaded-artifact verification are pending at this checkpoint.

Official OpenAI authentication documentation was searched and opened:
https://developers.openai.com/plugins/build/auth . OAuth client reuse and token
validation are distinct from conversation identity. G7 changes only the accepted
logical collaboration model, not the Access/OAuth policy or protocol.

## G7-C/D deployment gates

Online backups have passed before any replacement. Real Arasaka Runner
account/SID/ACL, management RPC 401, actual SYSTEM authenticated health, matched
CI binaries, controlled service changes and canary results must be recorded here
before a production PASS. G6 disposable CI SIDs do not establish these gates.

The existing G6 backup/rollback sequence remains applicable to SQLite/WAL,
executables, Servy, health monitor, Linux modules/Sidecar/configuration and
OpenResty route preservation. Only G7's explicit identity/wake concessions
supersede its old strong-conversation gate. No production service replacement
or formal classification task has occurred at this source checkpoint.

The first Windows backup attempt failed because the F1-only checker required
exactly seven historical services. Evidence is retained in
`stage-g7-windows-backup-before.json` / `stage-g7-backup-failure.log`; its partial
directory was not overwritten or accepted. The checker now has an explicit
`--current-services` mode: capture every live service name and full-row SHA-256,
then require the snapshot to match all records. Its default historical mode,
schema/index/collation checks, full integrity checks and overwrite refusal remain.
`--verify-snapshot --inventory <create-report>` revalidates the captured inventory.
Publish and self-test exit 0, including additional-service and row-tamper tests;
see `stage-g7-servy-backup-build.log` / `stage-g7-servy-backup-after.log`.

Accepted Windows restore point:
`C:\ProgramData\coord\backup\g7-20261025T050716Z-eea8ecd6`.
coord online snapshot quick_check=ok, row counts match, SHA-256
`9ecbde7f4b209674e8355e557e6c4921f8697b378444408018feb1615579ef54`;
Servy snapshot/inventory verification PASS, SHA-256
`2e407ea3cee96b490a7ed281b8d8543121f1780bac8690e17656eaefd57127c7`.
Both use SQLite Online Backup API, including committed WAL. Old EXE, both
service exports, monitor script/state/task XML and Relay code/config/secrets are
inside the administrator/SYSTEM-only backup. No raw secret leaves that host.
Evidence: `stage-g7-windows-backup.json`, SSH exit 0.

Linux root-only restore point:
`/var/backups/coord-g7-20261025T050727Z-01d4e8f7`, archive SHA-256
`6fa0c6af1782f4317ddf77c392a74835d2e3b43a6a8fbe65dbf22cb7274f9bcb`.
Required config/key/OAuth/module members were reopened and verified. Service
units, Feathers entrypoint, Sidecar and OpenResty configuration are preserved;
the new facade sibling identity module did not previously exist. Evidence:
`stage-g7-linux-backup.json`, SSH exit 0. No service was restarted for backup.

Before the source commit, `scripts/verify.ps1 -Stage g7-source` again passes all
eight checks. The reports retain both full local gates. CI and actual production
Runner/canary verification remain pending, not PASS.

## Deployment checkpoint and clock-skew correction

Initial matching source `229d423b003a88bf2d49cca96502dfb803a482f1` passed Fork
Windows run 40164606057; both downloaded artifacts, all three executable hashes,
28 focused executable integrations and 24 Relay tests were verified. Actual
Arasaka dedicated SID/ACL/RPC and SYSTEM gates passed (`stage-g7-site-result.json`),
as did real Runner crash/uncertain recovery and old-binary readback of all 27
original fields in all eight original tasks (`stage-g7-site-recovery.json`).
Windows deployment completed at 05:43Z with authenticated READY and 401 on
unauthenticated management RPC. Linux deployment passed at 05:46Z after a helper
checked the wrong health port; its automatic component rollback preserved live
OAuth state and the corrected helper uses the configured loopback client port.
Both failure and success evidence are retained. Access policy and oauth.cjs are
unchanged. The signing key was handed off using ephemeral RSA-OAEP-SHA256;
plaintext stayed on protected remote hosts and the ephemeral private key and
transfer plaintext were removed. An earlier local plaintext transfer command
was rejected by automatic approval review without a detailed reason and was
not executed.

Only synthetic Canary batches were created: operator
`ddb65e62-2847-4dbb-a188-40f2c20c9c8e` (eight tasks), browser
`d2313c78-22da-4cb8-bcd7-ece9897e0db1` (four tasks). The browser runtime fails before
JavaScript initialization with `failed to write kernel assets`, os error 3;
the two real ChatGPT conversation lifecycle gate remains NEEDS_USER_ACTION.
An actual installed Coord OAuth legacy tasks_list succeeds and excludes protected
Canaries. No original task was closed and no classification agent was started.

The first operator run passed six concurrent unique claims, foreign-token denial,
progress and renewal, but a later submit was not dispatched because the Windows
clock can lag the Linux issuer. A 100-request read-only stress probe reproduced
intermittent rejection with current issuance and none with issuance one second
earlier (`stage-g7-signature-stress.json`). New regression first fails on the
old verifier. The correction permits at most 5,000ms future issuance skew while
retaining strict expiration, policy bounds, scope, signature and 60,000ms lifetime.
Boundary tests cover 5,000/5,001ms, exact expiration and excessive TTL. All eight
`scripts/verify.ps1 -Stage g7-clock` checks pass. A second matching Fork build and
download recheck are required before replacing the corrected Relay module.

Explicit downloaded-release capacity verification also passes 100/1,000/10,000
tasks with six independent MCP processes, exact unique completions/receipts,
consistent terminal counts and zero SQLite busy log lines. Raw measurements are
in `stage-g7-capacity.json`; this is local capacity evidence, not remote throughput.

## Final deployed acceptance (06:08Z closeout)

The corrected implementation's Windows run 40167256908/job 121090740089 is
success in every step, including real disposable standard-user execution.
`stage-g7-clock-actions.json` retains job steps and artifact metadata. Downloaded
artifact IDs are 12588930284 (binaries) and 12588144443 (low-privilege evidence).
ZIP SHA-256 values are respectively
`22041c53f93c29da1f69eca839eeaa9e2dbde4cdc9939e94dbbf0fd0372a926f` and
`e8e9253e87169d4dea35a42333fefe814112ffe8d217397850cc270087b88343`.

| Actual installed executable | SHA-256 |
| --- | --- |
| coord.exe | b23e43f30a076d3d3dc75e90a07a7a3f9de35ad33351bb2e9bf8938f9f220ed4 |
| coord-action-runner.exe | 72c2e72892405b16042f1e7a1066eb35493695d54de1183447ab771192fe14f8 |
| coord-emote-action.exe | cac5496ed1dea50f837f28403feb62515de1ba1459ea2557abe6097ec07bec55 |

All three differ from the initial CI and local builds. After matching ZIP/hash
checks, the downloaded executables pass 28 focused integrations (11 Runner/
publisher, 11 actions, 6 progress; child helpers intentionally ignored) and all
25 Relay tests. Their real CI standard-user rehearsal also passes and cleans
its disposable account/directory. See `stage-g7-clock-artifacts.json`,
`stage-g7-clock-artifact-recheck.log` and `stage-g7-clock-low-privilege/`.

Before the second executable update a further accepted-state online snapshot,
old G7 binaries and registry were saved in
`C:\ProgramData\coord\g7\private\clock-before-20261025T060304Z`.
The publisher profiles' approved hash pins were updated with the matching
publisher. Only the three coord-owned Windows services and affected Linux
Sidecar/Feathers were restarted. Current identity module SHA-256 at all three
loaded locations is
`13b19cf9d638a45152e847406b6abddf48758622ffc20ade1eb9c7f51dc73efa`.
Corrected deployment proofs: `stage-g7-windows-clock-deployment.json` and
`stage-g7-linux-clock-deployment.json`. Current-time and backdated read probes
both pass 50/50 (`stage-g7-signature-stress-after.json`); the original current-time
probe had 46/50 denials and backdated probe 0/50.

The interrupted operator canary was reconciled by exact state: its two accepted
tasks were retained and the other claims expired normally. Two additional
synthetic fixtures supplied the later expiry/failure probes, making ten total.
The second lifecycle trace proves six unique concurrent worker claims, foreign
live-token denial, fresh progress/renewal, consumed sequence denial, six accepted
submissions, request replay uncertain, next claims, natural one-second lease
expiry, bounded failure reclaim at attempt 2 and stale submit denial. Its final
assertion expected `not_dispatched` for a tampered signature, while the server
correctly rejected malformed identity as HTTP 400/state `error`. The FAIL trace
is retained unchanged in `stage-g7-production-canary.json`; no accepted task
was replayed to turn that fixture failure green.

Read-only reconciliation independently verifies HTTP 400 for tampered identity,
401 for a bad transport key, not_dispatched for a valid unsigned legacy request
to the protected batch, token-free status, and metadata-only journal keys.
An initial reconciliation fixture still declared emote-0000 after removing its
signature, correctly receiving schema rejection; that failure is preserved,
and the corrected fixture declares legacy coord-relay-agent. Final evidence is
`stage-g7-production-canary-reconciled.json` (PASS).

Authoritative manager verification is PASS in `stage-g7-production-final.json`:
operator batch `ddb65e62-2847-4dbb-a188-40f2c20c9c8e` is completed, total/terminal
10, active 0, all 11 actions succeeded at action attempt 1. Results 00–09,
index.json and review_queue/candidates.json exist; result/index hashes are
recorded. The actual listener process executable path/hash matches the final
coord artifact; 7777/7779 bind only 127.0.0.1. Current Runner service token is
SID `S-1-5-21-3592388187-2353680085-815129118-1004`, privileged=false, with only
SeChangeNotifyPrivilege required by SCM. SYSTEM monitor is enabled/Ready with
last exit 0 and fresh authenticated READY. Rehearsal ports 17777/17778/17779
are released. All eight old tasks retain all 27 original fields, quick_check=ok;
original-field fingerprint is
`d8211827f1e3d8861aab497ad63e24046ce54bbe8e0ccc0996a220e292c637dd`.
Seven protected unrelated/host services retain identity/start mode/state/PID.

`stage-g7-linux-final.json` proves both corrected identity module hashes,
root-owned 0640 signing key, Sidecar READY/pending 0, no automatic service
restarts, unchanged production oauth.cjs and all archived OpenResty configuration
files. Both registered OAuth clients and all original 19 access/19 refresh
entries remain present. Counts became 21/21 after normal installed-connector
token flows; the active store was never restored. Actual installed legacy
coord_tasks_list returned exactly the original eight tasks and excluded both
protected Canaries. Public 401, both metadata resources, actual
`/oauth/authorize` challenge, RemoteControlMCP/2/3 and `/mcp` regression checks
pass (`stage-g7-public-regression.json`). Its first fixture used the wrong
`/authorize` URL and the 404 evidence is retained. F3-R non-owner and independent
protocol gaps remain PARTIAL; these checks do not manufacture that evidence.

The final downloaded executable explicitly passes the opt-in release capacity
suite with COORD_TEST_BIN: 100/1,000/10,000 tasks, six independent MCP processes,
exact unique completions/receipts/terminal counts, zero busy log lines.
At 10,000: 98.8978s, 101.1145 tasks/s, claim P95 42.875ms, daemon peak WS
21,446,656 bytes (`stage-g7-clock-capacity.json/log`). These local measurements
overlapped maintenance activity; no controlled production benchmark is claimed.
Final `scripts/verify.ps1 -Stage g7-close` again passes all eight gates; the
closeout commit adds only operating helpers, documentation and evidence, leaving
the compiled Rust/Relay implementation at the matching f0aa7da source. Helpers
pass PowerShell/Python/Node syntax checks; actual executed helpers have separate
site evidence. Whole-site post-canary rollback is NOT_RUN, distinct from the
actual copied-DB/old-binary and component rollback rehearsals.

Remaining boundary: browser runtime repeatedly fails before JS initialization
(`failed to write kernel assets`, os error 3). Real ChatGPT visual/progress/
extend/submit/next-claim lifecycle is NEEDS_USER_ACTION, not synthetic PASS.
Browser batch `d2313c78-22da-4cb8-bcd7-ece9897e0db1` still has four pending,
unclaimed synthetic tasks. Both logical bindings expire at
2026-10-26T05:45:23.904Z and fail closed thereafter. See
`../phase-g/G7-CHATGPT-CANARY.md` for the two-chat handoff and
`../phase-g/G7-DEPLOYMENT.md` for protected recovery. Fourteen synthetic
production tasks were created in total; zero formal classification tasks/agents,
zero old-task closures, zero Access-policy changes or machine restarts.

Git history and original dirty/untracked files are preserved. An accidental
push using origin (the upstream) was denied with HTTP 403; no upstream state
changed. The corrected push explicitly targets the verified lieyanbang Fork.
No force push, release tag or Release was used. TASK.md retains its original
SHA-256. Reports preserve the real failures rather than replacing their outputs.

## Discovery repair and real browser closeout (07:03 UTC)

The preceding browser-runtime and NEEDS_USER_ACTION observations are historical.
Browser control recovered. OAuth succeeded while discovery initially failed
because a new DCR client was absent from the identity policy. The exact client
was bound only to the browser batch and workers 0000/0001, preserving expiry,
old grants, signing material and Access policy. The original Plus account
reconnected successfully, and ChatGPT showed all ten G7 tools.

Both original chats inspected synthetic marker previews and completed two tasks
each: fresh progress, bounded renewal, submit, exact readback and next claim.
Manager/SQLite/output checks prove 4/4 completed, 5/5 actions succeeded, no active
task, all attempt 1. B's first final reply failed to render; a read-only follow-up
confirmed both exact results and completed batch without resubmission. Old task
fields, protected service PIDs, non-admin Runner, 401/401 and SYSTEM READY pass.

All eight local g7-reconnect checks pass. Only policy, operating helpers and docs
changed; installed binaries remain the matching f0aa7da CI artifacts. Details,
exact IDs/backups/hashes and residual boundaries are in G7_DISCOVERY_REPAIR.md.
F3-R remains independently PARTIAL. No extra tasks, formal classification,
autonomous wake or protected conversation identity is claimed. Browser binding
still expires 2026-10-26T05:45:23.904Z.

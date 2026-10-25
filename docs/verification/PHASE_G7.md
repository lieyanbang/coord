# Phase G7 — trusted logical workers and controlled deployment

Date: 2026-10-25 Australia/Brisbane. Current result: **IN_PROGRESS**.
The user accepts logical-worker impersonation within one OAuth owner and lack
of autonomous ChatGPT background wake. No other safety/integrity gate is relaxed.
Formal emote-000 through emote-120 tasks and classification agents are prohibited.

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

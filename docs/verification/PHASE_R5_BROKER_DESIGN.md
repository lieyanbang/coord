# Phase R5-1B — Broker architecture and recovery design

Observed 2026-10-26 10:32–10:35 UTC (20:32–20:35 Australia/Brisbane).
This is the B0 read-only baseline and implementation design. It preserves the
earlier R5 execution-boundary FAILs, R5-1H PASS, R5-1K BLOCKED result, and all
recorded probe deviations. No production key, ACL, binary, registry, service,
database, task, receipt, action, OAuth record, route, or Batch was changed.

## B0 result and frozen repository state

- B0 source and production inventory: **PASS**. The live service gates remain
  **FAIL/BLOCKED** and require the B1–B3 implementation before recovery.
- `TASK.md` SHA-256 at start: `9a504d9e4f5a90c2e914121bd24552440cb5abb1036d2c253e61542dfa6066eb`.
  It remains read-only.
- Starting HEAD: `b80a4c88e881fd87ddaf5aee101b928320506f34`.
  Work continues on local branch `codex/phase-r5-1b-20261026`.
- The pre-existing `.gitignore` and `docs/PLAN.md` edits and all untracked
  artifacts, Phase H evidence, MCP experiment files, `node_modules`, and Python
  bytecode are preserved. They are not part of this report or the new audit
  outputs. There is no `coord` CLI on PATH; the connected Coord MCP only offers
  batch-scoped worker methods, not the repository heartbeat/send/claim ledger
  interface. No shared engineering daemon was established, so stage commits
  will be the local ledger.
- The R5-0/R5-1K historical records remain unchanged. A new B0 Windows snapshot
  is `r5-1b-b0-windows.json`; the Linux snapshot is
  `r5-1b-b0-linux.json`. Both were collected through the External Context
  Skill's direct SSH procedure. The Windows PowerShell audit and Linux Python
  audit are read-only and sanitize their output. The Broker key itself was
  never opened, hashed, or copied.

## Current production state

### Windows / Arasaka

At 10:35Z the `coord` Servy wrapper service reported Running as LocalService,
but no `coord.exe` child existed and neither 7777 nor 7779 had a listener. A
no-credential `tasks/list` probe could not connect. Servy (LocalSystem, PID
3224) and CoordRelayAgent (LocalService, PID 5892) were Running; the dedicated
`CoordActionRunnerG7` account service was Stopped. The health Scheduled Task
is Ready under SYSTEM; its latest run at 10:34:34Z returned 1. The last status
file was FAIL at 10:34:18Z (`tasks_list=FAIL`, `sqlite_read_only=PASS`, 619
consecutive failures). The wrapper's Running state is not Core readiness.

The installed old G7 Core copies at `C:\ProgramData\coord\bin\coord.exe` and
`C:\ProgramData\coord\g7\bin\coord.exe` have the same SHA-256 recorded in
the sanitized audit. The recent startup failure remains the previously
observed trusted-path inspection denial at `broker.key`; the current audit
found no Core process and no Core stderr file in the expected G7 runtime log
directory. The old Broker-key DACL still grants Administrators and SYSTEM full
control and the pre-existing operator read/execute ACE; it grants neither
LocalService nor the dedicated Runner SID access. No ACL was changed.

Read-only SQLite evidence: `integrity_check=ok`, WAL mode, `schema_version=32`,
`user_version=0`, and no `task_attempts` table. The current sanitized protected
fingerprints and counts are recorded in the JSON audit. The formal Phase H
Batch remains 121 tasks (105 completed, 16 pending), attempt sum 112/max 2,
106 progress rows, 145 progress receipts, zero extension receipts, 105 task
receipts, and 105 actions. The old operator Canary remains 10/10 completed
with 11 actions; the old browser Canary remains 4/4 completed with 5 actions.
No task/action state was written. These B0 fingerprints become the comparison
baseline for post-deployment read-only audits.

The old probe-without-output attempts are not acceptance evidence. The
R5-1K report records: one attempt wrote into a LocalService-unwritable runtime
directory; a subsequent Scheduled Task registration had invalid XML; the last
file-backed probe returned 1 without an artifact. Its cause was not proven,
and its result remains **INCONCLUSIVE**. The new identity tests must write only
to an exact disposable directory whose output ACL is verified before task
launch, and must require both a fresh artifact and the matching task result.

### Linux Coord relay hop

At 10:33Z on `instance-20260424-1840`, `coord-relay.service` (PID 127496),
`feathersjsapp.service` (PID 127510), and `openresty.service` (PID 109204)
were active with zero restarts. Sidecar `/healthz` returned HTTP 200,
`READY`, connected, pending 0. Ports 13011/13012 remained on the Tailscale
address and Sidecar remained on loopback 13013. No Linux service, route, OAuth
state, or config was changed.

## Source findings and failure path

1. `ActionRegistry::load()` deserializes `external_runner` as `RunnerConfig`
   and invokes `RunnerConfig::validate()`. That validation opens and reads the
   shared `key_file` after `safe_existing_path()` checks every path component.
   A missing, unreadable, malformed, or overlong Broker key therefore fails
   action-registry loading before the Core opens its Store.
2. `server::serve()` then reads the independent management RPC key through
   `management_key()`. That function also reads the Broker key to reject equal
   values. A Broker credential failure therefore remains a Core startup
   dependency even when the management key itself is healthy.
3. Still before the 7777 RPC listener is bound, `serve()` checks the daemon SID,
   binds the 7779 listener, and builds `broker_router()`. Identity, Broker
   config, key-read, and port-bind errors propagate with `?`/`bail!` and abort
   the entire Core. The Broker task is only detached after those synchronous
   steps succeed.
4. Broker requests use a long-lived shared Bearer key. Both Core and Runner
   read the same secret. The management Bearer key is separate on disk, but
   startup equality checking couples the two files.
5. `coord health` currently checks authenticated `tasks/list` and SQLite only.
   It cannot distinguish Core/DB readiness from Broker listener or Runner
   liveness. The SYSTEM monitor therefore has no truthful per-component state.
6. Existing Store actions already fence `claim`/`authorize`/`finish` by action
   ID, lease owner, lease token, and unexpired lease. Expired running actions
   become `uncertain`; only explicitly idempotent transient failures can be
   retried. These rules must not be weakened or bypassed.

## Approved design

### Authentication and key ownership

- Use Ed25519 with a supported, locked Rust implementation. The Runner holds
  one 32-byte private seed in a protected file; only the dedicated Runner SID,
  SYSTEM, and Administrators may read it. The Core receives only a public key,
  key ID, and configured Runner SID. The Core must never open or cache the
  private seed or legacy `broker.key`.
- Sign a strict UTF-8 canonical request envelope containing protocol version,
  key ID, uppercase HTTP method, exact origin-form path, SHA-256 of the exact
  request body bytes, decimal Unix timestamp, and a cryptographically random
  nonce. Use fixed field order, LF separators, ASCII-only header values, no
  query string, bounded header/body lengths, and one signature encoding.
- Enforce a bounded clock-skew window. Verify the key ID, signature, method,
  path, body digest, timestamp, nonce syntax, and configured Runner identity
  before dispatch. Consume each accepted `(key_id, nonce)` once in a durable
  SQLite table with an expiry; reject replays after Core restart as well as
  within one process. Invalid signatures never reach Store action operations.
- There is no implicit Bearer fallback. The migration first runs Core with the
  Broker disabled and Runner stopped. The signed Broker and new Runner then
  move together. Old G7 cannot authenticate to the new Broker; reverting to
  old G7 would require a separately authorized credential change and is not
  part of this recovery.
- Keep management RPC authentication independent. Remove the content equality
  comparison with the Broker credential. If management authentication cannot
  be loaded, do not expose unauthenticated write RPC.

### Startup and health state machine

| Component | Startup failure policy | Health reporting |
| --- | --- | --- |
| Core RPC | Fatal if Core identity or management RPC authentication is invalid; otherwise serve independently of Broker | `core=READY/FAIL` |
| SQLite | Fatal on open/schema/data failure | `database=READY/FAIL` |
| Broker | Initialize in its own task after Core listener is available. Missing config is `UNAVAILABLE`; bind/listener loss is `UNAVAILABLE`; malformed config, inaccessible public key, bad key mapping, or daemon identity mismatch is explicit `SECURITY_ERROR`. Never fall back to unsigned/Bearer auth. | `broker=READY/UNAVAILABLE/SECURITY_ERROR` |
| Runner | Signed heartbeat is required; stale/missing heartbeat is `UNAVAILABLE`. Its private-key or configured-SID failure is an explicit Runner security error. | `runner=READY/UNAVAILABLE/SECURITY_ERROR` |

Overall readiness is `READY` only when Core, database, Broker, and Runner are
all ready; it is `DEGRADED` when Scheduler/RPC/DB work while Broker or Runner
is unavailable; it is `FAIL` when Core RPC or SQLite is unavailable or the
management authentication boundary is invalid. `GET /healthz` and the health
CLI expose component checks without returning credentials. Legacy health
consumers retain `tasks_list` and `sqlite_read_only`, with an explicit overall
mapping rather than silently calling Broker-degraded operation fully READY.

The SYSTEM monitor restarts only Coord-owned services and only after bounded,
component-specific failure thresholds. Broker degradation must not cause a
Core restart loop. A dead Runner can receive one bounded start attempt only
when the signed Broker is ready. It must not alter Action state; queue items
stay pending and expired in-flight work remains subject to the existing
uncertain/fence rules.

### Compatibility, migration, and recovery sequence

1. Build a compatibility Core that treats legacy `external_runner` data as
   opaque for startup, loads the separate management key without comparing it
   to the Broker key, and can serve the current schema-32 database with Broker
   `UNAVAILABLE`. Keep the old registry/action profiles intact. Stop the old
   Runner before Core replacement.
2. Rehearse the compatibility Core on a verified copy of the latest online
   backup. Prove Scheduler/RPC/SQLite readiness while 7779 is deliberately
   unavailable; prove task/action counts, accepted receipts, and old result
   fingerprints are unchanged. Rehearse copy-only rollback and old-binary
   database read boundaries without ever opening a migrated production DB in
   the old binary.
3. Deploy only the verified compatibility Core and Coord-specific service
   parameters; leave Relay, Sidecar, OAuth, routes, and unrelated services
   untouched. Wait for real SYSTEM health. This restores ordinary Scheduler
   service without granting LocalService access to `broker.key`.
4. On Arasaka, generate a new Runner keypair locally. Do not copy the private
   key through the workspace or CI. Apply exact DACLs to the new key files and
   service configuration; record only identities/rights, never key bytes or
   hashes. Verify Core can read the public key, Core cannot read the private
   key, and Runner SID can sign.
5. Deploy a matching signed-Broker Core config and Runner binary/config. Keep
   the same protected database; the nonce table is additive. Require positive
   and negative request-auth tests, Action fencing/uncertain tests, service
   identity and ACL proofs, real SYSTEM health, and read-only protected
   Batch/Canary fingerprints before declaring full readiness.
6. For a Broker-stage fault, roll back to the verified compatibility Core
   (Broker unavailable, Scheduler preserved). Do not restore an old DB over
   any newly accepted result and do not grant old G7 access to its Broker key.
   If Core/DB rollback cannot preserve accepted state, stop and forward-fix.

## Failure-injection and acceptance matrix

| Scenario | Required result |
| --- | --- |
| Broker config absent | Core, SQLite, authenticated RPC and permitted scheduler reads start; Broker/Runner explicitly unavailable; no 7779 listener |
| Public key missing, unreadable, malformed, or wrong key ID | Core remains available; Broker reports explicit security error; no unsigned or Bearer fallback |
| Configured daemon/Runner SID mismatch or config tampering | Explicit security error and no Broker listener; Core does not inherit Runner execution |
| Broker port conflict | Core remains available; Broker reports unavailable; no alternate address or silent retry storm |
| Runner stopped/crashed | Broker stays ready, Runner becomes stale/unavailable; queued actions remain queued; no duplicate action spawn |
| Runner valid claim/authorize/finish | Signed request accepted; fixed-profile action identity, authorization proof, and Store fence remain intact |
| Missing/forged/expired/replayed/wrong-path/wrong-method/body-modified/cross-key signature | Rejected before action Store mutation; accepted nonce remains rejected after Core restart |
| Action Runner crash after authorization | Existing action becomes/retains `uncertain`; no automatic replay of non-idempotent action; stale finish fence rejected |
| Health monitor sees Broker-only failure | Overall status is DEGRADED; Core is not restarted; only one bounded Runner recovery is allowed when Broker is ready |
| Additive nonce-schema migration | Integrity and protected rows/results/receipts/actions remain unchanged; compatibility Core reads the migrated copy; old G7 is not pointed at production migration |

## B0 stop conditions

Do not proceed with production recovery if any current protected fingerprint
differs, the online backup is not independently verified, the management RPC
key cannot remain separately protected, the new Core needs the old key contents,
or the compatibility Core cannot keep Scheduler/DB readiness independent from
Broker. Any failed/inconclusive identity probe remains **FAIL** or
**INCONCLUSIVE**, never a permission PASS. Formal Phase H tasks, both old
Canaries, and their existing failed business Actions remain read-only.

## B0 read-only refresh — 2026-10-26 14:45 UTC

The direct SSH refresh is preserved in
[`PHASE_R5_B1_LIVE_RECHECK_20261026T144506Z.json`](PHASE_R5_B1_LIVE_RECHECK_20261026T144506Z.json).
The scheduler Core and legacy Runner remain stopped; Servy and the Windows
Coord relay agent remain running. Ports 7777/7779 have no listeners. The real
SYSTEM monitor is enabled and scheduled, but its latest result is 1; the health
file is `FAIL` after 869 consecutive failures. Its legacy schema still has no
Core/DB/Broker/Runner component fields.

The configured production registry still has the legacy shared Broker-key
field and no Runner public-key mapping. The Broker-key DACL was inspected
without opening or hashing the file; no LocalService or Runner grant is
present. The independent management-key configuration remains present. No
ACL or service change was made.

The exact `TASK.md` Batch ID is
`90ce2ca7-184e-4c3d-9d87-56386db25419`. The read-only SQLite audit reports
`integrity_check=ok`, WAL mode, schema cookie 32, `user_version=0`, and no
nonce or derived-attempt tables. Its 121 task states remain 105 completed and
16 pending. All three protected fingerprints match the 12:49Z snapshot. The
database also contains a separate older active Batch with one completed task;
it was not created or changed in this phase and remains outside the protected
work. The 38 failed completion actions remain recorded and untouched.

An initial exploratory SQL probe in this refresh used a transposed UUID and
returned no formal-Batch rows. The parameter was corrected to the exact
`TASK.md` value; the succeeding read-only audit found the expected Batch and
counts. No SQL write or state transition occurred. The Linux relay, Feathers,
and OpenResty units were also read-only rechecked active with zero restarts;
the loopback sidecar returned `READY`, connected, pending 0.

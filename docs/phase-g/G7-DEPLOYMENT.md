# G7 controlled deployment and recovery

The user accepts cooperative four-hex worker labels under one OAuth owner and
the lack of autonomous ChatGPT background wake. Other authentication, fencing,
Runner privilege, ACL and fixed-action requirements remain hard gates. This
procedure authorizes synthetic canaries only; formal classification is excluded.

## Restore points and actual site

Windows baseline: `C:\ProgramData\coord\backup\g7-20261025T050716Z-eea8ecd6`.
The online coord snapshot and collation-aware Servy snapshot passed integrity,
row counts and full current-service inventory verification. Old executables,
service exports, Relay config/secrets and monitor task/script are protected
there. A fresh additional pre-deploy online snapshot is recorded in the private
G7 deployment manifest. Linux baseline:
`/var/backups/coord-g7-20261025T050727Z-01d4e8f7/configuration.tar.gz`.
Hashes and verification results are in `../verification/PHASE_G7.md`.

The installed G7 root is `C:\ProgramData\coord\g7`. coord and the Windows Relay
retain LocalService. `CoordActionRunnerG7` uses standard local account
`coord_action_g7`, SID `S-1-5-21-3592388187-2353680085-815129118-1004`.
SCM grants its service token only SeChangeNotifyPrivilege. Its actual native
service probes passed non-admin identity, fixed-file read/write ACLs, denial
of DB/management/signing keys/all six private worker workspaces, and 401 on
management RPC using both missing credentials and its broker credential.

The main RPC stays on 127.0.0.1:7777 with a separate management key. The Runner
broker is 127.0.0.1:7779. SYSTEM health monitoring uses a separate protected
copy of the management key and was verified by actual scheduled-task execution.
Do not give the Runner this key or broaden the monitor copy's ACL.

Production profiles `g7-canary-v1` and `g7-browser-v1` pin the native publisher's
hash and fixed root/arguments. Their outputs are separate under `canary/business`
and `browser/business`. The crash ping profile exists only in the copied-DB
rehearsal registry and is absent from the production registry. Runner writes
only specified outputs/runtime; refs/task inputs stay read-only. The probe and
rollback test services/processes are stopped and their three ports released.

## Applying changes

The `scripts/windows/*g7*.ps1` and `scripts/*g7*.py` helpers are exact, audited
one-site procedures, not repeatable general installers. Inspect persisted
private state before resuming a partial run. Do not recreate the account or
overwrite a restore point. Servy parameters are changed through export/import,
preserving existing service identity and unrelated options. The installer uses
`--params=<value>` when the value begins with an option.

Before replacing files, require matching source commit, successful Fork Windows
run, downloaded ZIP/executable hashes and executable rechecks. Rehearse the
actual Runner identity/ACL, SYSTEM authenticated READY, publish/index, process
crash/uncertain and old-executable database compatibility first. Never substitute
a disposable CI SID for the actual Arasaka account.

Disable the health task only during an executable/authentication maintenance
window. Stop the Runner, Relay and coord as required; update matching binaries,
the fixed registry and monitor arguments together. Restore health monitoring
only after authenticated readiness. If executable hashes are unchanged, a Relay
module update needs only the affected Relay/Feathers restarts. Never restart the
whole machine, OpenResty or unrelated services.

Linux updates are limited to Sidecar server/protocol/identity, the CoordMCP
index and its sibling identity module, plus the Coord-only unit drop-in.
`oauth.cjs`, active OAuth store, shared device environment, Access policies and
OpenResty routes remain intact. The key handoff used RSA-OAEP-SHA256; do not copy
plaintext secrets through the workspace or print them. Both hops load equivalent
public bindings with at most 24-hour TTL. Host-clock tolerance is at most five
seconds for future issuance; expiration and 60-second lifetime remain strict.

Create empty manager-only Canary batches, bind their actual UUIDs at all hops,
then add synthetic tasks. Browser and operator batches have distinct profiles
and output roots. The manager API is never exposed through MCP. Protected batches
are excluded from legacy list/get/claim. Keep tokens only in the claimant process.
Reconcile an uncertain outcome with exact task state before any subsequent call;
never fetch a newer attempt's token to finish older work.

## Recovery

The first Linux helper's wrong health-port check exercised automatic module and
configuration rollback while preserving active OAuth grants. The corrected
helper reads the configured Sidecar port (13013), leaving 13011/13012 unchanged.
Arasaka's isolated service rehearsal killed only the owned Runner child, proved
Job child termination, `uncertain` attempt 1 without replay and successful Servy
recovery. Old-binary readback on a migrated copied database preserved all old
task fields. These are controlled process/copy tests, not physical power-loss.

If recovery is needed, inspect current actions/receipts and stop new claims.
`scripts/windows/rollback-g7.ps1` stops only the three owned services, takes an
additional quiescent accepted-state snapshot, restores the old code/service/
monitor, leaves the Runner stopped/manual, and preserves the current database.
`scripts/rollback-g7-linux.py` restores only affected modules/config from the
verified tar and removes only the G7 drop-in, then restarts Sidecar/Feathers.
The final whole-site rollback helpers are syntax-checked procedures; a full
post-canary production rollback has not been performed. Their component
mechanisms were exercised in the rehearsals above.

Never restore the pre-deploy database over newly accepted work or restore the
old OAuth store over live grants. Keep published outputs/receipts for inspection.
Any database restore requires reconciling new records and its own exact snapshot
selection. Never automatically replay an uncertain non-idempotent action.

Actual current deployment, final CI hashes and canary outcome are recorded in
`../verification/PHASE_G7.md`. Historical G6/F3-R boundaries remain independently
scoped; two real ChatGPT lifecycle tests cannot be claimed from operator clients.

## Exact reconnect binding repair (2026-10-25)

A normal ChatGPT reconnect produced a new DCR client outside the configured
identity bindings. OAuth succeeded but discovery returned 403. The precise
new client was admitted only for the existing browser batch and workers
0000/0001, with the original scopes and expiry. Original bindings, legacy
allowlist and signing material remain unchanged. Both actual chats now pass
4/4 tasks and 5/5 actions. No new binary was deployed.

Before-state backups, affected-service reloads, exact repair/rollback steps and
verification are in `../verification/G7_DISCOVERY_REPAIR.md`. The site-specific
repair helpers deliberately reject reapplication. Future reconnects must reuse
the approved public OAuth client or undergo an exact binding review; never
allow every DCR client automatically. Expiry remains 2026-10-26T05:45:23.904Z.

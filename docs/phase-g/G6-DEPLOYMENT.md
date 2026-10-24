# G6 deployment and rollback gates

This is the operating sequence for Phase G6, not evidence of deployment.
Production remains disabled until the independent identity and low-privilege
execution gates in `../verification/PHASE_G6_A.md` and `PHASE_G6_B.md` pass.
An isolated daemon and synthetic clients cannot satisfy those production gates.

## Dependencies and stop conditions

| Transition | Required evidence | Stop before |
| --- | --- | --- |
| Local implementation -> isolated rehearsal | Windows fmt, locked clippy/tests/release, Relay/MCP/Python tests | Shared daemon or existing database writes |
| Identity binding -> production enablement | Six real ChatGPT connection contexts; reconnect, expiry, replay, wrong identity and batch denial; credentials unavailable to other conversations | Installing an enabled production identity registry |
| Runner -> production enablement | Different dedicated Windows identity from daemon; actual ACL read/write denial; approved executable hashes; receipt/attempt/action-fence checks; crash/timeout recovery | Starting production actions |
| Build -> deployment candidate | Fork working-branch commit, successful Windows Actions job, downloaded artifacts and hashes, execution of downloaded binaries | Replacing deployed binaries |
| Candidate -> production canary | A/B PASS, documented restore points and independently checked backups | Any production batch/task creation |
| Canary -> operational acceptance | Legacy tools, health, OAuth/Access, existing task preservation, ports 13011/13012 and isolated test lifecycle | Formal classification tasks or agents, which remain outside authorization |

F3-R remains independently PARTIAL until the non-owner rejection and missing
protocol evidence are actually collected. Do not refresh an OAuth credential
just to manufacture a read-only audit result. Report UI-driven normal login
or refresh separately if exercised.

## Backup and rollback table

| Component | Before deployment | Restore boundary |
| --- | --- | --- |
| Arasaka coord | SHA-256 of installed/candidate executables; consistent SQLite backup including WAL semantics; DB read-only integrity check; existing service identity/configuration snapshot | Stop only the owned coord process through its existing service manager; restore old binary and pre-upgrade DB together; restart only coord |
| Action Runner | Dedicated service identity/SID, exact executable/registry hashes, service configuration and ACL export; broker credential stored outside workspace/logs | Stop only Runner; disable new broker/registry; restore its files/ACLs. Keep any uncertain action for manual reconciliation |
| SYSTEM health monitor | Scheduled-task arguments/script and protected monitor-only management-key copy, matching the daemon key without printing either; actual SYSTEM authenticated READY | Restore prior task/script configuration together with the prior RPC authentication setting; avoid recovery loops caused by mismatched credentials |
| Feathers CoordMCP | Root-only snapshot of affected module files and identity/OAuth configuration, hashes and syntax checks | Restore only affected CoordMCP files/configuration and restart Feathers only if needed |
| Sidecar/agent | Existing files, protected configuration and process/service metadata; listeners and health before change | Restore only affected Relay/agent files; targeted restart after syntax checks; verify 13011/13012 |
| OpenResty/Access | Read-only route/policy inventory; no policy changes required by this implementation | Leave existing policies/routes intact; any later route change requires its own validated configuration and restore point |

Use SQLite backup APIs or a verified quiescent copy. Copying only a live `.db`
file is insufficient. Never overwrite the only backup with an upgraded DB.
Never restore an old DB over accepted new production work: first isolate and
reconcile the changed records. Rollback of external files is separate from
SQLite rollback; inspect action receipts and published hashes before replay.

## Controlled sequence after the gates pass

1. Recheck Git/Fork HEAD and candidate hashes, service identities, available disk,
   queue state and health. Preserve all current tasks and leases. Record a
   bounded maintenance window and the exact component restart list.
2. Create and verify the restore points above. Keep credentials in protected
   files; reports contain only field names, counts and hashes.
3. Install immutable binaries/configuration with actions disabled. Configure a
   dedicated Runner account/SID and minimum ACLs. Verify denial of writes to
   DB, executables, registry, other Agent attempt directories and system paths
   using the real Runner token. A shared built-in service SID alone is not
   sufficient proof of separation from another service using that SID.
   Before enabling management RPC authentication, update the existing health
   monitor task to pass `-RpcAuthKeyFile` referencing a separate monitor-only
   protected copy of the same management key. Restrict that copy to SYSTEM,
   Administrators and the actual monitor identity; the daemon key path may
   separately permit its service account, including the existing LocalService.
   The monitor deliberately refuses additional reader identities on its copy.
   Validate a boolean-only key match and actual SYSTEM authenticated health
   `READY`; malformed/missing key configuration must exit 2 without Servy
   recovery. The key is private process environment only during CLI invocation,
   never a scheduled-task argument value or persisted environment variable.
4. Restart only components actually changed, then verify health, legacy exact-ID
   operations, public metadata/challenges, 13011/13012 and existing task states.
   Stop immediately on a mismatch and use the component restore boundary.
5. Enable the approved identity registry and external Runner for one isolated
   canary batch. Use synthetic inputs/test images only. Record claim, progress,
   renewal, submit receipt, next claim, action outcomes and file hashes. Keep
   lease tokens out of evidence. Expire a canary attempt and prove stale denial.
6. Stop the canary on identity ambiguity, privilege/ACL failure, action
   uncertainty, receipt mismatch or changed unrelated service state. Reconcile
   before retrying. Do not create `emote-000` through `emote-120` or start a
   classification Agent.

No production backup, service restart or canary should be performed merely to
complete this checklist while A/B are blocked. The G6-D report records which
steps actually ran and which remain NOT_RUN.

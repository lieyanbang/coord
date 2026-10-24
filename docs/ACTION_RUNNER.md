# Windows Action Runner

Phase G6 separates completion execution from the daemon process. A trusted
registry's `external_runner` starts a dedicated loopback queue broker in the
daemon; that branch never creates an action child process. Start the independently
configured Windows service/process with:

```powershell
coord-action-runner.exe --registry C:\ProgramData\coord\actions\config\registry.json
```

The service manager must run that command as a dedicated **standard local or
domain account**, distinct from the daemon account. Store its service credential
through the chosen Windows service manager, not arguments, environment variables,
the registry JSON or the broker protocol. Servy configuration and service-logon
rights require an operator review on the actual host before activation. No
production account, service or ACL was created by the source implementation.

The executable checks its actual process TokenUser SID, elevation, Administrators
membership (including a UAC deny-only group), and dangerous assigned privileges.
SYSTEM, shared LocalService/NetworkService identities, the daemon SID, elevated
or administrator accounts, and a mismatching configured SID are rejected before
network access. A shared built-in service identity does not isolate this runner
from other services on the machine and is intentionally unsupported. A filtered
administrator token is also insufficient.

The broker exposes only `POST /claim`, `/authorize` and `/finish`, behind its own
32-byte random bearer secret encoded as 64 hex characters in `key_file`. External
mode also **requires** an independent `daemon_rpc_key_file`: the daemon reads it
and requires its bearer value on every management RPC POST. Missing credentials
or the runner's broker credential return HTTP 401 before dispatch. The runner
registry loader does not open that management secret, and its NTFS ACL must deny
the runner read access. Discovery GET stays readable. Without external mode the
existing default localhost RPC remains for trusted local clients and is not a
hostile-user security boundary.

Before enabling external-mode RPC authentication, configure the existing SYSTEM
health-monitor task with `-RpcAuthKeyFile <protected monitor copy>`. That file
must contain the same 64-hex management key while being a **separate protected
copy**, readable only by SYSTEM, Administrators and the monitor identity. The
daemon's own key file may also grant its actual service account (for example,
the existing LocalService daemon) read access; that shared service grant is not
accepted on the monitor copy. The monitor validates absolute non-reparse paths,
regular-file size, owner/effective allow entries and key format; invalid/missing
configuration exits 2 before probing or requesting recovery. It sets
`COORD_RPC_AUTH_KEY` only around the CLI invocation and restores the prior process
environment in `finally`, before any Servy command. Do not put the key in task
arguments, registry XML, logs or recovery JSON. Verify the two protected copies
match using a boolean-only check, then run the configured task as its real SYSTEM
identity and require authenticated health `READY` before activation. Retain the
legacy task configuration as a restore point.

Operator CLI/MCP processes can receive `COORD_RPC_AUTH_KEY` in their private
environment for loopback HTTP only. The relay agent supports an independent
administrator-protected key file. Never reuse an OAuth, relay, Agent or broker
credential as the management key. Do not grant a classification Agent that key.
The broker key grants only its three queue operations, with no JSON-RPC dispatch.

The runner does not open SQLite. Claiming retains the existing 360-second action
lease, at most two running actions and fencing. Authorization reconstructs the
event from SQLite and requires at least 305 seconds remaining; it rechecks the
completed task's current attempt, exact accepted result and matching submission
receipt, including validator submission receipts. Agent-supplied receipt or
command fields are ignored. Only the G4 group/ID input fields are included in
authorization context; arbitrary task payload and known secret fields are not
forwarded. The native child receives that event on stdin without task/action
lease tokens or a broker key. The fixed command clears its environment and
retains memory/process/output limits and the 300-second maximum runtime. Windows
creates it suspended, assigns its Job Object, then resumes its primary thread,
closing the previous spawn-before-assignment execution window.

Every external executable must have a SHA-256 pin. Registry loading and each
spawn recheck that pin and all path ancestors. Administrators must deny runner
write/delete/ACL-change rights to binaries, registry, keys and their parent
directories; hash checks alone cannot prevent an attacker replacing an executable
between validation and process creation. Use a new profile ID/version for any
command or binary revision. The default without either execution mode queues
actions without executing them. `isolated_in_process: true` exists only for
disposable regression fixtures and must never appear in production.

| Location | Daemon/admin | Runner | Classification Agent |
|---|---|---|---|
| daemon DB/WAL/SHM, daemon configuration, management secret | Required access | No read or write | No access |
| action executables, registry, broker key | Administrator writes; daemon reads | Read/execute only | No access |
| task input, `refs` | Read | Read only | Read only through approved tools |
| runner scratch/logs | Operator inspection | Modify only these subtrees | No access |
| each Agent's private attempt directory | Operator inspection | No need to read; publisher uses accepted DB result | Only that Agent's owned attempt |
| dedicated batch `results`, `.receipts`, `review_queue` | Operator inspection | Modify only those subtrees | Read only if required |

Precreate output directories. Do not grant runner create/delete rights on their
trusted parent or permission to change ACLs. Reject inherited broad write grants
and verify effective access through the actual runner token. NTFS permissions
are filesystem isolation; production also needs actual missing-key and broker-key
management RPC rejection. Until those boundaries and the actual production
SID/ACLs have passed, keep actions disabled in production.

`coord-emote-action.exe --root <fixed administrator-selected work root>` validates
the strict G4 result fields, image coverage, agent/attempt context, bounded JSON,
candidate references, review evidence and all ancestor reparse points. It adds
the schema's stricter candidate `refs/<game>/icons/<file>` shape. It never consumes
an Agent-selected staged filename: accepted inline results and task inputs come
from the broker's receipt-checked snapshot. An expired Agent's scratch file
therefore cannot replace a newer attempt's publication.

Task publication writes `results/<group>.json` and
`results/.receipts/<group>.json`. A unique temporary file is flushed and linked
atomically to the final name without replacement on the same local NTFS volume.
Identical retries succeed; different existing bytes or a reparse point fail.
Result and receipt are separately atomic: an interrupted pair can be repaired by
an idempotent retry only if existing bytes match. The batch action waits for all
task actions to succeed, revalidates their receipts/results and includes groups
00 through 120. It writes `results/index.json` and
`results/review_queue/candidates.json`, producing candidate comparisons without
global character IDs or automatic merges. Use a separate output root for each
batch; conflicting old outputs require operator reconciliation, never deletion
by an Agent.

On broker/network failure the runner exits so the service manager may restart it
with bounded backoff. It does not replay an uncertain finish call. Expired
non-idempotent actions become `uncertain` with no automatic second invocation;
completed tasks stay completed. Expired idempotent actions can retry within the
pinned limit; publication's byte comparison resolves a crash after a file was
already published. Read `actions/status` and inspect exact receipt/output hashes
before an operator decides how to resolve uncertainty. Rollback stops only the
dedicated runner, keeps outbox/receipts/results, restores the backed-up daemon
configuration and verified prior binary, and never resets task state to replay
an external effect.

The disposable CI acceptance command is:

```powershell
pwsh -NoProfile -File scripts/windows/test-action-runner.ps1 `
  -BinaryDirectory target/release `
  -EvidenceDirectory docs/verification/stage-g6-low-privilege
```

It deliberately refuses non-GitHub-Actions hosts. On an administrative disposable
Windows runner it creates one random standard user and an exact random temporary
root, applies only fixture ACLs, runs real credentials with `Start-Process`, checks
actual token/access and management RPC denial, publishes/indexes a one-task
isolated batch, kills only its own runner and confirms Job Object cleanup plus
non-idempotent `uncertain` recovery. Forced lease expiry affects only the fixture
database. The random account and test directory are cleaned in `finally`,
preserving redacted evidence elsewhere. This is isolated evidence, not production
account, six-Agent workspace or hardware acceptance.

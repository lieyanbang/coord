# G7 ChatGPT action discovery repair

2026-10-25. The original reconnect completed OAuth but failed MCP discovery:
`Authentication succeeded, action discovery failed`. This repair uses the
existing deployed G7 implementation; Rust/Relay source and the three installed
CI executable hashes are unchanged.

## Cause and bounded repair

The 06:27 UTC DCR registration generated a new ChatGPT OAuth client. The
06:27:29 token exchange returned 200, followed by MCP 403 at 06:27:30. The
facade's `resolvePrincipal` rejected the absent client binding before
initialize/tools/list. The old client remained allowed. Optional OpenID 404
and canonical redirect responses were not the cause.

Only the exact new client digest
`2e12506a52763d439506792019e34debb54b912d2575c82c3ef275af5cd01ab1`
was added as `g7-browser-reconnect` at all three policy-loading locations.
Its only batch is `d2313c78-22da-4cb8-bcd7-ece9897e0db1`, workers only 0000
and 0001, with the existing read/claim/progress/extend/submit/failure scopes.
Original bindings, legacy allowlist, signing key, not-before and expiry are
preserved. Expiry remains **2026-10-26T05:45:23.904Z**, failing closed thereafter.
No blanket DCR admission, Access change or manual OAuth-store/token edit occurred.

Exact before-state backups:

- Windows: `C:\ProgramData\coord\g7\private\reconnect-20261025T063741Z-97393b76`
- Linux: `/var/backups/coord-g7-20261025T050727Z-01d4e8f7/reconnect-20261025T063752Z-33cda995`

Only Windows CoordRelayAgent and Linux Sidecar/Feathers were restarted.
The repair helpers reject repetition when the binding already exists; they
are site-specific records, not unattended client-registration automation.
Rollback restores each host's `identity-before.json` and reloads only those
affected services, revoking the new binding while retaining the old policy.
Never restore the active OAuth store to undo normal login/token flows.

Signed read probes through both loaded relay hops accepted 0000/0001, denied
0002 and denied the operator batch. This is operator transport evidence,
separate from the real ChatGPT acceptance below.

## Actual ChatGPT reconnect

The browser runtime recovered. The original account's old Coord catalog entry
returned `Plugin not found`. A replacement private Coord entry was created,
using the already approved public OAuth client, existing callback, scopes and
server URL `https://lieyanbang.com/app/CoordMCP/`; no additional DCR client was
generated. The first callback encountered an expired ChatGPT login and lost
its callback state. After returning to the original Plus personal account,
normal OAuth was repeated successfully.

At 06:54:52 UTC token exchange returned 200; the ensuing MCP handshake included
200/202 responses. ChatGPT visibly reported the app connected and showed seven
write and three read tools with G7 worker/batch descriptions. Chat A's actual
`coord_agent_status(worker_id="0000",batch_id=...)` accepted the refreshed
arguments and returned exactly four pending, zero claimed tasks.

For image inspection, the original synthetic task and reference PNGs were
read through direct SSH. Their SHA-256 is
`894ef62f7a9533bb3e89aab6aeed003a8b512e033418d58d35e0165bfd933833`.
Each is 1x1, opaque white. Two 256x256 nearest-neighbor previews were attached
to each authorized chat, with their exact provenance. File-chooser upload was
unavailable because the extension lacks file-URL access; supported image
clipboard paste succeeded without changing extension permissions. Both chats
actually inspected the attachments and recorded opaque white pixels, with no
character claim or remote-directory inspection claim.

Local `scripts/verify.ps1 -Stage g7-reconnect` passed all eight checks, exit 0:
fmt, locked clippy/tests/release build, health monitor, Relay, Coord MCP and
Phase G Python tools. The PowerShell repair/verification scripts parse, and
the Python repair script compiles. No new native artifact is needed for this
policy-only deployment repair.

## Real two-chat acceptance: PASS

The manager's final snapshot at 07:03:15 UTC proves **4/4 completed, active 0,
5/5 actions succeeded at action attempt 1**, with no failures or cancellations.

| Chat / worker | Task | Group | Attempt |
| --- | --- | --- | --- |
| A / 0000 | 19355413-eeda-4d57-b499-3adcdf9e40e5 | 02 | 1 |
| A / 0000 | 462e3022-8da0-4826-8330-3d7ba43a6068 | 03 | 1 |
| B / 0001 | 6c215a3b-3376-4fe6-8631-541906e85330 | 01 | 1 |
| B / 0001 | dcfc7146-2d8c-4860-b14f-9eb1c9d68ae4 | 00 | 1 |

Each task has its matching worker/attempt progress sequence 1, consumed
extension sequence 1, a completion receipt, and a published JSON equal to its
database result. Each worker's second claim occurred after its first task's
completion. Chat A's visible final response confirms both exact readbacks.
Chat B executed both lifecycles, but its original final response rendered
`Unable to load this response`; an explicitly read-only follow-up performed
two more exact get-task calls and agent-status, visibly confirming both
completed tasks, attempt 1, emote-0001, and batch completed 4/4, active 0.
No write was replayed to repair this ChatGPT display issue. Per-call approval
used Allow once; persistent tool-approval policy was not broadened.

The four result hashes match index entries. `index.json` is structurally
complete, expected/available 4, no missing groups; review candidates are empty,
consistent with these marker-only fixtures. All six output files are hashed
in `stage-g7-browser-canary.json`. The verifier's first invocation correctly
failed while B was still active; no mutation occurred. After B completed,
both task/renewal/publication checks and the added index/hash checks passed.

Final production verification at 07:03 UTC preserves all 27 fields of the old
eight tasks, quick_check=ok, the original-task fingerprint, all seven protected
service PIDs/identities/start modes, installed binary hashes, non-admin Runner
SID, management 401/401 separation, loopback listeners and fresh SYSTEM READY.
Operator Canary remains 10/10 with 11/11 actions. Linux access logs show the
real lifecycle's MCP calls returning 200 and a normal token refresh returning
200; loaded services remain active with no automatic restart counts.

Evidence:

- `stage-g7-discovery-repair.json`: backup, exact policy change and signed
  positive/negative scope probes, without signing material.
- `stage-g7-discovery-http.json`: full-file scan statistics and query-free
  request paths/statuses; no tokens, callback codes or cookies.
- `stage-g7-browser-canary.json`: manager/SQLite/progress/action/output evidence.
- `stage-g7-reconnect-production.json`: final production regression.
- `stage-g7-reconnect-checks.json` and matching logs: all eight local checks.
- `g7-browser-a.txt`, `g7-browser-b.txt`: visible conversation text, no lease
  values. Proof screenshots remain in ignored `target/g7/` (discovery-refreshed,
  chat-a-complete, chat-b-readback JPGs).

This closes the requested discovery repair and two-chat synthetic acceptance.
It does not establish independent conversation credentials, autonomous wake,
real image-classification quality, browser-close fault recovery or F3-R's
non-owner/independent protocol gaps. No new batch/task was created this repair;
only the four pre-existing browser fixtures were processed. TASK.md is
unchanged. No formal classification agent or 121-task run was started.

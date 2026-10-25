# G7 trusted logical workers

G7 accepts cooperative declarations under one authenticated OAuth owner. It
supersedes G6's per-conversation anti-impersonation gate for this experiment.
A four-hex worker_id is a label, never a credential. Cloudflare Access/OAuth,
batch/tool allowlists, core fencing, private RPC keys, non-admin Runner ACLs
and fixed executable policies remain required.

| Role | worker_id | Core/result agent_id |
| --- | --- | --- |
| A0 | 0000 | emote-0000 |
| A1 | 0001 | emote-0001 |
| A2 | 0002 | emote-0002 |
| A3 | 0003 | emote-0003 |
| A4 | 0004 | emote-0004 |
| A5 | 0005 | emote-0005 |

Input is exactly four ASCII hex characters, normalized to uppercase. Old A0–A5
results remain accepted for compatibility. New private attempt paths use
workers/XXXX/attempts/<task-uuid>/<attempt-no>/ under the approved workspace.

Each logical call includes worker_id and batch_id, including scoped get/status
reads. The facade authenticates OAuth first, resolves the configured client,
batch, worker and tool binding, then signs the complete RPC and derives its core
agent ID. Sidecar and Windows Relay reverify signature, scope, batch and expiry.
Core still requires the exact owner, attempt and live lease token. The signature
authenticates the trusted relay, not the ChatGPT conversation. Another conversation
of the same OAuth owner may impersonate a label; this is the accepted G7 risk.

The optional version-2 policy has mode=logical and top-level enabled,
signing_key_file, legacy_client_sha256 and bindings. Each binding contains id,
client_sha256, worker_ids, batch_id, scopes, not_before and expires_at. Times are
Unix milliseconds, binding TTL is at most 24 hours and signed request TTL at most
60 seconds. Cross-host verification allows at most five seconds of future
issued_at clock skew; expiration and the 60-second signed lifetime stay strict.
The client hash comes from validated OAuth context, not a tool
argument. Token rotation does not require six independently pinned credentials.
Version-1 strong token policies remain unchanged. Without an enabled logical
policy, logical declarations fail closed. Policy/key files are administrator
owned and every hop receives equivalent policy and signing material.

Pinned legacy clients retain existing tool arguments and coord-relay-agent for
old tasks. Unsigned requests cannot claim/read a protected batch; exact task
lookup checks immutable batch membership first. Get/status remove lease tokens,
and journals cache neither request nor result bodies. On uncertain mutations,
inspect state before deciding; never automatically replay them.

Use agents/A0.md through A5.md only for an approved canary. On empty, the
coord_agent_status.batch_progress snapshot supplies authoritative batch status
and counts. Empty claims do not prove completion. No active turn means no
renewal: leases are <=600 seconds, attempts <=3,600 seconds, with at most three
execution attempts. There is no background-wake or 24-hour autonomy promise.

Actual deployment evidence belongs in ../verification/PHASE_G7.md. These packets
do not authorize formal classification tasks or production classification agents.

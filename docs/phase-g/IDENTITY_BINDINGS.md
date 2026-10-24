# Protected OAuth connector bindings (G6)

Status: local implementation and isolated synthetic tests only. Production is
disabled. A bearer credential identifies whoever can use that credential; it
does not identify a ChatGPT conversation. No request parameter, prompt label,
browser page, OAuth owner, or MCP session header supplies a trusted worker ID.

The [official OpenAI authentication documentation](https://developers.openai.com/plugins/build/auth)
states that ChatGPT reuses a DCR client for a connection. It does not promise
one OAuth client/token per conversation. Distinct connected accounts selectable
from different conversations also do not establish isolation. Before production
enablement, prove that each intended conversation cannot select or reuse any
other worker's connection. If the platform shares these credentials, Gate A is
BLOCKED even when the binding implementation passes every local test.

## Operator configuration

`docs/examples/identity-policy.example.json` is deliberately disabled. No
configuration keeps all existing fixed `coord-relay-agent` tools and leases.
When enabled, a policy contains administrator-only `bindings` entries:

```json
{
  "id": "canary-A0",
  "client_sha256": "SHA256_OF_AUTHENTICATED_CLIENT_ID",
  "token_sha256": "SHA256_OF_AUTHENTICATED_ACCESS_TOKEN",
  "agent_id": "A0",
  "batch_id": "ISOLATED_CANARY_BATCH_UUID",
  "scopes": ["coord:read", "coord:claim", "coord:progress", "coord:extend", "coord:submit", "coord:failure"],
  "not_before": 0,
  "expires_at": 1
}
```

Replace the illustrative fields with authenticated observations and UTC epoch
milliseconds. Each interval must be positive and at most 24 hours; operational
canaries should use a much shorter interval. Agent IDs are A0–A5. IDs, clients,
token digests and worker roles must be unique. The same client cannot be both
a new binding and a legacy client. Pinning client plus token intentionally
fails closed after access-token rotation: the operator must verify and replace
the token digest and restart the isolated components. Refresh-token family is
not available in this deployed host's authenticated access-token context.

`legacy_client_sha256` must explicitly list the existing legacy connector's
client digest before a mixed deployment. Its requests keep the original worker
identity. While policy is enabled, legacy reads and mutations cannot access any
batch reserved by a binding; its global list filters those batches and legacy
scheduler claims must specify a batch. A bound
worker has no global list tool authority and must use exact task reads or
batch-specific status. Claims return only the authenticated worker's own lease;
bound read responses strip all lease-token fields. Core agent/token fencing
still decides ownership atomically, including accepted receipt reconciliation.

The 32-byte random request-signing key is a private 64-character lowercase hex
file shared only by the three trusted services. Protect the policy and key
against Agent workspaces and low-privilege Action Runner access using actual
host permissions. Do not put either file, bearer values, refresh tokens or lease
tokens into prompts, shared workspaces, Git, tool results or logs. Their ACLs
have not been verified in production.

Use `COORD_MCP_IDENTITY_POLICY` (or registration `identityPolicyFile`) on the
Feathers host and `identityPolicyFile` in the Sidecar and Windows Relay Agent
configuration. Use the same binding policy and key, with host-local absolute
key-file paths. Do not enable only one hop: each Relay hop independently checks
the signature, role, batch, scope and expiry. A signed request binds the exact
UUID, method and parameters to a context lasting no more than 60 seconds. The
persistent Sidecar request journal prevents redispatch of the same UUID across
restart and includes the signed identity in its keyed request hash; journal
records contain neither the identity proof nor task/result/lease-token bodies.
This is request replay protection, not a claim that stolen OAuth bearer tokens
are impossible to reuse. Already-dispatched requests may finish within their
bounded timeout; configuration expiry is not process cancellation.

With G6-B's external Action Runner enabled, the core's main RPC requires the
separate `external_runner.daemon_rpc_key_file` credential. Configure the Windows
Relay Agent's `coordAuthKeyFile` with that absolute protected file path; the
trimmed file must be a 64-character hexadecimal secret. This main-RPC key is
independent of the Runner broker key and the Relay identity-signing key. Only
the daemon, trusted operator/CLI and Relay Agent may read it. The low-privilege
Runner must be denied access, or it could bypass the restricted broker through
the ordinary core API. The Agent sends the key only to its strict IPv4 loopback
core URL and rejects redirects. No key is included in the policy, MCP arguments,
task payloads, journal or logs. These controls need actual production ACL and
host tests before deployment; local header/redirect tests do not establish host
isolation.

## Isolated real-connection acceptance

1. Prepare a separate development MCP endpoint, OAuth state and database, with
   an isolated test batch and harmless test images. This is an operator setup;
   no production endpoint, queue, Access policy or classification task needs to
   be changed for an isolated exercise. Keep actions disabled. Do not register
   a guessed public URL or expose an unauthenticated daemon.
2. In ChatGPT settings, add two independently registered MCP connector/app
   instances with OAuth pointing to the development endpoint, and complete each
   visible authorization. Use separate names for the test roles. Never paste a
   token into either conversation. If settings support only globally selectable
   accounts, record that limitation and stop the security acceptance.
3. On the OAuth host, run `node audit-oauth-store.cjs /absolute/oauth.json` as
   the operator to inspect counts, field names and only live client/token SHA-256
   pairs and times. Match one fresh authorization at a time. Assign the pairs to
   A0/A1 in a private policy; no pair may be shared. Missing bearer material
   cannot be recovered from SHA-256 store keys and must not trigger credential
   harvesting or unauthorized token refresh.
4. Load the private policy on the three isolated services. In two new ChatGPT
   conversations, select only the assigned connection. Read the test task and
   execute claim → view the harmless image → progress → extend → submit → next
   claim. Save redacted tool outcomes, exact task/attempt IDs and timestamps.
5. Refresh/reconnect both conversations. Attempt choosing the other connection,
   wrong batch, an injected `agent_id`, expired policy, an old request UUID, an
   old lease and a consumed progress sequence. A conversation able to use the
   other's authenticated credential fails Gate A even if Rust fencing correctly
   rejects its original old lease. Repeat for all six roles before production.
6. Stop progress, close/disconnect a test conversation and observe actual lease
   expiry/reclamation. A progress message alone must not change `lease_until`.
   There is no promise that ChatGPT wakes in the background. Each lease is at
   most 600 seconds; the default attempt budget is 3,600 seconds and retry
   maximum remains three for the intended batch. For an uncertain mutation,
   read the exact task/attempt first; do not automatically reissue effects.

No six-role production test, production canary, formal 121-task batch or
classification Agent is authorized by this runbook alone. G6-D still requires
all real A/B security gates and the recorded deployment/rollback checks.

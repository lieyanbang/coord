# Phase G1: Agent Identity and Authorization

Result: **PARTIAL**. The current path does not provide six independently
authenticated ChatGPT conversations.

CoordMCP still maps each client to the constant `coord-relay-agent`; Relay
`protocol.mjs` enforces that same identity. Cloudflare Access authenticates the
same owner account and does not identify which ChatGPT conversation made a
request. A client label `A0`-`A5` is not a credential. The MCP schema rejects an
injected `agent_id` on claim and submit, and the Relay rejects a non-constant
agent ID, but this prevents spoofing a new label rather than creating six
separate principals.

The six-client test in `tests/agent_progress.rs` sends six synthetic IDs to a
temporary coord daemon and verifies unique claims plus cross-ID lease fencing.
It proves scheduler concurrency only. It does not exercise six ChatGPT Web
sessions, capability issuance, conversation-bound persistence, or production
wrong-identity denial. No capability was placed in a prompt, shared workspace,
or production configuration.

Local tests: `npm test --prefix relay/coord-mcp` exited **0** (12/12); Relay
protocol suite exited **0** (13/13). The relevant checks assert that callers
cannot add `agent_id: A0/A1` to the strict MCP tool schema. Production OAuth
hash read-back is recorded in `PHASE_G0.md`; it does not establish worker
identity.

Gate remains open until the MCP host can supply an independently protected,
conversation-bound credential or equivalent server-issued context, and
positive/negative tests prove A0 cannot act as A1, across batches, sessions,
expiry, replay and reconnect. Production Agent conversations and task claims
remain **NOT_RUN**.

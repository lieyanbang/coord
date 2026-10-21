# Phase F2: Tailscale Private Relay

Date: 2026-10-22 (Australia/Brisbane)
Result: **PARTIAL**. The isolated sidecar, strict application boundary,
production round-trip, recovery semantics, and regression tests pass. The
tailnet control-plane ACL is not readable from either node; a live probe also
showed BT-7274 can establish TCP to 13012, although the sidecar rejects it
before sending an authentication challenge. F2 is not closed and F3 has not
started pending the ACL decision.

## Scope and Architecture

- All remote checks/deployment used direct SSH; no MCP tools were used.
- Production coord remains on Arasaka at `127.0.0.1:7777`; it is not exposed
  on the relay host or a non-loopback Arasaka interface.
- The new, separate Linux sidecar runs at
  `100.116.134.114:13012/tcp`. Arasaka's Servy-managed `CoordRelayAgent`
  initiates the authenticated outbound connection. No FeathersJS, OpenResty,
  DesktopCommanderMCP, or existing 13011 configuration was changed.
- Sidecar API is bound only to `127.0.0.1:13013`. It uses a separate 32-byte
  bearer credential from the agent's 32-byte HMAC-SHA256 challenge credential.
  Credentials remain in the existing protected host directories and are not
  copied into the repository or evidence.
- Allowed methods are `tasks/list`, `tasks/get`, `agents/heartbeat`,
  `tasks/claim`, `tasks/extend`, `tasks/complete`, `scheduler/claim`,
  `scheduler/extend`, `scheduler/submit`, and `scheduler/failure`, each with
  exact schemas and fixed agent identity. `tasks/list` requires
  `skip_reclaim: true`. In particular, task creation/cancel and arbitrary RPC
  forwarding are denied.
- Requests/results are bounded to 64 KiB, frames to 96 KiB, concurrency to
  four, and calls have bounded timeouts. A dispatched call is not retried.
  Timeout, connector loss, or restart reports `uncertain`; lease-token fencing
  remains authoritative in coord.
- Journal fingerprints are HMAC-SHA256 keyed by the client credential. The
  journal omits parameters, results, bearer values, and lease tokens. Reused
  request IDs are never dispatched again; in-flight duplicate IDs are rejected
  as `uncertain`. Restart converts nonterminal journal records to `uncertain`.

## Local Gate and Regression Evidence

- `node --test --test-name-pattern="concurrent duplicate request ids" relay/relay.test.mjs`
  at `2026-10-21T14:32:42Z` first reproduced a race: exit 1, seven `dispatched` records for eight
  simultaneous requests sharing one ID. After checking `pending` by request ID
  before creating a second pending record, the same command at
  `2026-10-21T14:33:07Z` exited 0: one dispatch, one
  successful response, seven `uncertain` responses, and one coord-side call.
- `node --test --test-name-pattern="journal fingerprints are keyed HMACs" relay/relay.test.mjs`
  exited 0. It computes the expected HMAC from the test credential, verifies
  the journal fingerprint, and asserts parameters are absent.
- `node --test relay/relay.test.mjs relay/live-coord.test.mjs` at
  `2026-10-21T14:46:33Z` exited 0: 12 tests passed, zero failed or skipped.
  Coverage includes authenticated/invalid agents and clients, whitelist/schema
  rejection, payload and concurrency bounds, peer rejection before challenge,
  reconnect, timeout/late response, restart uncertainty, duplicate IDs,
  HMAC journal, token redaction, and real isolated release-coord lease fencing
  and submit replay. Tests use temporary state and do not touch production DB.
- `pwsh.exe -NoProfile -File scripts/verify.ps1 -Stage f2` at
  `2026-10-21T14:46:42Z` exited 0. `cargo fmt --all -- --check`, locked
  clippy, locked all-feature tests, locked release build, and relay tests each
  exited 0. Exact stage times and logs are in `stage-f2-checks.json` and
  `stage-f2-*.log`; the final relay test output is
  `stage-f2-relay-tests.log`.

## Fork Windows Actions

- Commit `41aa36d2054f2f520972c6cb2225362fc41739dc` triggered Actions run
  [39620800985](https://github.com/lieyanbang/coord/actions/runs/39620800985)
  at `2026-10-21T14:53:14Z`; it completed at `14:56:25Z` with conclusion
  `success`.
- Job `test (windows-2022)` and all steps succeeded, including fmt, clippy,
  tests, release build, `relay protocol and lease-fencing tests`, and artifact
  upload. Artifact `coord-windows-2022` (`12355300591`) is 4,226,358 bytes and
  has not expired.

## Production Deployment and Health

At `2026-10-21T14:43:45Z`, the existing sidecar health was `READY`, the Arasaka
agent was connected, and there were zero pending calls. The candidate
`relay/server.mjs` was copied to a unique `/tmp` path and SHA-256 checked before
installation. `node --check --input-type=module < <staged-file>` exited 0. A
filename-extension-only probe (`node --check <staged-file-with-.next-suffix>`)
returned `ERR_UNKNOWN_FILE_EXTENSION`; no source change followed from that
probe.

The previous source hash was
`1d8388ad7a2514cf02fad08ad737b4640b94fb62c917c12c92424f879d561573`.
The new deployed `server.mjs` SHA-256 is
`3351081c4d2ef9ba7f166822dc48e5a1bf6b8fe18e1ff36fac11a051e98dcf84` and
matches the tested workspace file. `protocol.mjs` and `agent.mjs` also match
their workspace SHA-256 values (`b0f262091f98cab400ce3254713460301964c093431c0419728a6860df3f6f9a` and
`9070da2bc597152ed81c43535ea7df21d07941fe18216fe39a96edc2774cdc68`). The
Arasaka copies of `protocol.mjs` and `agent.mjs` match those same hashes.

The update used an atomic same-directory rename and restarted only
`coord-relay.service`. The prior server file is preserved at
`/var/backups/coord-relay/server.mjs.20261021T144345Z`. At
`2026-10-21T14:43:47Z`, the service was active under UID/GID 991
(`coord-relay`), PID 81799; health returned `READY`, `agentConnected=true`,
`pending=0`. No rollback was required. The service has `NoNewPrivileges=yes`,
empty capability sets, `PrivateTmp=yes`, `ProtectSystem=strict`,
`ProtectHome=yes`, `UMask=0077`, and writes only to `/var/lib/coord-relay`.
Configuration and separate secret files remain under `/etc/coord-relay`.

At `2026-10-21T14:44:42Z`, direct checks returned exit 0 for
`coord-relay.service`, `feathersjsapp.service`, `openresty.service`, and
`tailscaled.service` active. Listeners were `100.116.134.114:13011`,
`100.116.134.114:13012`, and `127.0.0.1:13013`; 13011 still had an established
Arasaka connection. The new 13012 listener was owned by the isolated relay
unit. The narrow UFW entry remained present; its effective behavior is
discussed below.

At `2026-10-21T14:44:59Z`, Arasaka's Windows boot time was unchanged at
`2026-10-21T06:35:49.8354060Z`. `Servy`, `CoordRelayAgent`,
`ArasakaDDNS`, `ArasakaFFmpegWatchdog`, `ArasakaFFmpegWorker`, `MediaSTT`,
`OCI-A1-Grabber`, and `CodexProxy` were all Running/Automatic. Servy
`status --name CoordRelayAgent --quiet` and `show --name CoordRelayAgent --quiet`
both exited 0; the entry remained Running as `NT AUTHORITY\LocalService`,
PID 8460, with automatic startup and process recovery. The agent's established
socket was from `100.89.6.126` to relay port 13012. Coord continued listening
only on `127.0.0.1:7777`.

At `2026-10-21T14:52:05Z`, the production round-trip, executed as the dedicated
`coord-relay` identity, returned HTTP 200 / `state=ok` for `tasks/list` with
`skip_reclaim=true` and one result. Health was READY with the real Arasaka
connector. Negative checks
returned 401 for an invalid bearer, 400 for `tasks/send`, and 400 for
`tasks/list` with `skip_reclaim=false`; those rejected calls did not grow the
journal. `127.0.0.1:13013` was not reachable through the relay's Tailscale
address. No production task was claimed, completed, or mutated by these probes.

## Tailscale ACL and Effective Source Boundary

- At `2026-10-21T14:49:59Z`, read-only `tailscale status` listed Arasaka `100.89.6.126` and BT-7274
  `100.124.104.23` as active peers. The node-side Tailscale CLI does not expose
  the administrator ACL policy, so the configured policy itself is
  **UNVERIFIED**; no tailnet policy was changed.
- `ufw status numbered` showed rule 48 allowing only Arasaka to
  `100.116.134.114:13012/tcp` on `tailscale0`. However, `iptables -S INPUT`
  showed `ts-input` before UFW, and `ts-input` contains `-i tailscale0 -j
  ACCEPT`. Therefore the UFW entry is not an effective Tailscale source filter
  on this host and is not counted as a passing boundary.
- At `2026-10-21T14:50:05Z`, from BT-7274, TCP connect to 13012 succeeded, but reading the application
  socket returned EOF with zero bytes and no challenge. The sidecar checks the
  peer IP against the single configured Arasaka address before sending a
  challenge; invalid credentials and wrong peer identity are independently
  rejected. This proves application-level rejection, not network-layer ACL
  isolation.
- A narrowly scoped ACL change was requested for authorization. Until the
  Tailscale policy is independently inspected/updated and the non-Arasaka TCP
  probe is blocked, the control-plane ACL gate remains open and F2 stays
  PARTIAL. The request does not authorize changes to any other Tailscale grant.

## Commands and Outcomes

| Check | Command / method | Outcome |
| --- | --- | --- |
| Relay suite | `node --test relay/relay.test.mjs relay/live-coord.test.mjs` | exit 0; 12/12 |
| Windows stage | `pwsh.exe -NoProfile -File scripts/verify.ps1 -Stage f2` | exit 0; all five checks |
| Fork Actions | Windows run `39620800985` for `41aa36d` | success; all steps and artifact upload passed |
| Arasaka service state | `servy-cli.exe status/show --name CoordRelayAgent --quiet` over SSH | both exit 0; Running / LocalService |
| Sidecar deployment | hash-verified staged file, atomic install, `systemctl restart coord-relay.service` | SSH script exit 0; READY and agent reconnected |
| Production RPC | authenticated `tasks/list`, `skip_reclaim=true` | HTTP 200, `state=ok` |
| Production negative RPC | bad bearer, `tasks/send`, reclaim-enabled list | 401 / 400 / 400; no journal dispatch |
| Existing channel regression | `ss` listener and established connection checks | 13011 remained active with Arasaka; shared services active |
| Non-Arasaka source | BT-7274 TCP + application read | TCP connected, zero bytes/no challenge; app rejected; Tailscale ACL unresolved |

F3 remains **TODO**. `TASK.md` was not changed. Existing production services and
port 13011 were not restarted or reconfigured; Arasaka was not rebooted.

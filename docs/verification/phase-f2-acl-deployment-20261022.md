# Phase F2 ACL Deployment Evidence

Date: 2026-10-22 (Australia/Brisbane); remote event timestamps are UTC.
Host: `instance-20260424-1840`, Tailscale IPv4 `100.116.134.114`.

## Policy and Scope

The Tailscale admin JSON editor was inspected read-only. Its grants include a
wide `src: ["*"]` to `dst: ["100.64.0.0/10", "fd7a:115c:a1e0::/48"]` with
`ip: ["*"]`, as well as a separate 13011 grant for Arasaka and BT-7274. Grants
are additive, so adding an Arasaka allow would not deny BT-7274. No Tailscale
policy grant was changed. The existing UFW rule 48 for Arasaka/13012 remains,
but is ineffective as a source boundary because `INPUT` jumps to `ts-input`
first and `ts-input` accepts `tailscale0`.

The deployed rule is a separate `inet coord_relay_acl` input base chain at
priority `-10`, before the iptables-nft filter hook. It drops only IPv4 TCP
packets arriving on `tailscale0` for `100.116.134.114:13012` unless the source
is Arasaka `100.89.6.126`. No other destination, port, Tailscale grant, UFW
rule, or INPUT/ts-input rule was changed.

## Backup and Preflight

At `2026-10-21T15:57:01Z`, direct SSH read-only checks exited 0. They showed
`INPUT` policy DROP, `-A INPUT -j ts-input` before UFW, and
`-A ts-input -i tailscale0 -j ACCEPT`. `nftables.service` was inactive and
disabled; `/etc/nftables.conf` contains `flush ruleset`, so it was not enabled
or reloaded. `tailscaled`, `coord-relay`, `feathersjsapp`, and `openresty` were
active. At `15:59:05Z`, `sudo -n id` exited 0 and confirmed root via the
existing non-interactive sudo authorization.

Before any rule change, snapshots were saved under
`/var/backups/coord-relay-acl/20261021T155949Z/`:

| Snapshot | SHA-256 |
| --- | --- |
| `nft-ruleset.before` | `fb4b2657f1ab9cd3c36c668921623be5d1376245076a79efd35d2da1c2285950` |
| `iptables-save.before` | `6c88dd6df275101551a33dd6b4a6f00143267a3e6c6a9875d80e2c3add283864` |
| `ufw-status.before` | `0c0bb7adcc20fa2d648b6f2691bd0468349015a518b6f4d62417b00ac4fdec66` |
| `nftables.conf.before` | `60dac93ffe0ea440fc4a8941a080b6fb8d2c8655d47baf856e97182a0d1ca29a` |

The first backup shell wrapper exited 1 because Windows-to-Bash CRLF left a
carriage return on its final wildcard argument to `sha256sum`. All four
snapshot files were present; a corrected explicit-path `sha256sum` command
exited 0 and produced the hashes above. No live firewall rule had yet been
changed.

Preflight checks exited 0 for `nft --check --file` and
`systemd-analyze verify`. The first `systemd-analyze verify` invocation used a
temporary filename without a `.service` suffix and exited 1 with
`Invalid argument`; copying the same candidate to
`/tmp/coord-relay-acl-candidate.service` and rerunning exited 0. `sh -n` on
the candidate verifier exited 0.

Immediately before the temporary filter was loaded, BT-7274 reported
`tailscale ip -4 = 100.124.104.23`; a fresh `TcpClient.Connect` to port 13012
succeeded with process exit 0. It ran after the `15:59:05Z` host preflight and
before the `15:59:49Z` backup, confirming the prior open network path.

## Temporary Rule and Live Tests

At `2026-10-21T16:03:26Z`, loading the exact candidate with
`sudo nft --file /tmp/coord-relay-acl.nft.candidate.b935e786` exited 0. The
live chain showed input priority `filter - 10` (numeric `-10`) and the exact
interface/destination/port/source match; the counter started at zero.

After that temporary rule:

- BT-7274's fresh `TcpClient.ConnectAsync(...).Wait(4000)` timed out; the
  expected-denial probe exited 0. At `16:07:41Z`, the exact drop counter was
  3 packets / 156 bytes.
- Arasaka's fresh TCP connect to `100.116.134.114:13012` succeeded. Its
  Servy-managed `CoordRelayAgent` remained connected, and the sidecar health
  endpoint reported `READY`, `agentConnected=true`, and `pending=0`.
- An authenticated read-only `tasks/list` request through the relay to
  Arasaka coord returned HTTP 200, `state=ok`, and one result. It used
  `skip_reclaim=true`; no production task was claimed or mutated.

At `16:07:41Z`, the same read also showed `tailscaled`, `coord-relay`,
`feathersjsapp`, and `openresty` active. Port 13011 remained established with
both Arasaka and BT-7274; port 13012 remained established from Arasaka. No
existing service was restarted.

## Persistent Install and Final State

At `2026-10-21T16:09:39Z`, the verified files were installed and only the new
`coord-relay-acl.service` was started/enabled. The unit is enabled through
`network-pre.target`, ordered `Before=network-pre.target tailscaled.service`,
and loads the separate `inet` table. Its `ExecStop` deletes only that table.
The nft rules file uses `destroy table` plus the table definition in one
netlink batch, so a reapply atomically replaces only this table.

Deployed SHA-256 values matched the workspace files:

| Deployed file | SHA-256 |
| --- | --- |
| `/etc/coord-relay-acl/coord-relay-acl.nft` | `b935e786a35fbeb6d90cb9b9b0c1a95d69f961d8e61b3dfae5bb94885c9f7bd5` |
| `/etc/systemd/system/coord-relay-acl.service` | `b1b93ff913bafca5904aab2078fa2ac4c23bc5417f707931bef4f8cb7ec00ace` |
| `/usr/local/sbin/coord-relay-acl-verify` | `2d645f6102829508d8f435ec4e7321a3757bcc7b68898a844cfe36ac6ebb57c4` |

At `2026-10-21T16:14:18Z`, `/usr/local/sbin/coord-relay-acl-verify` exited 0
and printed `host_acl=PASS`. `coord-relay-acl.service` was active/enabled;
`coord-relay.service` remained active with its original PID 81799. The four
existing services remained active. The nft counter was 3 packets / 156 bytes.
The Arasaka `CoordRelayAgent` was independently read as Running/Automatic,
PID 8104; a new Arasaka TCP connect passed at `16:14:35.8171624Z` (exit 0).
An authenticated relay `tasks/list` then returned HTTP 200, `state=ok`, and
one result at `16:14:29.022Z` (exit 0).

For the persistent-rule negative test, the counter was 3 at `16:14:52Z`.
BT-7274 (`100.124.104.23`) started a fresh TCP connect at
`16:14:58.3774636Z`; it timed out after four seconds and the expected-denial
probe exited 0. At `16:15:08Z`, the counter was 6 packets / 312 bytes, showing
the new SYN traffic hit the early drop rule. The host verifier again exited 0
with `host_acl=PASS` and Arasaka health `READY`.

The current Tailscale-generated `INPUT -> ts-input -> ACCEPT tailscale0` path
was left active during the real negative probe; the separate `-10` base chain
blocked BT before that filter hook. Systemd ordering and table isolation were
verified without restarting `tailscaled`; no Tailnet connections or 13011
sessions were interrupted. A full server reboot and Tailscale service restart
were not run. The persistent boot order is configured and inspected, but this
evidence does not claim a post-reboot test.

The exact deployment files, verifier, and scoped rollback procedure are in
`relay/host-acl/`. The closeout commit's local Windows gate and Fork Actions
run are tracked separately in `PHASE_F2.md` after they complete.

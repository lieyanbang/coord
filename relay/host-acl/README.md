# Relay Host ACL

This host-level filter is limited to IPv4 TCP ingress on `tailscale0` to the
coord relay `100.116.134.114:13012`. It drops every source except Arasaka
`100.89.6.126`. It runs at nftables input priority `-10`, before the
iptables-nft filter hook where Tailscale's generated `ts-input` chain accepts
Tailnet traffic. It does not edit Tailscale grants, UFW rules, port 13011, SSH,
or other services.

The Tailnet currently has a broad `src: ["*"]` grant to the Tailnet address
ranges. Tailscale grants are additive, so an extra allow rule would not deny
BT-7274. Keep that broad grant unchanged; this dedicated host table enforces
the source restriction for only the relay port.

## Install

Copy the three files in this directory to the relay host, then install them as
root:

```sh
install -d -m 0755 /etc/coord-relay-acl
install -m 0644 coord-relay-acl.nft /etc/coord-relay-acl/coord-relay-acl.nft
install -m 0644 coord-relay-acl.service /etc/systemd/system/coord-relay-acl.service
install -m 0755 verify.sh /usr/local/sbin/coord-relay-acl-verify
systemctl daemon-reload
systemctl enable --now coord-relay-acl.service
/usr/local/sbin/coord-relay-acl-verify
```

`nft -f` applies the `destroy` plus table definition in one netlink batch. That
makes repeated unit starts atomic and replaces only `inet coord_relay_acl`.
The unit is pulled in by `network-pre.target` and ordered before both that
target and `tailscaled.service`; the custom nftables table is separate from
Tailscale's generated iptables-nft tables and survives their rule updates.

## Verification

Run `/usr/local/sbin/coord-relay-acl-verify` on the relay host. It checks the
enabled/active unit, exact hook and source match, the shared service states,
13011/13012 listeners, and the authenticated agent's READY health signal.
From BT-7274, a fresh TCP connect to `100.116.134.114:13012` must time out or
fail; the matching nftables drop counter must increment. From Arasaka, the
Servy-managed agent must remain connected and an authenticated read-only
`tasks/list` request with `skip_reclaim=true` must still return HTTP 200 and
`state=ok`. These network probes are separate from the host-only script.

## Rollback

The rollback touches only this table and its unit:

```sh
systemctl disable --now coord-relay-acl.service
if nft list table inet coord_relay_acl; then
  echo "unexpected: coord relay ACL table remains" >&2
  exit 1
fi
systemctl is-active tailscaled coord-relay feathersjsapp openresty
```

The stop action deletes `inet coord_relay_acl` only. It does not flush the
ruleset, restart Tailscale, change UFW, or affect SSH/13011. Leave the installed
files and timestamped pre-change firewall snapshots in place for audit and
re-application with `systemctl enable --now coord-relay-acl.service`.

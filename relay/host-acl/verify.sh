#!/bin/sh
set -eu

unit=coord-relay-acl.service
table=$(nft list table inet coord_relay_acl)

systemctl is-enabled --quiet "$unit"
systemctl is-active --quiet "$unit"
printf '%s\n' "$table" | grep -F 'hook input priority filter - 10'
printf '%s\n' "$table" | grep -F 'iifname "tailscale0" ip daddr 100.116.134.114 tcp dport 13012 ip saddr != 100.89.6.126 counter packets'

for service in tailscaled coord-relay feathersjsapp openresty; do
	systemctl is-active --quiet "$service"
done

listeners=$(ss -Hlnpt)
printf '%s\n' "$listeners" | grep -F '100.116.134.114:13011'
printf '%s\n' "$listeners" | grep -F '100.116.134.114:13012'
curl --silent --show-error --fail http://127.0.0.1:13013/healthz | grep -F '"agentConnected":true'

printf 'host_acl=PASS\n'

# Phase F2: Tailscale Private Relay

Date: 2026-10-21 (Australia/Brisbane)
Result: **IN_PROGRESS (read-only protocol and dependency audit only)**.
F1 is closed. No F2 relay has been implemented, deployed, or accepted yet.
No production relay configuration, listener, or service was changed in this
initial F2 work.

## Existing Reference Protocol

- The read-only reference workspace is
  `C:\Users\leiya\Desktop\project\DesktopCommanderMCP`, HEAD
  `c95a0be` (`fix: prevent duplicate remote startup and normalize Windows output`).
  Its worktree already contains tracked modifications and untracked files,
  including `server/`, deployment scripts, and a `TASK.md`; this task changed
  none of them.
- `src/remote-device/remote-channel.ts` uses `socket.io-client` 4.8.3, websocket
  transport only, namespace `/desktop-commander`, and path
  `/desktop-commander/socket.io`. Device identity is sent in the Socket.IO auth
  handshake as `device` plus `token`; after connect, `hello` identifies protocol
  version 1 and runtime metadata. Calls and responses use `call`, `result`, and
  `error` message types.
- `server/desktop-commander/device-manager.cjs` authenticates enabled devices
  with `crypto.timingSafeEqual`, enforces hello identity/version, limits each
  device to 64 pending calls, and applies a configured call timeout. Its generic
  `toolName` dispatch is intentionally **not** an acceptable coord interface.
  The coord relay must use a distinct identity and namespace/path or a separate
  service, and allow only explicitly enumerated `coord_rpc` methods and schemas.
- `docs/API.md` documents coord HTTP JSON-RPC as unauthenticated and intended
  for trusted local clients. Therefore the coord daemon must remain loopback-only;
  an authenticated relay must validate every method, parameter shape, payload
  size, concurrency, and timeout before calling it. Lease tokens and task
  payloads must not appear in logs.

## Relay-Side Preflight Snapshot

The earlier same-task, read-only preflight of `instance-20260424-1840` recorded
Tailscale, Feathers, and OpenResty active; Tailscale address `100.116.134.114`;
Feathers backend port `13010` bound to loopback; and the existing DesktopCommander
channel on `100.116.134.114:13011`. The earlier preflight also observed the
Arasaka DesktopCommander Agent task and an established connection to port 13011.
An established TCP connection does not prove a valid hello/heartbeat. These are
historical preflight observations, not a new live F2 acceptance probe; the exact
observation timestamps were not retained in this closeout.

Existing namespace `/desktop-commander`, devices BT-7274, 1840, and Arasaka, and
the current Arasaka Agent must remain untouched. The DesktopCommander workspace
is dirty, so integration must be additive and limited to explicitly owned files
after the startup hook and rollback boundary are understood.

## Live Relay Host Read-only Audit

At `2026-10-21T12:55Z`-`12:56Z`, direct SSH read-only checks (no MCP) recorded:

- `sudo -n systemctl show feathersjsapp.service --property=FragmentPath --property=User --property=Group --property=MainPID --property=WorkingDirectory --property=ExecStart --property=ActiveState --property=SubState` exited 0. `feathersjsapp.service` is active/running, PID 816, working directory `/opt/app/feathers`, entrypoint `/usr/bin/node /opt/app/feathers/index.js`, unit `/etc/systemd/system/feathersjsapp.service`. `User` and `Group` are unset; `ps -o uid=,gid=,pid=,comm= -p 816` exited 0 and confirmed UID/GID 0.
- `sudo -n grep -nE 'fileshareModule|registerFileshare|resolveListenPort|createServer|serviceHandles' /opt/app/feathers/index.js` exited 0. The active bootstrap declares the DesktopCommander module path at line 6, requires `registerDesktopCommander` at line 17, creates its handle at line 91, and calls `server.listen` at line 92. The service shares this HTTP server with existing Feathers modules. Adding a namespace there would require a Feathers process restart and carries broader impact, so it is not the initial F2 integration choice.
- `systemctl is-active tailscaled` exited 0 (`active`); `ip -4 -brief addr show dev tailscale0` reported `100.116.134.114/32`. `ss -ltnp` exited 0 and showed `127.0.0.1:13010` owned by PID 816 and `100.116.134.114:13011` owned by Nginx. A read-only listener check exited 0 and found no listener on candidate `100.116.134.114:13012`.
- The installed Socket.IO server package is `4.8.1`. Remote file hashes: `/opt/app/feathers/index.js` `057a41549b669406d045d7c017e59e43d37056fd1752239e8f07c0ee6862eea2`; `realtimetext/lib/desktop-commander/device-manager.cjs` `2226267e490e232dafe292b7ee552dce44a49712a0441a69663ed6368e97a2e0`; `realtimetext/lib/desktop-commander/index.cjs` `5f6f8f4dbf42cfbffbe609772a912dc84890535ab7cf731b3962b703d100e55d`. Local reference `device-manager.cjs` and `index.cjs` SHA-256 values match the installed files.
- SSH command outcomes: the remote `rg` probe exited 1 because `rg` is not installed; the first escaped `grep` pattern exited 1 with `Unmatched ( or \(`. A simpler `grep -nE` probe succeeded. An initial Node version expression returned a `require` function rather than a version despite exit 0; it was not used as evidence, and `grep -m1 version` on the installed package manifest then confirmed `4.8.1` with exit 0. These were read-only probe errors; no remote file or service changed.

Candidate direction: implement a dedicated coord relay sidecar bound only to the
Tailscale address on an unused port, with its own restricted service identity,
namespace/path, and credential. Port `13012` is only a candidate: Tailscale ACL
reachability from Arasaka, firewall behavior, service-account setup, and the
rollback package are not yet verified. Do not reload Feathers or alter the
existing DesktopCommander channel as part of F2.

## F2 Kickoff Windows Gate

- `pwsh.exe -NoProfile -File scripts/verify.ps1 -Stage f2` ran from
  `2026-10-21T12:53:30Z` to `12:54:06Z`; fmt, locked clippy, locked all-feature
  tests, and locked release build each exited 0. Details are in
  `stage-f2-checks.json` and `stage-f2-*.log`.
- Kickoff commit `ec0d3c856fe8cf7efc9cd3cee252620bf75a42c9` was pushed to the
  Fork branch. Windows Actions run `39603443737` completed from
  `12:54:47Z` to `12:58:30Z`, conclusion `success`; job `test (windows-2022)`
  and fmt, clippy, test, release build, and artifact-upload steps all succeeded.
  Read-only REST queries for run/jobs and artifacts exited 0. Artifact
  `coord-windows-2022` (`12348150026`) is present, 4,226,357 bytes, not expired;
  API archive SHA-256 is
  `f970baeae2899d6f9c2b174751d0c29032aafaebee2d07860d7a11db9f23707e`.
- After the host audit, `pwsh.exe -NoProfile -File scripts/verify.ps1 -Stage f2`
  ran from `2026-10-21T13:01:39Z` to `13:02:14Z`; fmt, locked clippy, locked
  all-feature tests, and locked release build each exited 0. The refreshed
  output is in `stage-f2-checks.json` and `stage-f2-*.log`.

## Next F2 Work

- Complete the deployment and rollback design for the isolated sidecar; verify
  Tailscale ACL reachability, candidate port availability, and a non-root service
  identity before creating any service or listener.
- Define a separate 256-bit coord identity, constant-time token verification,
  Tailscale-only reachability, and a strict `coord_rpc` allowlist. Do not forward
  generic DesktopCommander `call` messages or arbitrary JSON-RPC methods.
- Add local protocol regression tests for valid/invalid identities, wrong
  namespace, denied methods and malformed parameters, request/result limits,
  timeout and concurrency bounds, reconnect behavior, and redaction of lease
  tokens. Then test real Arasaka-to-relay calls, disconnect/reconnect, lease
  fencing, and existing DesktopCommander channel regression.
- F2 remains **NOT_ACCEPTED** until deployment and every F2 gate in `TASK.md`
  has real evidence. No remote F2 mutation or service interruption has occurred.

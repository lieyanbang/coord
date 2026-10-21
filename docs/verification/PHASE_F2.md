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

## Next F2 Work

- Finish the read-only audit of the active Feathers bootstrap, current Socket.IO
  attachment point, service ownership, and rollback procedure before selecting
  an isolated namespace/path or dedicated relay process.
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

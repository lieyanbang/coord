# Phase F0: Environment and Deployment Preflight

Date: 2026-10-21 (Australia/Brisbane)
Result: **PASS for F0 audit only**. No remote service, database, route, or process was changed.

## Repository and Artifact

- Working branch: `codex/phase-f-deployment`; initial HEAD `a32eae20ae243d56549f6a502f34f5daf90c5717`.
- `origin` is upstream `DmarshalTU/coord`; `fork` is `lieyanbang/coord`. No upstream write was made.
- No coord daemon was known at task start. Arasaka had no `C:\ProgramData\coord`, coord process, or listener in ports 7700-7800. No task/ack IDs were created.
- Windows Actions run `39436136707` on `d4f41043c75a811ab756e2bc65a816eefef823a8` completed successfully. Artifact `12272506068` is recorded in `stage-e-actions.json` with EXE SHA-256 `6884df79dff810546db84cace765991b9ebec168701136ce0fa15fe51c43d3f6`.
- Local artifact copy `target/verification/stage-e-ci/coord.exe` has the same SHA-256. Use this CI artifact for deployment; the unrelated current `target/release/coord.exe` has a different hash.

## Arasaka

Read-only access used the canonical SSH metadata in the external-context skill: `lieyanbang@arasaka.tailc57112.ts.net`, port `13988`, key path `C:\Users\leiya\.ssh\arasaka.key`. The SSH session had administrator role.

| Item | Observed state |
| --- | --- |
| OS | Windows 10 IoT Enterprise LTSC, build 19044 |
| Tailscale | `100.89.6.126`; relay peer `100.116.134.114` active |
| C: free space | 51,399,036,928 bytes |
| Servy service | `Running`, `Automatic`, `LocalSystem`; image `C:\ProgramData\Servy\Servy.Host.exe` |
| Servy CLI | `C:\Program Files\Servy\servy-cli.exe`, version `10.2.0+bc0f0c7b67862028549e8d99c0b1d16c2b58d228` |
| Servy config DB | `C:\ProgramData\Servy\db\Servy.db`; active `-wal`/`-shm` files exist |
| Coord | Install path absent; no coord process; no listener in 7700-7800 |

The Servy `Services` table was inspected through SQLite `mode=ro`; only service names and non-secret health fields were selected. The six existing entries and Windows service states are:

| Name | Windows state / start / identity | Health | Heartbeat |
| --- | --- | --- | ---: |
| `ArasakaDDNS` | Running / Automatic / LocalSystem | enabled, 3 failures | 15 s |
| `ArasakaFFmpegWatchdog` | Running / Automatic / LocalSystem | enabled, 3 failures | 10 s |
| `ArasakaFFmpegWorker` | Running / Automatic / LocalSystem | enabled, 3 failures | 10 s |
| `CodexProxy` | Stopped / Automatic / LocalSystem | disabled | 30 s |
| `MediaSTT` | Running / Automatic / `.\media-stt-svc` | disabled | 300 s |
| `OCI-A1-Grabber` | Running / Automatic / LocalSystem | enabled, 3 failures | 300 s |

The Servy CLI help confirmed read-only `status`, `show`, and `export` commands, plus targeted `uninstall --name`. Install options include `--enableHealth`, `--heartbeatInterval` (5-86400 s), `--maxFailedChecks`, `--recoveryAction` (`None`, `RestartService`, `RestartProcess`, `RestartComputer`), and bounded restart attempts. Its documented health controls are process heartbeat/recovery and an outbound diagnostic heartbeat URL; the CLI help does not expose an HTTP readiness probe for `tasks/list` or SQLite. F1 must add an independent local application health probe.

## FeathersJS and OpenResty Relay

Read-only SSH used `ubuntu@instance-20260424-1840.tailc57112.ts.net`, port `13027`, with the indexed SSH key. Installed manuals `openresty-web-architecture(7)` and `feathers-host(7)` were read before interpreting the live paths.

- `feathersjsapp.service`: enabled, active, main PID 816, entry `/opt/app/feathers/index.js`, backend on `127.0.0.1:13010`.
- `openresty.service`: enabled and active. Public listeners are `0.0.0.0:80/443`; the existing DesktopCommander private channel listens on `100.116.134.114:13011`.
- Site config: `/usr/local/openresty/nginx/conf/conf.d/lieyanbang.conf` (`root:root`, mode `0644`), SHA-256 `4c9c8ff884a004649bf8349b997cc24d441f28840a3357924c7e27e8a0e8c9a4`.
- Existing MCP paths are `/mcp`, `/app/RemoteControlMCP/`, `/app/RemoteControlMCP2/`, and `/app/RemoteControlMCP3/`; their OAuth and `.well-known` routes map to Feathers on loopback. No `/app/CoordMCP/` directive exists.
- `sudo -n /usr/local/openresty/nginx/sbin/nginx -t` exited 0. No reload or restart was performed.

## Commands and Recovery Boundary

The exact live CLI and host commands used were read-only: `servy-cli.exe --help`, `servy-cli.exe help install`, `servy-cli.exe status --name Servy --quiet`, `servy-cli.exe show --name <existing-name> --quiet`, SQLite read-only queries of `Services`, `systemctl show`, `ss -ltnp`, `sha256sum`, and `sudo -n .../nginx -t`.

## Windows Local Gate

`pwsh.exe -NoProfile -File scripts/verify.ps1 -Stage f0` exited 0. The checks JSON is `stage-f0-checks.json`: fmt 0 (no output), locked clippy 0, locked all-feature tests 0, and locked release build 0. The test log reports 35 passed, 0 failed, and 1 intentionally ignored capacity test; no tests reported a skip. The capacity suite was not part of this F0 gate.

Before F1, take a SQLite online backup of the Servy DB with Python's `sqlite3.Connection.backup()` into the dedicated coord backup directory. Do not copy the live DB file by itself because WAL/SHM files are present. Normal rollback should uninstall only the new `coord` Servy entry with the documented targeted uninstall command; do not restore the whole Servy database if other services may have changed after the snapshot.

Before F3, copy the exact OpenResty site file to a timestamped sibling backup and compare its SHA-256. Edit only the new CoordMCP routes, run `nginx -t`, then reload only after Feathers and negative/positive route checks pass. Rollback restores that exact file, verifies its hash and `nginx -t`, then reloads OpenResty. Back up only the Feathers files changed for this feature; do not replace the host tree.

Existing services, config files, OAuth stores, and credentials remain untouched. `TASK.md` remains read-only. F1-F3 have not started; each requires its own implementation, live acceptance, and stage evidence.

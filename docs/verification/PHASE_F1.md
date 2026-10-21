# Phase F1: Arasaka Service and Health

Date: 2026-10-21 (Australia/Brisbane)
Result: **IN_PROGRESS**. The deployed service and scoped recovery gates pass;
Servy-host and Windows restart tests remain NOT_RUN.

## Local Implementation and Gate

- `coord health` validates the JSON-RPC envelope and `tasks/list` response,
  opens SQLite read-only, checks schema and latency, and records READY/DEGRADED/
  FAIL without serializing task data or lease tokens. `tasks/list.skip_reclaim`
  keeps monitoring from mutating lease state.
- `scripts/windows/coord-health-monitor.ps1` triggers only Servy `coord` after
  three hard failures, with a 10-minute cooldown and at most three recoveries
  per hour. It rejects stale status files and treats slow successful probes as
  DEGRADED. Two Windows PowerShell 5.1 issues were fixed: native stderr no
  longer aborts the failure path, and Servy's prefixed `status --quiet` output
  is parsed fail-closed.
- `scripts/windows/test-coord-health-monitor.ps1` verifies the three-failure
  threshold and targeted `start` branch using a random temp directory and fake
  Servy CLI. The harness removes only its own temp directory.
- Latest `pwsh -NoProfile -File scripts/verify.ps1 -Stage f1`: fmt 0, locked
  clippy 0, locked all-feature tests 0, locked release build 0, monitor harness
  0. Evidence: `stage-f1-checks.json`, `stage-f1-*.log`, and
  `stage-f1-health-monitor.log`.
- Fork Windows Actions run `39542595503` for commit `76f9e85` completed with
  the Windows job and fmt/clippy/test/release-build/artifact-upload steps all
  successful. Artifact `coord-windows-2022` (`12319759491`, 4,226,357 bytes)
  has API digest `sha256:331baddc816c47f6e14c764d289af7a3417f0968315a85319c2506da8b6839a2`.
  This monitor/docs-only commit did not change the deployed Rust executable;
  the installed EXE hash remains independently verified above.

## Arasaka Deployment

- Connected through the indexed direct SSH workflow; no MCP was used.
- Deployed `coord.exe` SHA-256:
  `b58d3620b1b6de8035bfcb97e99f2e4fe654f48b1cb5719ef9fca013f5652e0d`.
- Servy service `coord` is Running/Automatic as `NT AUTHORITY\LocalService`.
  The recovered process ran BelowNormal. Production listener is only
  `127.0.0.1:7777`; test port 7778 was released after the isolated daemon test.
- SYSTEM task `coord-health-monitor` is Ready with boot and one-minute triggers,
  `LastTaskResult=0`, and highest run level. The installed monitor SHA-256 is
  `49ef870c3c9c706fd761cbdef5fdc7737471f4c56bbe651a7833e8d549a6056a`.
- Servy reports process health enabled, 10-second heartbeat, `RestartProcess`,
  and size rotation enabled at 10 MB (date rotation disabled). A pre-update
  monitor copy is retained at
  `C:\ProgramData\coord\backup\coord-health-monitor.pre-servy-status-fix-20261021.ps1`
  with SHA-256 `bd6d0ed4ed074056026bfc64e2cbcdf0fa7301cd12f33350d695d73afbd38320`.

## Recovery Evidence

- Live database backup:
  `C:\ProgramData\coord\backup\coord-live-pre-failure-20261021.db`, 61,440
  bytes, SHA-256
  `6744ae421cff06b4fa4ab423a2a7777657bc7f046140659ab56d5d136c08dba5`.
  The verified helper `sqlite-online-backup-v3.py` SHA-256 is
  `a434f33e94d1a46277704dde0ce4da932febeb003b7e44ea5ec38f9dc7b87e34`.
- Created and re-verified an isolated restore copy at
  `C:\ProgramData\coord\data\verification-20261021\coord-restored.db`.
  Both backup and restore checks exited 0: `quick_check=ok`, four user tables,
  matching row counts, and matching SHA-256. The restored DB opened under coord;
  JSON-RPC and read-only SQLite health returned READY on loopback port 7778.
- WAL/crash/lease rehearsal used only that restore copy. Isolated task
  `4afa7827-3743-4f7d-b38d-5a29f10093ef` was claimed by agent A (attempt 1); a
  127,752-byte WAL was present immediately before force-stopping only the
  verified test PID. After daemon restart, health returned READY and agent B
  reclaimed the same task at attempt 2. Token values were not logged. The
  isolated daemon was stopped and port 7778 released.
- Servy process recovery: force-stopped only the verified production coord PID
  `10460` after confirming its executable path and loopback listener. Servy
  relaunched coord as PID `9764`; the service remained Running, listener stayed
  loopback-only, priority was BelowNormal, health returned READY, and the
  application recovery record did not change.
- Health-monitor recovery: after `servy-cli stop --name coord --quiet` exited 0,
  three monitor invocations each returned the expected probe-failure exit 1.
  The monitor called only `start --name coord --quiet`, recorded `action=start`
  and exit 0, and restored the service as PID `9604`; health returned READY in
  4 ms, BelowNormal, on `127.0.0.1:7777`. No production queue task was created.
- A remote PowerShell 5.1 isolated test also verified the deployed monitor's
  three-failure branch against a fake Servy CLI that emits the real prefixed
  status format; calls were exactly `status coord` then `start coord`.

## Remaining Gate

- **NOT_RUN:** restarting the Servy host would interrupt six dependent
  production services. A full Windows restart may also start
  `OCI-A1-Grabber`, which can create OCI resources. Neither restart was
  performed; both await the user's explicit decision.
- The boot trigger is configured but cannot be promoted to tested until a
  restart is authorized. F1 therefore remains IN_PROGRESS; F2/F3 must not start
  until the remaining F1 gate is resolved.
- No production tasks or acknowledgements were created. The isolated test DB
  and monitor fixtures remain under
  `C:\ProgramData\coord\data\verification-20261021`; lease token values were
  not printed or added to documentation, and no test listener/service remains.

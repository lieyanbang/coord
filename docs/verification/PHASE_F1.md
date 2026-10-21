# Phase F1: Arasaka Service and Health

Date: 2026-10-21 (Australia/Brisbane)
Result: **IN_PROGRESS**. No coord service has been installed or started yet.

## Local Implementation

- Added a read-only `coord health` CLI probe. It validates the JSON-RPC 2.0
  envelope and `tasks/list` array, opens the configured database read-only,
  checks the `tasks` schema, writes READY/DEGRADED/FAIL plus latency and last
  READY time, and never serializes task data or lease tokens.
- Added `tasks/list.skip_reclaim=true` for monitors. This keeps expired-lease
  recovery with the existing background reclaimer rather than making the probe
  mutate task state. The default list behavior is unchanged.
- Added `scripts/windows/coord-health-monitor.ps1`: three consecutive hard
  failures trigger only the `coord` Servy service, with a 10-minute cooldown
  and a maximum of three recovery attempts per hour. Slow successful probes
  are DEGRADED and do not restart the service.
- Added `scripts/windows/sqlite-online-backup.py` for non-overwriting SQLite
  online backups, verification, and isolated restore rehearsals.
- `pwsh -NoProfile -File scripts/verify.ps1 -Stage f1`: fmt, locked clippy,
  locked all-feature tests, and locked release build all exited 0. Detailed
  logs and checks are `stage-f1-*.log` and `stage-f1-checks.json`.
- Targeted health tests: 2 passed, including an expired lease remaining claimed
  across the read-only probe and a claimed lease token absent from stdout/status.
  Windows PowerShell 5.1 monitor smoke test returned READY with both checks
  passing and created no recovery record.

## Remote Preparation

- Fresh read-only checks found no `C:\ProgramData\coord`, no `coord` Servy
  entry, no `coord-health-monitor` scheduled task, and no listeners on 7777 or
  7778. Existing Servy-managed services were unchanged.
- Created `C:\ProgramData\coord\{bin,data,logs,health,backup}` with SYSTEM and
  Administrators full control; LocalService has traversal on the root, read/
  execute on `bin`, and modify on `data`/`logs`. It has no write access to
  `health` or `backup`.
- Before any Servy configuration change, used SQLite `Connection.backup()` to
  snapshot `C:\ProgramData\Servy\db\Servy.db` to
  `C:\ProgramData\coord\backup\Servy-pre-coord-20261021.db` (32,768 bytes,
  SHA-256 `24ab81a59c70f4c1aab51062ececaa308b1470539ee4fab9d5c6b1f3b02f9778`).
  The source and backup user-table row counts match. `PRAGMA quick_check` is
  **NOT_RUN** because Python's SQLite runtime lacks Servy's custom
  `UNICODE_NOCASE` collation; no replacement collation was guessed.

## Remaining Gate

Still required before F1 can PASS: deploy the Actions-built EXE and Servy entry;
verify LocalService identity, loopback-only listener and rotation; install the
SYSTEM scheduled health monitor; exercise JSON-RPC/SQLite readiness, targeted
Servy recovery and process-kill recovery; and validate the coord DB backup/
restore, WAL consistency and lease recovery. Full Servy-host and OS restart
tests are pending user confirmation because the Servy host has six dependent
production services, including OCI-A1-Grabber. No production queue tasks have
been created.

# Phase F1: Arasaka Service and Health

Date: 2026-10-21 (Australia/Brisbane)
Result: **PARTIAL (technical acceptance complete; candidate Windows Actions pending)**.
Servy-host and Windows restart recovery, live service health, coord WAL integrity,
coord backup checks, and the Servy configuration DB integrity gate have passed.
The only remaining F1 closure condition is successful Fork Windows Actions for
the commit containing this evidence and the collation-aware checker. Keep F2
TODO until that run succeeds.

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
- Post-restart evidence commit `54b5863` passed Fork Windows Actions run
  `39568595098`: job `test (windows-2022)` and fmt, clippy, test, release build,
  and artifact upload all concluded success. Run metadata and step results
  were read directly from the GitHub Actions REST API.

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

## Post-Restart Acceptance (2026-10-21)

### Servy host restart

- After the user authorized the operation, `Restart-Service -Name Servy -Force`
  was issued at `2026-10-21T06:29:52Z`; the remote acceptance script exited 0.
  Servy PID changed from 10700 to 6260. `ArasakaDDNS`, both FFmpeg services,
  `MediaSTT`, `OCI-A1-Grabber`, and `coord` returned to Running. `CodexProxy`
  remained Stopped, matching its pre-restart state; it was not started or
  stopped by this task.
- Immediately after that restart, coord returned READY in 67 ms. The monitor
  task briefly reported Task Scheduler result `267009` (running); after it
  settled, it was Ready with `LastTaskResult=0`, and another probe returned
  READY in 4 ms. The temporary running result was not treated as a failure or
  as a completed success.

### Windows restart and live recovery

- `shutdown.exe /r /t 0 /f` was issued at `2026-10-21T06:31:53.629Z` and
  returned exit 0. The user reported that Tailscale, SSH, and Remote Control 4
  were temporarily unavailable after reboot and later recovered. Direct SSH
  was independently working by `2026-10-21T08:06:29Z`; Remote Control 4
  recovery is user-reported and was not independently tested because this task
  did not use MCP.
- The live Windows `LastBootUpTime` was `2026-10-21T06:35:49.835Z`.
  Tailscale was Running/Automatic at `100.89.6.126`; `sshd` and Servy were also
  Running. Servy was Automatic/LocalSystem, PID 3224.
- Post-boot service inventory showed all seven Servy-managed entries Running:
  `ArasakaDDNS`, `ArasakaFFmpegWatchdog`, `ArasakaFFmpegWorker`, `CodexProxy`,
  `MediaSTT`, `OCI-A1-Grabber`, and `coord`. Windows service start mode was
  Automatic for each. Servy CLI `status --name <name> --quiet` returned exit 0
  and Running for every entry.
- `CodexProxy` had an initial post-boot Stopped status despite a separately
  running process, as reported by the user. The user subsequently started its
  Windows service. Current state was independently verified without changing
  it: Servy DB contains its service entry, Servy CLI reports Running (exit 0),
  the Windows service is Running/Automatic/LocalSystem (PID 8236), and the
  process chain is `Servy.Service.CLI.exe` PID 8236 -> `powershell.exe` PID
  8188 -> `codex-proxy.exe` PID 1020. Its health request returned HTTP 200 in
  109 ms. Thus it is currently Servy-managed, not merely an independent
  orphan process. No CodexProxy process or service was started/stopped by this
  verification, and no Servy configuration was changed.
- Port/process ownership after boot: coord PID 5368 owns only
  `127.0.0.1:7777`; CodexProxy PID 1020 owns `0.0.0.0:13556`, `0.0.0.0:8790`,
  and `127.0.0.1:9090`; MediaSTT's process tree owns `127.0.0.1:13356`; the
  FFmpeg worker owns `192.168.137.1:13324`. The DDNS and OCI Grabber process
  trees had no listening ports. No listener was found for `coord` outside
  loopback.
- Actual local health requests returned: coord `health` READY at
  `2026-10-21T08:06:40.993Z` (JSON-RPC `tasks/list` and read-only SQLite both
  PASS, 4 ms total); CodexProxy `GET /health` 200; MediaSTT `GET /health/live`
  and `/health/ready` both 200; FFmpeg Worker `GET /health` and `/ready` both
  200. ArasakaDDNS, FFmpeg Watchdog, FFmpeg Worker, and OCI-A1-Grabber also
  passed Servy status/process liveness checks. The OCI Grabber check did not
  query or mutate OCI resources.
- Servy configuration flags read from its DB, excluding command parameters and
  credentials: coord health monitoring enabled (10 s heartbeat, two failed
  checks, bounded recovery); process heartbeat monitoring was enabled for
  ArasakaDDNS (15 s), both FFmpeg services (10 s), and OCI-A1-Grabber (300 s).
  It was disabled for CodexProxy and MediaSTT; those services were instead
  covered by the HTTP checks above. MediaSTT readiness and FFmpeg backend
  readiness both returned HTTP 200.
- The SYSTEM/highest-run-level `coord-health-monitor` task has enabled boot and
  periodic triggers. After reboot it was Ready with `LastTaskResult=0`; its
  latest recorded probe at `2026-10-21T08:07:16.178Z` was READY, 4 ms, zero
  consecutive failures. No recovery action was recorded for this boot.

### Database and lease state after reboot

- Current coord DB was opened read-only while the daemon was running. It
  reported `journal_mode=wal`, `PRAGMA quick_check=ok`, and four user tables:
  `agents=0`, `batches=0`, `task_receipts=0`, `tasks=0`. There were zero active
  leases, expired leases, claimed rows without tokens, or submitted tasks. No
  production task or acknowledgement was created; therefore there was no live
  production lease to reclaim after this reboot. The earlier isolated crash/WAL
  rehearsal remains the evidence for attempt-1 -> attempt-2 reclaim behavior.
- Current coord files after boot: `coord.db` 4,096 bytes,
  `coord.db-wal` 61,832 bytes, `coord.db-shm` 32,768 bytes. The read-only
  `quick_check` included the active WAL and returned `ok`; no checkpoint or
  database mutation was issued.
- Online backups were made with
  `py -3.14 sqlite-online-backup-v3.py --source ... --destination ...` before
  each disruptive restart, then re-opened with the same helper's
  `--verify-only` mode. The Servy-host-restart pair is
  `coord-pre-host-restart-20261021T062927182Z.db` (61,440 bytes,
  `quick_check=ok`, four table row counts match, SHA-256
  `6744ae421cff06b4fa4ab423a2a7777657bc7f046140659ab56d5d136c08dba5`) and
  `servy-pre-host-restart-20261021T062927182Z.db` (32,768 bytes, two table row
  counts match, SHA-256
  `0c22950d27d00926640ef9e732b433d45619a6852c355e3259c42b2ba358e7d0`).
- The pre-Windows-restart pair is `coord-pre-windows-restart-20261021T063113961Z.db`
  (same size/counts/hash as the first coord backup) and
  `servy-pre-windows-restart-20261021T063113961Z.db` (32,768 bytes, two table
  row counts match, SHA-256
  `a31ca224a73979d065a3b3b315498396f4cba45eb25c1edcfbf2fece348e549a`).
  Post-boot `--verify-only` against current sources exited 0 for all four
  snapshots and reported matching row counts.
- Before the collation-aware gate below, the current Servy DB was found in WAL mode (`Servy.db` 32,768 bytes,
  `-wal` 4,152 bytes, `-shm` 32,768 bytes); read-only enumeration found
  `SchemaInfo=1` and `Services=7`, matching its online backups. At that point,
  `PRAGMA quick_check` returned **NOT_RUN** with
  `no such collation sequence: UNICODE_NOCASE`; the failing index is
  `idx_services_name_unique`. Servy's CLI help exposes no integrity-check
  command, and no collation semantics were guessed or registered. This was the
  remaining evidence gap; the later source-matched check below closes it.

## Servy Configuration Integrity Gate (2026-10-21)

### Installed version and source

- Direct SSH read-only query to `C:\Program Files\Servy\servy-cli.exe
  --version` returned `Servy.CLI 10.2.0+bc0f0c7b67862028549e8d99c0b1d16c2b58d228`
  with exit 0. The executable ProductVersion matched; its SHA-256 was
  `8ACBBD8253982FFC1FD05CE1DFF080426638D6B64BA25A59B1AF0A21DDEFE032`.
- The installed `Servy.psd1` names `https://github.com/aelassas/servy` as its
  ProjectUri. GitHub's commit API resolved the exact build commit, dated
  `2026-10-05T06:10:14Z`, with `verification.verified=true`:
  [Servy commit bc0f0c7](https://github.com/aelassas/servy/commit/bc0f0c7b67862028549e8d99c0b1d16c2b58d228).
- At that commit,
  [UnicodeNoCaseCollation.cs](https://github.com/aelassas/servy/blob/bc0f0c7b67862028549e8d99c0b1d16c2b58d228/src/Servy.Infrastructure/Data/UnicodeNoCaseCollation.cs#L16)
  registers the `UNICODE_NOCASE` SQLite collation; its `Compare` method handles
  nulls and returns `string.Compare(param1, param2,
  StringComparison.OrdinalIgnoreCase)` ([lines 46-56](https://github.com/aelassas/servy/blob/bc0f0c7b67862028549e8d99c0b1d16c2b58d228/src/Servy.Infrastructure/Data/UnicodeNoCaseCollation.cs#L46-L56)).
  [`AppDbContext` registers before opening connections](https://github.com/aelassas/servy/blob/bc0f0c7b67862028549e8d99c0b1d16c2b58d228/src/Servy.Infrastructure/Data/AppDbContext.cs#L23-L26);
  the database initializers also register this type before schema work.
- The same source commit declares `System.Data.SQLite` **2.0.4** and
  `SourceGear.sqlite3` **3.53.4** in
  [`Directory.Packages.props`](https://github.com/aelassas/servy/blob/bc0f0c7b67862028549e8d99c0b1d16c2b58d228/Directory.Packages.props#L17-L18),
  targeting `net10.0-windows`. The checker used that Provider package and native
  SQLite package, and ran on Arasaka as .NET 10.0.10; observed Provider assembly
  version was `2.0.4.0` and SQLite engine version was `3.53.4`. No Python Unicode
  comparison approximation was used.

### Checker and snapshot

- Added `scripts/verification/ServySnapshotCheck/Program.cs` and its project
  file. It registers the source-equivalent collation before opening SQLite,
  opens the source with `ReadOnly=true` and `FailIfMissing=true`, refuses an
  existing snapshot path, and uses `SQLiteConnection.BackupDatabase` to create
  the new snapshot. It enables `query_only` only on the snapshot connection;
  both integrity PRAGMAs and all schema/configuration reads use that snapshot.
  The tool outputs service names and full-row SHA-256 values, never raw config,
  command-line, or credential columns.
- `dotnet publish scripts/verification/ServySnapshotCheck/ServySnapshotCheck.csproj
  -c Release -r win-x64 --self-contained true`: exit 0 (local SDK 10.0.302).
  Local `--self-test`: exit 0. It created a temporary WAL fixture using the
  SQLite Online Backup API, passed both integrity checks and seven Unicode
  comparison cases, and proved an existing snapshot is not overwritten. The
  exact published binary was uploaded to Arasaka; remote `--self-test` also
  exited 0. The uploaded executable was removed from its verified per-run Temp
  directory after validation; the snapshot was preserved. No service was
  started, stopped, or restarted.
- The production snapshot was created at `2026-10-21T12:19:39Z` from the live
  source `C:\ProgramData\Servy\db\Servy.db` to the previously absent
  `C:\ProgramData\coord\backup\servy-f1-collation-20261021T121838Z.db`.
  Command: `servy-snapshot-check.exe --source
  C:\ProgramData\Servy\db\Servy.db --snapshot
  C:\ProgramData\coord\backup\servy-f1-collation-20261021T121838Z.db`;
  checker and SSH exits were both 0. Source was opened read-only and copied via
  the SQLite Online Backup API; no production database file was written. The
  create run used checker EXE SHA-256
  `70C1E43CCFFBFEE378C1C0667AEC3075DEE517165E6212761A227CA43BE3F30E`.
- The final checker re-opened only the snapshot in read-only mode at
  `2026-10-21T12:23:45Z`; `--verify-snapshot` and SSH both exited 0. A separate
  `Get-FileHash -Algorithm SHA256` at `12:24:41Z` confirmed the same main-file
  hash. The main snapshot is 32,768 bytes with SHA-256
  `9E64050D0A5FCA14FB460D7730996960A34612AE3C8E012AB5A16A7A4A1ACB29`.
  The snapshot `-wal` sidecar is 0 bytes (empty-file SHA-256
  `E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855`); a
  32,768-byte `-shm` sidecar was created by the read-only WAL-mode verification.

### Results

- Checker output: `quick_check=["ok"]`, `integrity_check=["ok"]`, seven of
  seven collation comparisons matched `StringComparison.OrdinalIgnoreCase`.
- `idx_services_name_unique` exists, is unique, and its single key is
  `Services.Name COLLATE UNICODE_NOCASE` (`index_list` and `index_xinfo` agreed).
- `SchemaInfo` contains exactly one row: `Id=1`, `Version=10`.
- `Services` contains exactly these seven rows. `record_sha256` covers every
  column of each row; raw configuration values were not emitted:

| Service | record_sha256 |
| --- | --- |
| ArasakaDDNS | `080058B717D4139A1C71021613CD23D90D0A2A6E3BE5C209EC7B1E0BA43D022A` |
| ArasakaFFmpegWatchdog | `190239B8CD1FE2F898D6C82E77B2D6B19C2C0A2E347AE29BD647AD69E20D0305` |
| ArasakaFFmpegWorker | `30764475F914038EBF89F0EA03A703453CCF73F06C9921E783155E5360803C18` |
| CodexProxy | `FB2573763189B04872C3AB72082C650E1034ACDA204C1F80D2C2B1DF89718F91` |
| coord | `2F83F99D2B600A228B6816B5517B71A67EE39C3F60A99806FC71393B7BBF0F05` |
| MediaSTT | `333A7FA95FE4FDB5B4F80A2ACEB18FDD85FE8CF59E59163090D6CEAB850719E1` |
| OCI-A1-Grabber | `2969F540B58BA4B7FB8533024721FBAB353C394BB9A7B77A1BAE2F70EBBD2978` |

- Final checker EXE: 75,918,514 bytes, SHA-256
  `9A324BC43795675EA47E991FF36907D4A36FAEAD8E95EA429BC97CEFDCB084BD`.
  `Program.cs` SHA-256 is
  `F7376BC60D97F5484DA2CBF9E297E89C4DEFF4A863E636AF61AF6F719CAB60C4`;
  `ServySnapshotCheck.csproj` SHA-256 is
  `66685FF0F845BB6F61DF03B54A321A03F59C13C539DDCFBA0F302955B8D03D6F`.

### Post-integrity health

- At `2026-10-21T12:28:30Z`, Windows `LastBootUpTime` remained
  `2026-10-21T06:35:49.8354060Z`; Servy was Running/Automatic.
- `servy-cli.exe status --name coord --quiet`: exit 0, `Running`.
- `coord.exe --url http://127.0.0.1:7777/ health --db
  C:\ProgramData\coord\data\coord.db --status
  C:\Users\lieyanbang\AppData\Local\Temp\coord-f1-health-after-final-integrity-20261021T1225Z.json
  --timeout-ms 2000 --degraded-after-ms 2000`:
  exit 0, READY at `12:28:32.282Z`, total 4 ms, `tasks_list=PASS` (3 ms),
  `sqlite_read_only=PASS` (1 ms), zero consecutive failures. The command used
  `tasks/list` with `skip_reclaim=true`; the unique temporary status file was
  removed after the probe.
- The listener remained `127.0.0.1:7777`, owned by PID 5368 at
  `C:\ProgramData\coord\bin\coord.exe`. No restart or recovery action was
  issued during the collation gate.
- Local `pwsh.exe -NoProfile -File scripts/verify.ps1 -Stage f1` completed with
  fmt, locked clippy, locked all-feature tests, locked release build, and the
  Windows PowerShell health-monitor harness all exit 0. Timings and command
  names are in `stage-f1-checks.json` and individual logs.

## Gate Decision

- Servy-host restart, Windows boot recovery, coord and service health, loopback
  isolation, coord WAL integrity, previous backup/restore rehearsal, and
  restart-time online backup row-count checks passed.
- All technical F1 gates, including the Servy configuration database integrity
  gate, now pass. **Overall F1 remains PARTIAL only until Fork Windows Actions
  succeeds for the candidate commit.** F2/F3 remain TODO pending that CI result.
  No service configuration or production queue state was changed during this
  verification.
- No production tasks or acknowledgements were created. The isolated test DB
  and monitor fixtures remain under
  `C:\ProgramData\coord\data\verification-20261021`; lease token values were
  not printed or added to documentation, and no test listener/service remains.

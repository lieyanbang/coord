# Stage A — quality hardening

Starting commit: `572771a`. Date: 2026-10-20. Bootstrap ledger: Git; no shared
daemon is available. TASK.md remains untouched. Windows is the default platform.

## Reproduced defects and fixes

`cargo test --test quality --locked`: exit 101 before fixes; **all four tests
failed** (`stage-a-quality-before.log`). After fixes: exit 0, four passed
(`stage-a-quality-after.log`).

- Migration previously created indexes against absent legacy columns before
  adding them. Migration now runs under an immediate transaction, inspects real
  columns and creates dependent indexes last. Busy timeout: five seconds.
- Unknown state / invalid JSON became pending/null. These now return explicit
  SQLite conversion errors; NULL optional values remain valid, corrupt values
  fail. Empty optional timestamps also fail instead of becoming NULL.
- Same-minute same-name tasks and colliding agent slugs overwrote Vault notes.
  Task filenames now include full UUIDs; agent filenames include deterministic
  UUIDv5 identity hashes, with bounded ASCII display names. Temporary files have
  unique names. Legacy notes are retained; existing historical links are not
  rewritten. Vault must remain operator-controlled; it is a best-effort mirror.
- First event before heartbeat acquired the same non-reentrant lock twice.
  A lock-held initialization helper removes the deadlock.

Long-poll subscribes before its initial query and propagates sweep errors.
A deterministic injected write between query and wait verifies that the event
is not lost. Real HTTP long-poll tests also pass. RPC rejects browser Origin
requests, caps JSON bodies at 1 MiB and active handlers at 128 (`-32002` busy).
Non-loopback binding warns about the absence of authentication. The new real
daemon boundary test verifies Origin rejection, HTTP 413 and no task side effect.

## Known lease defect staged for Gate B

The permissive old test was replaced by a strict stale-result assertion.
`cargo test --test lease expired_lease_reclaims_and_lets_a_new_agent_win --locked`
exited **101**, proving stale completion is accepted (`stage-a-lease-defect.log`).
It is explicitly ignored for this quality-only stage with the Stage B reason;
this is NOT a lease PASS. Gate B must enable and pass it after migrating all
write interfaces; no claim of overall acceptance is made here.

## Dependency and tooling audit

Actual cargo-audit 0.22.2 baseline: exit 1, six vulnerability advisories.
Targeted compatible lock updates: h2 0.4.14 -> 0.4.20, rustls 0.23.40 -> 0.23.46,
rustls-webpki 0.103.13 -> 0.103.16, anyhow 1.0.102 -> 1.0.104. No indiscriminate
dependency refresh. UUIDv5 enables collision-resistant agent note identity.

Remaining RUSTSEC-2026-0189 is explicitly scoped out in `.cargo/audit.toml`:
only rmcp's Streamable HTTP server transport is affected, and this project uses
stdio only. The independent axum RPC is protected from browser Origin requests.
Revisit the exception before enabling any rmcp HTTP transport.

Warnings retained and documented: paste is unmaintained; ratatui 0.29's lru 0.12
has IterMut/panic-safety advisories. Our code does not call IterMut or use custom
panicking key destructors; a TUI dependency migration is not necessary for this
scope. These warnings are not represented as fixed. Cargo metadata enumerated
294 packages, all with declared licenses; MIT original LICENSE is unchanged.
This is metadata/audit review, not a legal certification or reachability proof.

Justfile now runs coord, uses an isolated persistent development DB without
deleting it, supports PowerShell, and removes broad process-kill helpers.
AGENTS now separates repository bootstrap from deployed product coordination.

## Gate evidence

**Gate A: PASS.** `pwsh -File scripts/verify.ps1 -Stage a` exited 0: fmt,
clippy, tests and release build all exited 0. cargo-audit with the documented
transport exception exited 0; three retained warnings remain above.
Per-command exit codes, UTC times,
durations and logs are recorded in `stage-a-checks.json`. Required local checks
must all exit 0 before committing. The known lease skip is explicitly above.
Stage A cloud evidence is recorded by its actual pushed SHA/run separately.

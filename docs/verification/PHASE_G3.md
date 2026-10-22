# Phase G3: Trusted Completion Actions

Result: **PARTIAL** overall. The isolated Windows source and recovery gate
passes; production execution remains disabled pending service-account and
publisher controls.

The opt-in JSON registry selects fixed `.exe`/`.com` paths under
`allowed_root`, fixed argument templates with a small placeholder allowlist,
and fixed working directories. Unknown `shell` fields, outside-root
executables, and unknown placeholders are rejected. Process creation uses
`Command` without a shell, clears the environment, bounds stdin/output/time and
uses a Windows Job Object (eight processes, 256 MiB memory, kill-on-close).
Concurrency is two. Completion and outbox insertion share a SQLite transaction;
Action state is independent from task state and uses its own owner/token lease.
Idempotent retry is at-least-once; a non-idempotent expired action becomes
`uncertain`, never presumed successful.

The final `tests/agent_actions.rs` Windows run reports **10 passed, 0 failed,
6 ignored**. It includes the real isolated daemon test
`daemon_crash_after_action_side_effect_recovers_as_uncertain_without_replay`:
the temporary child writes one marker then waits, the temporary daemon process
is killed, the temporary SQLite Action lease is expired, and a restarted daemon
recovers the Action as `uncertain` without a second marker. The completed task
stays completed. Other tests cover atomicity, duplicate completion, validator
PASS, idempotent replay/fencing, non-idempotent uncertainty, timeout, output
overflow, and command-policy rejection. No production database is involved.

The executor runs as the coord daemon's existing Windows account; it does not
switch to a low-privilege account and the registry is not a filesystem sandbox.
The example publisher/index binaries are placeholders and are not shipped.
Production acceptance is therefore **NOT_RUN** until a dedicated least-
privilege account, restrictive ACLs, reviewed/hash-pinned native binaries, and
an isolated production-policy rehearsal are independently verified.

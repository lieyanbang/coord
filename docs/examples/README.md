# Completion Action example

`completion-actions.example.json` is an administrator template. Replace the two
actual account SIDs, independent random broker/management key files and the
reviewed artifact SHA-256; the checked-in placeholders deliberately fail
validation. `cargo build --release --locked` now builds the real
`coord-action-runner.exe` and `coord-emote-action.exe` alongside `coord.exe`.
The same native emote binary dispatches task publication and batch indexing from
the trusted event kind.

Production activation is gated on [the runner procedure](../ACTION_RUNNER.md),
actual account/ACL/RPC-denial tests, artifact verification and Phase G6-A. The
example never creates a business batch or starts an Agent. `TASK.md` remains the
read-only scope contract.

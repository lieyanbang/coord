# Completion Action Example

`completion-actions.example.json` shows the trusted registry shape for the
first-pass emote workflow. It is intentionally not executable as checked in:
`coord-emote-publisher.exe` and `coord-emote-index.exe` are placeholders and
are not included. The registry loader refuses missing executables and paths
outside `allowed_root`.

Before deployment, an operator must provide reviewed native binaries, set
restrictive filesystem ACLs on the registry/root/results, configure the coord
daemon under a dedicated least-privilege Windows account, verify no inherited
secrets, and exercise recovery against an isolated database. Neither the sample
profile nor local source tests authorize using the current production Relay
identity or adding business tasks.

# Phase G Operator Packet

This directory contains Phase G launch packets, the strict first-pass result
schema, and preparation/validation/aggregation utilities. The task request
generator is read-only by default and never calls coord. `TASK.md` remains the
execution contract; these documents do not authorize production task creation.

## Current Gate

G1 is **PARTIAL**: the deployed Cloudflare Access/OAuth path proves one owner
identity, while the deployed CoordMCP/Relay maps every client to the single
`coord-relay-agent`. A label such as A0 is input data, not an authenticated
session identity. The six simulated local clients prove scheduler concurrency
and lease fencing only; they do not prove six independent ChatGPT identities.
Do not start any A0-A5 production conversation until an independently
non-exportable session binding and its positive/negative production tests exist.

G2 is **PARTIAL**: lease fencing, progress sequence, freshness and budget
mechanics pass isolated Windows tests. Checkpoints are self-reported, the
60-minute budget applies to one attempt, and neither behavior proves ChatGPT
liveness or six-worker identity separation.

G3 source behavior passes isolated Windows tests, including a temporary daemon
crash after an external test-file side effect. G3 is **PARTIAL** for production:
the Action child inherits the coord daemon account, and no dedicated least-
privilege account/ACL, approved native publisher binary or production policy
has been verified. Idempotent recovery is at-least-once; a non-idempotent
uncertain event requires operator reconciliation.

G0 and G4 preparation are **PASS**. The source inventory was revalidated in
read-only dry-run mode and produced a 121-task request with SHA-256
`60126faa792c22ffccdae498ea5272db4832d3eb52cb1efda673620cb1a2c91f`; it was
not submitted. No production Batch, task, or Agent was created or started.

G5 is **PARTIAL** and production deployment is **NOT_RUN**. Local Windows gates
pass, but Fork Actions and its artifact must still be verified before code
acceptance; deployment remains withheld until G1 and the G3 service-account and
publisher gates pass. The production SQLite database, Arasaka services,
Sidecar and Relay routes were not changed. F3-R remains PARTIAL independently.

## Tools

```powershell
py -3 scripts/phase_g/prepare_initial_tasks.py --source-root D:\ChatGPT_Emote_Work
py -3 scripts/phase_g/validate_result.py --result result.json --expected expected-task.json --refs-root D:\ChatGPT_Emote_Work\refs
py -3 scripts/phase_g/aggregate_results.py --request prepared-request.json --results-dir results --refs-root D:\ChatGPT_Emote_Work\refs --out-dir results
```

The first command emits a complete prepared request but does not submit it. To
write the request artifact, pass `--write-request <new-path-outside-source>`;
it refuses to overwrite a file. Aggregation also defaults to dry-run; `--write`
creates only a preliminary `index.json` and `review_queue/candidates.json` and
refuses to overwrite either. Candidate keys are for review suggestions only,
not persistent character IDs.

The action registry example in `docs/examples/completion-actions.example.json`
is a template. Its named publisher/index `.exe` files do not ship in this
repository and must not be enabled until built, reviewed, hash-pinned and tested
under the deployment service account.

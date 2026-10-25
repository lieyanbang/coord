# Phase G operator materials

The active contract is TASK.md and PLAN §13 (G7). The initializer remains
read-only by default: this repository does not authorize the formal 121 tasks
or production classification agents.

G7 adds an opt-in trusted logical-worker mode under the existing owner OAuth.
worker_id is four hex characters, not a credential; the same owner's other
conversations may impersonate it. See [logical workers](LOGICAL_WORKERS.md) and
the six canary packets in agents/. Strong G6 token bindings remain optional.
Fencing, batch/tool allowlists, management RPC keys, non-admin Runner ACLs and
fixed native actions remain deployment gates.

The live G7 state and evidence are in [PHASE_G7](../verification/PHASE_G7.md).
Historical G0–G6 results remain in ../verification/; their incomplete production
identity assumptions are not the accepted G7 logical-worker contract. The
G6 deployment runbook still defines backup/integrity/rollback requirements;
G7 explicitly relaxes only per-conversation anti-impersonation and background wake.

The native coord-emote-action.exe and coord-action-runner.exe are actual Cargo
build targets. Use exact CI artifacts with administrator-approved hash pins and
real Runner SID/ACL validation. Example registry placeholders are not deployment
configuration. Agent results cannot select commands or publish directly.

Preparation/validation/aggregation:

```powershell
py -3 scripts/phase_g/prepare_initial_tasks.py --source-root D:\ChatGPT_Emote_Work
py -3 scripts/phase_g/validate_result.py --result result.json --expected expected-task.json --refs-root D:\ChatGPT_Emote_Work\refs
py -3 scripts/phase_g/aggregate_results.py --request prepared-request.json --results-dir results --refs-root D:\ChatGPT_Emote_Work\refs --out-dir results
```

The initializer does not submit. --write-request requires a new output path.
Aggregation defaults to dry-run; --write emits preliminary index/review candidates
without permanent character IDs or automatic merges. Inputs and refs are read-only.
Old A0–A5 result IDs remain compatible; G7 emits canonical emote-XXXX result IDs.

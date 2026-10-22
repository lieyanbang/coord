# Phase G0: Audit and Baseline

Result: **PASS** for the audit and protection gate. No production write or
service action was performed. Phase F3-R remains independently **PARTIAL**.

## Repository and Input Data

- Read the execution contract `TASK.md`, `docs/PLAN.md` section 11, `AGENTS.md`,
  the F1/F2/F3-R evidence, Coord Rust store/A2A/MCP/server code, Relay protocol,
  agent and server, and CoordMCP facade. `TASK.md` SHA-256 at inspection:
  `3af6474aa99074cdf203206dda97ad2372eba1137357047f580080de6dca780c`.
- The active branch is `codex/phase-g-agent-actions`, based at
  `92aea58` (`docs(f3-r): record final Windows verification`). Fork and upstream
  remotes were verified as `lieyanbang/coord` and `DmarshalTU/coord`.
- The inherited dirty `.gitignore`, untracked `TASK.md`, `.desktop-commander-artifacts/`
  and `relay/coord-mcp/node_modules/` were preserved. No reset, history rewrite,
  upstream write, or production database change was made.
- `py -3 scripts/phase_g/prepare_initial_tasks.py --source-root D:\ChatGPT_Emote_Work`
  exited **0**, state `dry_run`: 1,218 manifest rows, 1,206 unique images,
  12 duplicate mappings, 121 groups, and A0-A5 plus task/121/task/122 empty.
  Request SHA-256:
  `60126faa792c22ffccdae498ea5272db4832d3eb52cb1efda673620cb1a2c91f`.
  No request was written and no Coord call was made.

## Read-only Production Baseline

At `2026-10-22T16:37:12Z`, a direct SSH PowerShell query to Arasaka exited **0**.
The Windows Servy service was Running/Automatic/LocalSystem; boot time was
`2026-10-21T06:35:51Z`. Servy CLI reported both `coord` and `CoordRelayAgent`
Running (each exit **0**). Coord PID 5368 owned only `127.0.0.1:7777`. A
read-only JSON-RPC `tasks/list` with `skip_reclaim=true, limit=1` returned one
row without a JSON-RPC error. `coord.exe --version` at `16:43:50Z` returned
`coord 0.4.1` (SSH exit **0**). Installed binary SHA-256:
`b58d3620b1b6de8035bfcb97e99f2e4fe654f48b1cb5719ef9fca013f5652e0d`.

At `2026-10-22T16:37:22Z`, a direct SSH read-only query to the Sidecar host
exited **0**. `coord-relay.service`, `coord-relay-acl.service`,
`feathersjsapp.service`, `openresty.service` and `tailscaled.service` were all
active. Listeners remained `100.116.134.114:13011`,
`100.116.134.114:13012`, and `127.0.0.1:13013`; the Sidecar unit ran as
`coord-relay:coord-relay`. `/healthz` returned `READY`,
`agentConnected=true`, `pending=0`. No service or route was changed.

During the same read-only review window, production hashes were read without
printing configuration or credentials: CoordMCP `index.cjs` was
`e2a4c125f6742f915662707ff123f3eb0a3e3ec6b64ae2818baf1c3ea7cd5db3`;
`oauth.cjs` was `0dcc483bd3d6afd5f4625885aed70f5cdf1b30f6a44ced739549d382ce049c53`;
Sidecar `server.mjs` was
`3351081c4d2ef9ba7f166822dc48e5a1bf6b8fe18e1ff36fac11a051e98dcf84`.
The live OAuth hash differs from the last recorded F3-R deployment hash, so
F3-R is not promoted to PASS.

Commands and exit codes: `git status --short --branch` **0**; the inventory
dry-run **0**; each direct SSH read-only query above **0**. The final
`pwsh -NoProfile -File scripts/verify.ps1 -Stage g0` began at
`2026-10-22T16:49:39Z`; fmt, clippy, locked Rust tests, release build, Relay,
CoordMCP and Python utility tests all exited **0**. Per-command evidence is in
`stage-g0-checks.json` and `stage-g0-*.log`; final overall-gate evidence is in
`PHASE_G5.md`.

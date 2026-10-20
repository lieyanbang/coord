#!/usr/bin/env bash
# Three "fake agents" race for tasks against a running coord daemon. Run
# this in one pane while `coord top` is open in another to see the
# atomic claim primitive in action.
#
# Usage:
#   coord serve                          # term 1
#   coord top                            # term 2
#   ./scripts/demo.sh                    # term 3
#
# Override defaults with env vars:
#   COORD_URL=http://...   NUM_AGENTS=5   NUM_TASKS=20   ./scripts/demo.sh
set -euo pipefail

URL="${COORD_URL:-http://127.0.0.1:7777/}"
COORD="${COORD:-coord}"
NUM_AGENTS="${NUM_AGENTS:-3}"
NUM_TASKS="${NUM_TASKS:-12}"

trap 'echo; echo "stopping demo"; kill $(jobs -p) 2>/dev/null || true' EXIT

# Spawn N fake agents in the background. Each heartbeats, polls for a
# pending task, races to claim it, "works" briefly, then completes it.
# `tasks/claim` is atomic, so even with all N agents trying the same task
# at the same instant exactly one wins.
for i in $(seq 1 "$NUM_AGENTS"); do
    AGENT="worker-$i"
    (
        while true; do
            "$COORD" --url "$URL" heartbeat "$AGENT" --name "demo worker" \
                >/dev/null 2>&1 || true

            # Optional POSIX demo: python3 parses full IDs and the claim fence.
            TASK_ID=$(curl -fsS -H 'Content-Type: application/json' "$URL" \
                -d '{"jsonrpc":"2.0","id":1,"method":"tasks/list","params":{"state":"pending","limit":1}}' \
                | python3 -c 'import json,sys; r=json.load(sys.stdin).get("result",[]); print(r[0]["id"] if r else "")')
            if [[ -n "${TASK_ID:-}" ]]; then
                if CLAIM=$("$COORD" --url "$URL" claim "$TASK_ID" --as "$AGENT" 2>/dev/null); then
                    TOKEN=$(printf '%s' "$CLAIM" | python3 -c 'import json,sys; print(json.load(sys.stdin)["lease_token"])')
                    sleep "0.$((RANDOM % 9 + 2))"
                    "$COORD" --url "$URL" complete "$TASK_ID" --as "$AGENT" --token "$TOKEN" \
                        --result "{\"by\":\"$AGENT\"}" >/dev/null 2>&1 || true
                fi
            fi
            sleep 0.3
        done
    ) &
done

# Drip-feed tasks so the TUI shows steady churn instead of one big batch.
for i in $(seq 1 "$NUM_TASKS"); do
    "$COORD" --url "$URL" send "demo-task-$i" --payload "{\"i\":$i}" >/dev/null
    sleep 0.6
done

# Keep agents running for a few more seconds so the GIF has a clean tail.
sleep 5

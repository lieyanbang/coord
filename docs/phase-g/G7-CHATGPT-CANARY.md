# Two-conversation canary handoff

Status: **NEEDS_USER_ACTION**. The browser automation runtime currently fails
before JavaScript initializes (`failed to write kernel assets`, Windows os
error 3). No G7 real ChatGPT write/renew/submit cycle is claimed. The four
synthetic browser tasks remain unclaimed. This handoff does not authorize any
formal image classification or 121-task initialization.

Existing probe chats:

- A: https://chatgpt.com/c/6adc81f8-d6fc-83ec-a347-1f5e22910047
- B: https://chatgpt.com/c/6adc82c9-4b6c-83ec-b273-477e77ade133

Both use the existing authenticated Coord connector. Check that the refreshed
tool schema includes worker_id. Chat A declares `0000`; chat B declares `0001`.
Every logical call includes browser batch
`d2313c78-22da-4cb8-bcd7-ece9897e0db1`. This label is cooperative, not a protected
conversation credential; either owner conversation can declare either value.

The current binding expires at 2026-10-26 05:45:23.904 UTC. An operator must
review and renew equivalent policy at all three hops after that deadline;
never bypass the fail-closed expiry. The policy is not a permanent supervisor.

Use the matching role packet in agents/A0.md or agents/A1.md, with this browser
batch only, plus these fixture-specific instructions:

1. Call coord_agent_status with your worker_id and the browser batch. Call
   coord_claim_next with a new request UUID and lease_seconds 600. Keep the
   returned task, attempt_no and lease token private to this conversation.
2. Inspect the claimed task's synthetic one-pixel PNG and its synthetic marker
   reference before claiming visual verification. Fixtures are on Arasaka under
   `C:\ProgramData\coord\g7\browser\business\task` and `refs`. If the current
   tools cannot retrieve/view those files, report that exact boundary and stop;
   do not invent image inspection or request arbitrary command execution.
3. Call coord_report_progress with sequence 1 and the exact task/attempt/token,
   then coord_extend_lease_progress with progress_sequence 1 and a new request
   UUID. Keep the original claim metadata; do not read a newer attempt's token.
4. Build the documented schema_version 1 result using the task's group_id and
   source_image_ids, agent_id emote-0000 or emote-0001 and its exact attempt_no.
   This marker depicts no character: characters is empty and uncertain records
   the actual inspection, checked reference path, reason and low review priority.
   Submit with coord_submit_result and a new request UUID.
5. Read back the exact task, then claim the next browser task and repeat once.
   Stop after two tasks in this chat. Inspect authoritative batch_progress;
   an empty claim by itself does not prove completion. If a mutation is uncertain,
   read its exact state before deciding; do not automatically replay it.

The expected combined result is four completed tasks with distinct live claims,
two explicit renewal cycles per chat, four native publishes plus one aggregate
action, and no lease token in list/get or saved evidence. Operator verification
uses authenticated manager actions/status, output hashes and original-task
comparison; ChatGPT cannot change an action profile or call management RPC.

Refresh/close one chat during an additional approved fixture to test lost-turn
recovery if desired. No active turn means no renewal; leases expire normally.
Do not promise autonomous wake, 24-hour continuity or result replay after lease
loss. Browser acceptance remains incomplete until actual tool traces and
server-side task/attempt/action evidence establish the lifecycle above.

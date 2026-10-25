mod common;

use common::Daemon;
use serde_json::{json, Value};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

fn progress_params(task: &Value, agent_id: &str, sequence: u64, request_id: Uuid) -> Value {
    json!({
        "id": task["id"],
        "agent_id": agent_id,
        "lease_token": task["lease_token"],
        "attempt_no": task["attempt_no"],
        "sequence": sequence,
        "request_id": request_id,
        "checkpoint": format!("completed checkpoint {sequence}"),
    })
}

#[test]
fn progress_fences_attempts_and_requires_fresh_unconsumed_checkpoints() {
    let daemon = Daemon::new();
    let batch = daemon.rpc(
        "batches/create",
        json!({"name":"progress gate","config":{"max_attempts":2,"require_progress":true}}),
    );
    let batch_id = batch["id"].as_str().unwrap();
    let tasks = daemon.rpc(
        "batches/add_tasks",
        json!({"batch_id":batch_id,"tasks":[{"key":"one","name":"one","payload":{}}]}),
    );
    let claimed = daemon.rpc(
        "scheduler/claim",
        json!({"agent_id":"agent-a","batch_id":batch_id,"lease_seconds":3600}),
    )["task"]
        .clone();
    assert_eq!(claimed["attempt_no"], 1);
    let status = daemon.rpc(
        "scheduler/agent_status",
        json!({"agent_id":"agent-a","batch_id":batch_id}),
    );
    assert_eq!(status["active_claims"], 1);
    assert_eq!(status["batch_progress"]["total"], 1);
    assert_eq!(status["batch_progress"]["status"], "active");
    let deadline = chrono::DateTime::parse_from_rfc3339(claimed["lease_until"].as_str().unwrap())
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert!(deadline <= chrono::Utc::now() + chrono::Duration::seconds(601));

    let stale_legacy_extend = daemon.request(
        "scheduler/extend",
        json!({"id":claimed["id"],"agent_id":"agent-a","lease_token":claimed["lease_token"],"lease_seconds":600}),
    );
    assert!(stale_legacy_extend.get("error").is_some());

    let progress_id = Uuid::new_v4();
    let first_progress = progress_params(&claimed, "agent-a", 1, progress_id);
    let first = daemon.rpc("scheduler/progress", first_progress.clone());
    assert_eq!(first["ok"], true);
    assert_eq!(
        daemon.rpc("scheduler/progress", first_progress)["progress_at"],
        first["progress_at"]
    );
    let conflict = daemon.request(
        "scheduler/progress",
        progress_params(&claimed, "agent-a", 3, Uuid::new_v4()),
    );
    assert!(conflict.get("error").is_some());

    let extension_id = Uuid::new_v4();
    let extended = daemon.rpc(
        "scheduler/extend_progress",
        json!({
            "id":claimed["id"],"agent_id":"agent-a","lease_token":claimed["lease_token"],
            "attempt_no":claimed["attempt_no"],"progress_sequence":1,
            "request_id":extension_id,"lease_seconds":120
        }),
    );
    assert_eq!(extended["ok"], true);
    assert_eq!(
        daemon.rpc(
            "scheduler/extend_progress",
            json!({
                "id":claimed["id"],"agent_id":"agent-a","lease_token":claimed["lease_token"],
                "attempt_no":claimed["attempt_no"],"progress_sequence":1,
                "request_id":extension_id,"lease_seconds":120
            }),
        )["lease_until"],
        extended["lease_until"]
    );
    let progress_reuse = daemon.request(
        "scheduler/extend_progress",
        json!({
            "id":claimed["id"],"agent_id":"agent-a","lease_token":claimed["lease_token"],
            "attempt_no":claimed["attempt_no"],"progress_sequence":1,
            "request_id":Uuid::new_v4(),"lease_seconds":120
        }),
    );
    assert!(progress_reuse.get("error").is_some());
    assert_eq!(
        daemon.rpc(
            "scheduler/agent_status",
            json!({"agent_id":"agent-a","batch_id":batch_id})
        )["active_claims"],
        1
    );
    assert!(!daemon
        .rpc(
            "scheduler/agent_status",
            json!({"agent_id":"agent-a","batch_id":batch_id})
        )
        .to_string()
        .contains("lease_token"));

    let failed = daemon.rpc(
        "scheduler/failure",
        json!({"id":claimed["id"],"agent_id":"agent-a","lease_token":claimed["lease_token"],"error":"retry"}),
    );
    assert_eq!(failed["ok"], true);
    thread::sleep(Duration::from_millis(150));
    let reclaimed = daemon.rpc(
        "scheduler/claim",
        json!({"agent_id":"agent-b","batch_id":batch_id,"lease_seconds":30}),
    )["task"]
        .clone();
    assert_eq!(reclaimed["attempt_no"], 2);
    let old_progress = daemon.request(
        "scheduler/progress",
        json!({
            "id":reclaimed["id"],"agent_id":"agent-a","lease_token":claimed["lease_token"],
            "attempt_no":1,"sequence":2,"request_id":Uuid::new_v4(),"checkpoint":"late old worker"
        }),
    );
    assert!(old_progress.get("error").is_some());
    assert_eq!(tasks["tasks"][0]["attempt_no"], 0);
}

#[test]
fn progress_lease_budget_is_a_hard_total_cap() {
    let daemon = Daemon::new();
    let batch = daemon.rpc(
        "batches/create",
        json!({"name":"short progress budget","config":{"require_progress":true,"agent_lease_budget_seconds":1}}),
    );
    let batch_id = batch["id"].as_str().unwrap();
    daemon.rpc(
        "batches/add_tasks",
        json!({"batch_id":batch_id,"tasks":[{"key":"one","name":"one"}]}),
    );
    let task = daemon.rpc(
        "scheduler/claim",
        json!({"agent_id":"agent-a","batch_id":batch_id,"lease_seconds":600}),
    )["task"]
        .clone();
    thread::sleep(Duration::from_millis(1_100));
    let progress = daemon.rpc(
        "scheduler/progress",
        progress_params(&task, "agent-a", 1, Uuid::new_v4()),
    );
    assert_eq!(progress["ok"], true);
    let extension = daemon.request(
        "scheduler/extend_progress",
        json!({
            "id":task["id"],"agent_id":"agent-a","lease_token":task["lease_token"],
            "attempt_no":task["attempt_no"],"progress_sequence":1,
            "request_id":Uuid::new_v4(),"lease_seconds":300
        }),
    );
    assert!(extension.get("error").is_some());
}

#[test]
fn six_simulated_agents_claim_distinct_tasks_without_cross_identity_fences() {
    let daemon = common::Daemon::new();
    let batch = daemon.rpc(
        "batches/create",
        json!({"name":"six-agent isolated canary","config":{"require_progress":true}}),
    );
    let batch_id = batch["id"].as_str().unwrap().to_string();
    let tasks = (0..6)
        .map(|index| json!({"key":format!("task-{index}"),"name":format!("task-{index}")}))
        .collect::<Vec<_>>();
    daemon.rpc(
        "batches/add_tasks",
        json!({"batch_id":batch_id,"tasks":tasks}),
    );

    let client = daemon.http.clone();
    let url = daemon.url.clone();
    let barrier = Arc::new(Barrier::new(6));
    let claims = (0..6)
        .map(|index| {
            let client = client.clone();
            let url = url.clone();
            let barrier = barrier.clone();
            let batch_id = batch_id.clone();
            thread::spawn(move || {
                let agent_id = format!("A{index}");
                barrier.wait();
                let response: Value = client
                    .post(url)
                    .json(&json!({
                        "jsonrpc":"2.0",
                        "id":index,
                        "method":"scheduler/claim",
                        "params":{"agent_id":agent_id,"batch_id":batch_id,"lease_seconds":600}
                    }))
                    .send()
                    .unwrap()
                    .json()
                    .unwrap();
                assert!(response.get("error").is_none(), "{response}");
                (agent_id, response["result"]["task"].clone())
            })
        })
        .collect::<Vec<_>>();
    let claims = claims
        .into_iter()
        .map(|claim| claim.join().unwrap())
        .collect::<Vec<_>>();
    let ids = claims
        .iter()
        .map(|(_, task)| task["id"].as_str().unwrap())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(ids.len(), 6);
    assert!(claims
        .iter()
        .all(|(agent, task)| task["claimed_by"].as_str() == Some(agent.as_str())));
    assert!(claims
        .iter()
        .all(|(_, task)| task["lease_until"].is_string()));

    let (owner, task) = &claims[0];
    let cross_identity = daemon.request(
        "scheduler/extend",
        json!({
            "id":task["id"],
            "agent_id":if owner == "A0" {"A1"} else {"A0"},
            "lease_token":task["lease_token"],
            "lease_seconds":60
        }),
    );
    assert!(cross_identity.get("error").is_some());

    let status = daemon.rpc(
        "scheduler/agent_status",
        json!({"agent_id":owner,"batch_id":batch_id}),
    );
    assert_eq!(status["active_claims"], 1);
    assert!(!status.to_string().contains("lease_token"));
}

fn progress_claim(daemon: &Daemon) -> (Value, Value) {
    let batch = daemon.rpc(
        "batches/create",
        json!({"name":"g6 recovery","config":{"require_progress":true,"retry_backoff_ms":0}}),
    );
    daemon.rpc(
        "batches/add_tasks",
        json!({"batch_id":batch["id"],"tasks":[{"key":"one","name":"one"},{"key":"two","name":"two"}]}),
    );
    let task = daemon.rpc(
        "scheduler/claim",
        json!({"agent_id":"A0","batch_id":batch["id"],"lease_seconds":600}),
    )["task"]
        .clone();
    (batch, task)
}

#[test]
fn progress_receipt_replay_cannot_change_attempt() {
    let daemon = Daemon::new();
    let (_, task) = progress_claim(&daemon);
    let request = progress_params(&task, "A0", 1, Uuid::new_v4());
    let accepted = daemon.rpc("scheduler/progress", request.clone());
    let mut wrong_attempt = request.clone();
    wrong_attempt["attempt_no"] = json!(99);
    assert!(daemon
        .request("scheduler/progress", wrong_attempt)
        .get("error")
        .is_some());
    assert_eq!(daemon.rpc("scheduler/progress", request), accepted);
}

#[test]
fn extension_receipt_replay_cannot_change_identity_or_attempt() {
    let daemon = Daemon::new();
    let (_, task) = progress_claim(&daemon);
    daemon.rpc(
        "scheduler/progress",
        progress_params(&task, "A0", 1, Uuid::new_v4()),
    );
    let request = json!({"id":task["id"],"agent_id":"A0","lease_token":task["lease_token"],
        "attempt_no":task["attempt_no"],"progress_sequence":1,"request_id":Uuid::new_v4(),"lease_seconds":600});
    let accepted = daemon.rpc("scheduler/extend_progress", request.clone());
    for (key, value) in [("agent_id", json!("A1")), ("attempt_no", json!(99))] {
        let mut forged = request.clone();
        forged[key] = value;
        assert!(daemon
            .request("scheduler/extend_progress", forged)
            .get("error")
            .is_some());
    }
    assert_eq!(daemon.rpc("scheduler/extend_progress", request), accepted);
}

#[test]
fn stale_progress_and_disconnected_attempt_cannot_renew_or_submit() {
    let daemon = Daemon::new();
    let (batch, task) = progress_claim(&daemon);
    daemon.rpc(
        "scheduler/progress",
        progress_params(&task, "A0", 1, Uuid::new_v4()),
    );
    let connection = rusqlite::Connection::open(daemon.root.join("state.db")).unwrap();
    connection.busy_timeout(Duration::from_secs(5)).unwrap();
    connection
        .execute(
            "UPDATE task_progress SET progress_at=?1 WHERE task_id=?2",
            rusqlite::params![
                (chrono::Utc::now() - chrono::Duration::seconds(601)).to_rfc3339(),
                task["id"].as_str().unwrap()
            ],
        )
        .unwrap();
    let extension = json!({"id":task["id"],"agent_id":"A0","lease_token":task["lease_token"],
        "attempt_no":task["attempt_no"],"progress_sequence":1,"request_id":Uuid::new_v4(),"lease_seconds":600});
    assert!(daemon
        .request("scheduler/extend_progress", extension.clone())
        .get("error")
        .is_some());
    // Only the disposable daemon's database is adjusted to model a disconnect.
    connection
        .execute(
            "UPDATE tasks SET lease_until=?1 WHERE id=?2",
            rusqlite::params![
                (chrono::Utc::now() - chrono::Duration::seconds(2)).to_rfc3339(),
                task["id"].as_str().unwrap()
            ],
        )
        .unwrap();
    daemon.rpc("tasks/reclaim", json!({}));
    thread::sleep(Duration::from_millis(20));
    let replacement = daemon.rpc(
        "tasks/claim",
        json!({"id":task["id"],"agentId":"A1","leaseSeconds":60}),
    );
    assert_eq!(replacement["attempt_no"], 2);
    assert!(daemon
        .request("scheduler/extend_progress", extension)
        .get("error")
        .is_some());
    assert!(daemon
        .request("scheduler/submit", json!({"id":task["id"],"agent_id":"A0","lease_token":task["lease_token"],"result":{"stale":true}}))
        .get("error")
        .is_some());
    let submit = json!({"id":replacement["id"],"agent_id":"A1","lease_token":replacement["lease_token"],"result":{"verified":true}});
    let accepted = daemon.rpc("scheduler/submit", submit.clone());
    assert_eq!(accepted["ok"], true);
    assert_eq!(daemon.rpc("scheduler/submit", submit), accepted);
    let next = daemon.rpc(
        "scheduler/claim",
        json!({"agent_id":"A1","batch_id":batch["id"],"lease_seconds":60}),
    );
    assert_eq!(next["empty"], false);
    assert_ne!(next["task"]["id"], task["id"]);
}

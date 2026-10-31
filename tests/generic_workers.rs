mod common;

use coord::core::store::{CompleteOutcome, Store, TaskFilter};
use coord::core::types::{normalize_worker_id, BatchConfig, TaskInput};
use coord::core::validator::{Validation, ValidatorConfig};
use serde_json::json;
use uuid::Uuid;

fn store() -> Store {
    Store::open(&std::env::temp_dir().join(format!("coord-generic-{}.db", Uuid::new_v4()))).unwrap()
}

fn batch(store: &Store, config: BatchConfig) -> (Uuid, Uuid) {
    let created = store.create_batch("generic", "", config).unwrap();
    let batch_id = created["id"].as_str().unwrap().parse().unwrap();
    let task = store
        .add_tasks(
            batch_id,
            vec![TaskInput {
                key: "one".into(),
                name: "one".into(),
                payload: json!({"generic":true}),
            }],
        )
        .unwrap()
        .remove(0);
    (batch_id, task.id)
}

#[test]
fn worker_labels_are_bounded_and_historical_hex_is_canonical() {
    for worker in ["writer-1", "Writer-1", "a", "worker_2"] {
        assert_eq!(normalize_worker_id(worker).as_deref(), Some(worker));
    }
    assert_eq!(normalize_worker_id("aBcD").as_deref(), Some("ABCD"));
    assert_eq!(
        normalize_worker_id("EMOTE-abcd").as_deref(),
        Some("emote-ABCD")
    );
    for worker in ["", "-a", "../a", "hello world", "写作", &"a".repeat(65)] {
        assert!(normalize_worker_id(worker).is_none());
    }
}

#[test]
fn all_claim_entries_apply_the_selected_batch_budget() {
    for entry in 0..4 {
        let store = store();
        let (batch_id, task_id) = batch(
            &store,
            BatchConfig {
                require_progress: true,
                agent_lease_budget_seconds: 2,
                ..Default::default()
            },
        );
        let claim = match entry {
            0 => store.claim_task(task_id, "writer-1", Some(3600)),
            1 => store.claim_next("writer-1", Some(batch_id), Some(3600)),
            2 => store.claim_next("writer-1", None, Some(3600)),
            _ => store.claim_trusted(batch_id, "writer-1", Some(3600)),
        }
        .unwrap()
        .unwrap();
        assert_eq!(
            (claim.lease_until.unwrap() - claim.claimed_at.unwrap()).num_seconds(),
            2
        );
        assert!(store
            .extend_lease(
                task_id,
                "writer-1",
                claim.lease_token.as_deref().unwrap(),
                Some(3600)
            )
            .unwrap()
            .is_none());
    }
}

#[test]
fn trusted_failure_receipt_fences_retries_and_preserves_release_budget() {
    let store = store();
    let (batch_id, task_id) = batch(
        &store,
        BatchConfig {
            max_attempts: 2,
            retry_backoff_ms: 0,
            ..Default::default()
        },
    );
    let first = store
        .claim_trusted(batch_id, "writer-1", None)
        .unwrap()
        .unwrap();
    for (scope, worker, attempt) in [
        (Uuid::new_v4(), "writer-1", 1),
        (batch_id, "writer-2", 1),
        (batch_id, "writer-1", 2),
    ] {
        assert!(store
            .fail_trusted(task_id, scope, worker, attempt, "failed input")
            .unwrap()
            .is_none());
    }
    let receipt = store
        .fail_trusted(
            task_id,
            batch_id,
            "writer-1",
            first.attempt_no,
            "failed input",
        )
        .unwrap()
        .unwrap();
    assert_eq!(receipt["state"], "pending");
    assert_eq!(receipt["failure_count"], 1);
    assert_eq!(
        store
            .fail_trusted(task_id, batch_id, "writer-1", 1, "failed input")
            .unwrap()
            .unwrap()["replayed"],
        true
    );
    assert!(store
        .fail_trusted(task_id, batch_id, "writer-1", 1, "changed input")
        .is_err());
    std::thread::sleep(std::time::Duration::from_millis(5));
    let next = store
        .claim_trusted(batch_id, "writer-2", None)
        .unwrap()
        .unwrap();
    assert!(store
        .complete_trusted(task_id, batch_id, "writer-1", 1, json!({}), None)
        .unwrap()
        .is_none());
    assert_eq!(
        store
            .fail_trusted(task_id, batch_id, "writer-1", 1, "failed input")
            .unwrap()
            .unwrap()["state"],
        "pending"
    );
    store
        .release_trusted(task_id, batch_id, "writer-2", next.attempt_no, None)
        .unwrap()
        .unwrap();
    assert_eq!(store.get_task(task_id).unwrap().unwrap().failure_count, 1);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let last = store
        .claim_trusted(batch_id, "writer-1", None)
        .unwrap()
        .unwrap();
    assert_eq!(
        store
            .fail_trusted(task_id, batch_id, "writer-1", last.attempt_no, "permanent")
            .unwrap()
            .unwrap()["state"],
        "failed"
    );
    let progress = store.get_batch_status(batch_id).unwrap();
    assert_eq!(progress["status"], "completed");
    assert_eq!(progress["outcome"], "failed");
    assert_eq!(progress["all_succeeded"], false);
}

#[test]
fn rejected_results_remain_only_in_attempt_history() {
    let database =
        std::env::temp_dir().join(format!("coord-rejected-history-{}.db", Uuid::new_v4()));
    let store = Store::open(&database).unwrap();
    let (batch_id, task_id) = batch(
        &store,
        BatchConfig {
            validator: Some(ValidatorConfig::default()),
            retry_backoff_ms: 0,
            ..Default::default()
        },
    );
    let claim = store
        .claim_trusted(batch_id, "writer-1", None)
        .unwrap()
        .unwrap();
    store
        .complete_trusted(
            task_id,
            batch_id,
            "writer-1",
            claim.attempt_no,
            json!({"rejected":true}),
            None,
        )
        .unwrap()
        .unwrap();
    let job = store.begin_validation().unwrap().unwrap();
    assert!(store
        .finish_validation(&job, Validation::Fail("rejected".into()))
        .unwrap());
    let rejected = store.get_task(task_id).unwrap().unwrap();
    assert!(rejected.result.is_none());
    assert!(rejected.submission_id.is_none());
    let connection = rusqlite::Connection::open(&database).unwrap();
    let historical_result: String = connection
        .query_row(
            "SELECT result FROM task_attempts WHERE task_id=?1 AND attempt_no=?2",
            rusqlite::params![task_id.to_string(), claim.attempt_no],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&historical_result).unwrap(),
        json!({"rejected":true})
    );
    connection
        .execute(
            "UPDATE tasks SET result=?2 WHERE id=?1",
            rusqlite::params![task_id.to_string(), historical_result],
        )
        .unwrap();
    assert!(store.get_task(task_id).unwrap().unwrap().result.is_none());
    std::thread::sleep(std::time::Duration::from_millis(5));
    let next = store
        .claim_trusted(batch_id, "writer-2", None)
        .unwrap()
        .unwrap();
    assert!(next.result.is_none());
    assert!(store
        .complete_trusted(
            task_id,
            batch_id,
            "writer-1",
            claim.attempt_no,
            json!({"rejected":true}),
            None
        )
        .unwrap()
        .is_none());
    assert_eq!(
        store.resume_trusted(batch_id, "writer-1").unwrap()["checkpoint"],
        serde_json::Value::Null
    );
}

#[test]
fn historical_emote_attempt_resumes_without_actions_and_scoped_queries_are_read_only() {
    let store = store();
    let (batch_id, task_id) = batch(&store, BatchConfig::default());
    let old = store
        .claim_task(task_id, "emote-ABCD", None)
        .unwrap()
        .unwrap();
    let resumed = store
        .claim_trusted(batch_id, "abcd", None)
        .unwrap()
        .unwrap();
    assert_eq!(resumed.attempt_no, old.attempt_no);
    assert_eq!(
        store.resume_trusted(batch_id, "ABCD").unwrap()["state"],
        "active"
    );
    assert_eq!(
        store.resume_trusted(batch_id, "emote-abcd").unwrap()["state"],
        "active"
    );
    let (other_batch, _) = batch(&store, BatchConfig::default());
    let tasks = store
        .list_tasks_filtered(
            100,
            &TaskFilter {
                batch_id: Some(other_batch),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].batch_id, Some(other_batch));
    assert_eq!(
        store.get_task(task_id).unwrap().unwrap().attempt_no,
        old.attempt_no
    );
    assert!(matches!(
        store
            .complete_task(
                task_id,
                "emote-ABCD",
                old.lease_token.as_deref().unwrap(),
                json!({"ok":true}),
                None
            )
            .unwrap(),
        CompleteOutcome::Completed { .. }
    ));
    assert_eq!(
        store.get_batch_status(batch_id).unwrap()["outcome"],
        "succeeded"
    );
    store.cancel_batch(other_batch).unwrap();
    assert_eq!(
        store.get_batch_status(other_batch).unwrap()["outcome"],
        "cancelled"
    );
}

#[test]
fn historical_mixed_case_hex_claim_recovers_without_creating_an_alias() {
    let store = store();
    let (batch_id, task_id) = batch(&store, BatchConfig::default());
    let original = store.claim_task(task_id, "aBcD", None).unwrap().unwrap();
    let recovered = store
        .claim_trusted(batch_id, "ABCD", None)
        .unwrap()
        .unwrap();
    assert_eq!(recovered.id, original.id);
    assert_eq!(recovered.claimed_by.as_deref(), Some("aBcD"));
    assert_eq!(recovered.attempt_no, original.attempt_no);
    assert_eq!(
        store.resume_trusted(batch_id, "abcd").unwrap()["state"],
        "active"
    );
}

#[test]
fn rpc_reads_redact_secrets_while_legacy_claim_and_stdio_fail_work() {
    let daemon = common::Daemon::new();
    let task = daemon.rpc("tasks/send", json!({"name":"legacy"}));
    let claim = daemon.rpc("tasks/claim", json!({"id":task["id"],"agentId":"legacy"}));
    assert!(claim["lease_token"].is_string());
    assert!(daemon
        .rpc("tasks/get", json!({"id":task["id"]}))
        .get("lease_token")
        .is_none());
    for row in daemon
        .rpc("tasks/list", json!({"skip_reclaim":true}))
        .as_array()
        .unwrap()
    {
        assert!(row.get("lease_token").is_none());
    }
    let created = daemon.rpc("batches/create", json!({"name":"stdio generic"}));
    daemon.rpc(
        "batches/add_tasks",
        json!({"batch_id":created["id"],"tasks":[{"key":"one","name":"one"}]}),
    );
    let mut mcp = common::Mcp::new(&daemon.url);
    let owned = mcp.tool(
        "coord_claim",
        json!({"batch_id":created["id"],"worker_id":"writer-1"}),
    );
    let failed = mcp.tool("coord_fail", json!({"task_id":owned["id"],"batch_id":created["id"],"worker_id":"writer-1","attempt_no":owned["attempt_no"],"error":"business failure"}));
    assert_eq!(failed["failure_count"], 1);
    assert_eq!(
        mcp.tool("tasks_list", json!({"batch_id":created["id"]}))
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

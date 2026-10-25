mod common;

use coord::core::store::{Store, TaskFilter};
use coord::core::types::{BatchConfig, TaskInput, TaskState};
use serde_json::json;
use std::sync::{Arc, Barrier};

fn tempdir(label: &str) -> std::path::PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("coord-{label}-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn batch(store: &Store, max_attempts: u32) -> uuid::Uuid {
    let mut config = BatchConfig {
        max_attempts,
        retry_backoff_ms: 1,
        max_backoff_ms: 1,
        ..BatchConfig::default()
    };
    config.require_progress = false;
    let value = store.create_batch("trusted-scheduler", "", config).unwrap();
    value["id"].as_str().unwrap().parse().unwrap()
}

fn add(store: &Store, batch_id: uuid::Uuid, count: usize) {
    store
        .add_tasks(
            batch_id,
            (0..count)
                .map(|i| TaskInput {
                    key: format!("task-{i}"),
                    name: format!("task {i}"),
                    payload: json!({"i":i}),
                })
                .collect(),
        )
        .unwrap();
}

#[test]
fn checkpoint_resume_complete_are_persistent_and_idempotent() {
    let root = tempdir("trusted-resume");
    let db = root.join("state.db");
    let store = Store::open(&db).unwrap();
    let batch_id = batch(&store, 3);
    add(&store, batch_id, 1);
    let claim = store
        .claim_trusted(batch_id, "worker-a", Some(60))
        .unwrap()
        .unwrap();
    let attempt_no = claim.attempt_no;
    let task_id = claim.id;
    let first = store
        .progress_trusted(
            task_id,
            batch_id,
            "worker-a",
            attempt_no,
            "downloaded inputs",
            Some(60),
        )
        .unwrap()
        .unwrap();
    assert_eq!(first["sequence"], 1);
    let repeated = store
        .progress_trusted(
            task_id,
            batch_id,
            "worker-a",
            attempt_no,
            "downloaded inputs",
            Some(60),
        )
        .unwrap()
        .unwrap();
    assert_eq!(repeated["replayed"], true);
    let later = store
        .progress_trusted(
            task_id,
            batch_id,
            "worker-a",
            attempt_no,
            "parsed manifest",
            Some(60),
        )
        .unwrap()
        .unwrap();
    assert_eq!(later["sequence"], 2);
    let old_replay = store
        .progress_trusted(
            task_id,
            batch_id,
            "worker-a",
            attempt_no,
            "downloaded inputs",
            Some(60),
        )
        .unwrap()
        .unwrap();
    assert_eq!(old_replay["sequence"], 1);
    assert_eq!(old_replay["replayed"], true);
    drop(store);

    let store = Store::open(&db).unwrap();
    let resumed = store.resume_trusted(batch_id, "worker-a").unwrap();
    assert_eq!(resumed["state"], "active");
    assert_eq!(resumed["checkpoint"], "parsed manifest");
    assert_eq!(resumed["task"]["id"], task_id.to_string());
    assert!(resumed["task"].get("lease_token").is_none());

    let result = json!({"answer":42});
    let completed = store
        .complete_trusted(
            task_id,
            batch_id,
            "worker-a",
            attempt_no,
            result.clone(),
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(completed["state"], "completed");
    assert_eq!(completed["replayed"], false);
    let replay = store
        .complete_trusted(task_id, batch_id, "worker-a", attempt_no, result, None)
        .unwrap()
        .unwrap();
    assert_eq!(replay["state"], "completed");
    assert_eq!(replay["replayed"], true);
    assert!(store
        .complete_trusted(
            task_id,
            batch_id,
            "worker-a",
            attempt_no,
            json!({"answer":43}),
            None,
        )
        .is_err());
    let final_task = store.get_task(task_id).unwrap().unwrap();
    assert_eq!(final_task.state, TaskState::Completed);
    assert_eq!(final_task.attempt_count, 1);
    assert_eq!(final_task.failure_count, 0);
    assert_eq!(store.status_trusted(batch_id).unwrap()["active_claims"], 0);
}

#[test]
fn release_and_expiration_preserve_failure_budget_and_old_fences() {
    let root = tempdir("trusted-release");
    let store = Store::open(&root.join("state.db")).unwrap();
    let batch_id = batch(&store, 1);
    add(&store, batch_id, 1);
    let first = store
        .claim_trusted(batch_id, "worker-a", Some(60))
        .unwrap()
        .unwrap();
    let released = store
        .release_trusted(
            first.id,
            batch_id,
            "worker-a",
            first.attempt_no,
            Some("context lost"),
        )
        .unwrap()
        .unwrap();
    assert_eq!(released["state"], "pending");
    let after_release = store.get_task(first.id).unwrap().unwrap();
    assert_eq!(after_release.failure_count, 0);
    assert_eq!(after_release.attempt_count, 1);
    assert_eq!(after_release.interruption_streak, 1);
    assert_eq!(
        store.resume_trusted(batch_id, "worker-a").unwrap()["state"],
        "released"
    );
    std::thread::sleep(std::time::Duration::from_millis(10));
    let second = store
        .claim_trusted(batch_id, "worker-b", Some(60))
        .unwrap()
        .unwrap();
    assert_eq!(second.id, first.id);
    assert_eq!(second.attempt_no, first.attempt_no + 1);
    assert!(store
        .release_trusted(first.id, batch_id, "worker-a", first.attempt_no, None)
        .unwrap()
        .is_none());
    assert!(store
        .progress_trusted(
            first.id,
            batch_id,
            "worker-a",
            first.attempt_no,
            "stale checkpoint",
            Some(30),
        )
        .unwrap()
        .is_none());
    assert!(store
        .complete_trusted(
            first.id,
            batch_id,
            "worker-a",
            first.attempt_no,
            json!({"late":true}),
            None,
        )
        .unwrap()
        .is_none());
    let token = second.lease_token.as_deref().unwrap();
    assert!(store
        .report_failure(second.id, "worker-b", token, json!({"kind":"real_failure"}))
        .unwrap());
    let failed = store.get_task(first.id).unwrap().unwrap();
    assert_eq!(failed.state, TaskState::Failed);
    assert_eq!(failed.failure_count, 1);
    assert_eq!(failed.attempt_count, 2);
    assert_eq!(store.retry_failed_tasks(batch_id).unwrap(), 1);
    let retried = store.get_task(first.id).unwrap().unwrap();
    assert_eq!(retried.state, TaskState::Pending);
    assert_eq!(retried.failure_count, 1);
    assert_eq!(retried.attempt_count, 2);
    assert_eq!(retried.max_attempts, 2);
}

#[test]
fn concurrent_trusted_claims_have_unique_tasks_and_one_claim_per_worker() {
    let root = tempdir("trusted-concurrency");
    let db = root.join("state.db");
    let store = Store::open(&db).unwrap();
    let batch_id = batch(&store, 3);
    add(&store, batch_id, 6);
    drop(store);

    let barrier = Arc::new(Barrier::new(6));
    let workers = (0..6)
        .map(|i| {
            let path = db.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let store = Store::open(&path).unwrap();
                barrier.wait();
                store
                    .claim_trusted(batch_id, &format!("worker-{i}"), Some(60))
                    .unwrap()
                    .unwrap()
                    .id
            })
        })
        .collect::<Vec<_>>();
    let claimed = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(claimed.len(), 6);

    let store = Store::open(&db).unwrap();
    let all_claimed = store
        .list_tasks_filtered(
            20,
            &TaskFilter {
                state: Some("claimed".into()),
                ..TaskFilter::default()
            },
        )
        .unwrap();
    assert_eq!(all_claimed.len(), 6);
    assert!(all_claimed
        .iter()
        .all(|task| task.attempt_no == 1 && task.attempt_count == 1));
}

#[test]
fn reassigned_worker_resumes_from_the_last_durable_checkpoint() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let batch_id = batch(&store, 3);
    add(&store, batch_id, 1);
    let first = store
        .claim_trusted(batch_id, "worker-a", Some(60))
        .unwrap()
        .unwrap();
    store
        .progress_trusted(
            first.id,
            batch_id,
            "worker-a",
            first.attempt_no,
            "checkpoint before handoff",
            Some(60),
        )
        .unwrap()
        .unwrap();
    store
        .release_trusted(
            first.id,
            batch_id,
            "worker-a",
            first.attempt_no,
            Some("context lost"),
        )
        .unwrap()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));

    let second = store
        .claim_trusted(batch_id, "worker-b", Some(60))
        .unwrap()
        .unwrap();
    assert_eq!(second.id, first.id);
    assert_eq!(second.attempt_no, first.attempt_no + 1);
    let old_worker = store.resume_trusted(batch_id, "worker-a").unwrap();
    assert_eq!(old_worker["state"], "reassigned");
    assert_eq!(old_worker["checkpoint"], "checkpoint before handoff");
    let new_worker = store.resume_trusted(batch_id, "worker-b").unwrap();
    assert_eq!(new_worker["state"], "active");
    assert_eq!(new_worker["checkpoint"], "checkpoint before handoff");
}

#[test]
fn resume_reports_expiry_without_mutating_and_reclaimer_keeps_failure_budget() {
    let root = tempdir("trusted-expiry");
    let store = Store::open(&root.join("state.db")).unwrap();
    let batch_id = batch(&store, 2);
    add(&store, batch_id, 1);
    let claim = store
        .claim_trusted(batch_id, "worker-a", Some(1))
        .unwrap()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1_150));
    let resumed = store.resume_trusted(batch_id, "worker-a").unwrap();
    assert_eq!(resumed["state"], "expired");
    assert_eq!(
        store.get_task(claim.id).unwrap().unwrap().state,
        TaskState::Claimed
    );
    let reclaimed = store.reclaim_expired_leases().unwrap();
    assert_eq!(reclaimed.len(), 1);
    assert_eq!(reclaimed[0].failure_count, 0);
    assert_eq!(reclaimed[0].attempt_count, 1);
    assert_eq!(reclaimed[0].interruption_streak, 1);
    assert_eq!(
        store.resume_trusted(batch_id, "worker-a").unwrap()["state"],
        "expired"
    );
}

#[test]
fn release_and_complete_race_has_one_atomic_winner() {
    let root = tempdir("trusted-release-complete-race");
    let db = root.join("state.db");
    let store = Arc::new(Store::open(&db).unwrap());
    let batch_id = batch(&store, 3);
    add(&store, batch_id, 1);
    let claim = store
        .claim_trusted(batch_id, "worker-a", Some(60))
        .unwrap()
        .unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let release_store = store.clone();
    let release_barrier = barrier.clone();
    let release = std::thread::spawn(move || {
        release_barrier.wait();
        release_store
            .release_trusted(claim.id, batch_id, "worker-a", claim.attempt_no, None)
            .unwrap()
    });
    let complete_store = store.clone();
    let complete_barrier = barrier.clone();
    let task_id = claim.id;
    let attempt_no = claim.attempt_no;
    let complete = std::thread::spawn(move || {
        complete_barrier.wait();
        complete_store
            .complete_trusted(
                task_id,
                batch_id,
                "worker-a",
                attempt_no,
                json!({"winner":"complete"}),
                None,
            )
            .unwrap()
    });
    barrier.wait();
    let released = release.join().unwrap();
    let completed = complete.join().unwrap();
    assert_ne!(released.is_some(), completed.is_some());
    let task = store.get_task(task_id).unwrap().unwrap();
    assert!(matches!(
        task.state,
        TaskState::Pending | TaskState::Completed
    ));
    assert_eq!(task.failure_count, 0);
    if task.state == TaskState::Completed {
        assert_eq!(
            store.status_trusted(batch_id).unwrap()["states"]["completed"],
            1
        );
    } else {
        assert_eq!(task.interruption_streak, 1);
    }
}

#[test]
fn trusted_http_rpc_redacts_token_and_keeps_legacy_api_available() {
    let daemon = common::Daemon::new();
    let batch = daemon.rpc("batches/create", json!({"name":"trusted HTTP"}));
    let batch_id = batch["id"].as_str().unwrap();
    daemon.rpc(
        "batches/add_tasks",
        json!({"batch_id":batch_id,"tasks":[{"key":"one","name":"one","payload":{}}]}),
    );
    let claimed = daemon.rpc(
        "scheduler/trusted_claim",
        json!({"batch_id":batch_id,"worker_id":"00A1"}),
    );
    assert_eq!(claimed["attempt_no"], 1);
    assert!(claimed.get("lease_token").is_none());
    let task_id = claimed["id"].as_str().unwrap();
    let progress = daemon.rpc(
        "scheduler/trusted_progress",
        json!({"task_id":task_id,"batch_id":batch_id,"worker_id":"00A1","attempt_no":1,"checkpoint":"received"}),
    );
    assert_eq!(progress["sequence"], 1);
    let resumed = daemon.rpc(
        "scheduler/trusted_resume",
        json!({"batch_id":batch_id,"worker_id":"00A1"}),
    );
    assert_eq!(resumed["state"], "active");
    assert_eq!(resumed["checkpoint"], "received");
    assert!(resumed["task"].get("lease_token").is_none());
    let completed = daemon.rpc(
        "scheduler/trusted_complete",
        json!({"task_id":task_id,"batch_id":batch_id,"worker_id":"00A1","attempt_no":1,"result":{"ok":true}}),
    );
    assert_eq!(completed["state"], "completed");
    assert_eq!(
        daemon.rpc("scheduler/trusted_status", json!({"batch_id":batch_id}))["active_claims"],
        0
    );
    let duplicate = daemon.request(
        "scheduler/trusted_progress",
        json!({"task_id":task_id,"batch_id":batch_id,"worker_id":"00A1","attempt_no":1,"checkpoint":"forbidden","lease_token":"not accepted"}),
    );
    assert!(duplicate.get("error").is_some());
    let legacy_task = daemon.rpc("tasks/send", json!({"name":"legacy","payload":{}}));
    let legacy_claim = daemon.rpc(
        "tasks/claim",
        json!({"id":legacy_task["id"],"agentId":"legacy"}),
    );
    assert!(legacy_claim["lease_token"].as_str().is_some());
}

#[test]
fn old_database_migration_preserves_counts_results_and_checkpoints_idempotently() {
    let root = tempdir("trusted-migration");
    let db = root.join("old-state.db");
    let active_id = uuid::Uuid::new_v4();
    let pending_id = uuid::Uuid::new_v4();
    let completed_id = uuid::Uuid::new_v4();
    let batch_id = uuid::Uuid::new_v4();
    let now = chrono::Utc::now();
    let old = rusqlite::Connection::open(&db).unwrap();
    old.execute_batch(
        "CREATE TABLE tasks(
            id TEXT PRIMARY KEY,name TEXT NOT NULL,state TEXT NOT NULL,claimed_by TEXT,
            payload TEXT NOT NULL,result TEXT,created_at TEXT NOT NULL,updated_at TEXT NOT NULL,
            kind TEXT NOT NULL DEFAULT 'task',priority TEXT NOT NULL DEFAULT 'normal',
            claimed_at TEXT,lease_until TEXT,lease_token TEXT,attempt_no INTEGER NOT NULL DEFAULT 0,
            attempt_count INTEGER NOT NULL DEFAULT 0,max_attempts INTEGER NOT NULL DEFAULT 3,
            available_at TEXT,batch_id TEXT,task_key TEXT,error TEXT,completed_at TEXT,
            submission_id TEXT,validation_attempts INTEGER NOT NULL DEFAULT 0,
            validation_error_count INTEGER NOT NULL DEFAULT 0,validation_token TEXT,
            validation_until TEXT,pending_ack TEXT
        );
        CREATE TABLE task_progress(
            task_id TEXT NOT NULL,lease_token TEXT NOT NULL,agent_id TEXT NOT NULL,
            attempt_no INTEGER NOT NULL,sequence INTEGER NOT NULL,checkpoint TEXT NOT NULL,
            progress_at TEXT NOT NULL,last_extended_sequence INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY(task_id,lease_token)
        );",
    )
    .unwrap();
    old.execute(
        "INSERT INTO tasks(id,name,state,claimed_by,payload,created_at,updated_at,claimed_at,lease_until,lease_token,attempt_no,attempt_count,max_attempts,batch_id)
         VALUES(?1,'active','claimed','worker-old','{}',?2,?2,?3,?4,'opaque-legacy-token',4,4,5,?5)",
        rusqlite::params![active_id.to_string(), now.to_rfc3339(), (now-chrono::Duration::minutes(1)).to_rfc3339(), (now+chrono::Duration::minutes(10)).to_rfc3339(), batch_id.to_string()],
    ).unwrap();
    old.execute(
        "INSERT INTO tasks(id,name,state,payload,created_at,updated_at,attempt_no,attempt_count,max_attempts)
         VALUES(?1,'pending','pending','{}',?2,?2,7,7,10)",
        rusqlite::params![pending_id.to_string(), now.to_rfc3339()],
    ).unwrap();
    old.execute(
        "INSERT INTO tasks(id,name,state,claimed_by,payload,result,created_at,updated_at,completed_at,attempt_no,attempt_count,max_attempts)
         VALUES(?1,'completed','completed','worker-old','{}','{\"kept\":true}',?2,?2,?2,3,3,5)",
        rusqlite::params![completed_id.to_string(), now.to_rfc3339()],
    ).unwrap();
    old.execute(
        "INSERT INTO task_progress(task_id,lease_token,agent_id,attempt_no,sequence,checkpoint,progress_at)
         VALUES(?1,'opaque-legacy-token','worker-old',4,2,'migrated checkpoint',?2)",
        rusqlite::params![active_id.to_string(), now.to_rfc3339()],
    ).unwrap();
    drop(old);

    let store = Store::open(&db).unwrap();
    let active = store.get_task(active_id).unwrap().unwrap();
    assert_eq!(active.attempt_no, 4);
    assert_eq!(active.attempt_count, 4);
    assert_eq!(active.failure_count, 3);
    let pending = store.get_task(pending_id).unwrap().unwrap();
    assert_eq!(pending.attempt_no, 7);
    assert_eq!(pending.attempt_count, 7);
    assert_eq!(pending.failure_count, 7);
    let completed = store.get_task(completed_id).unwrap().unwrap();
    assert_eq!(completed.attempt_no, 3);
    assert_eq!(completed.result, Some(json!({"kept":true})));
    assert_eq!(completed.failure_count, 3);
    let resumed = store.resume_trusted(batch_id, "worker-old").unwrap();
    assert_eq!(resumed["state"], "active");
    assert_eq!(resumed["checkpoint"], "migrated checkpoint");
    assert_eq!(resumed["progress_sequence"], 2);
    drop(store);

    let reopened = Store::open(&db).unwrap();
    assert_eq!(
        reopened.get_task(active_id).unwrap().unwrap().failure_count,
        3
    );
    assert_eq!(
        reopened.resume_trusted(batch_id, "worker-old").unwrap()["checkpoint"],
        "migrated checkpoint"
    );
    drop(reopened);
    let check =
        rusqlite::Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let integrity: String = check
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
}

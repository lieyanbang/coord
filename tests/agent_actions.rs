mod common;

use coord::core::actions::{ActionRegistry, ActionState};
use coord::core::store::{CompleteOutcome, Store};
use coord::core::types::{BatchConfig, TaskInput};
use coord::core::validator::{Validation, ValidatorConfig};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

#[test]
#[ignore = "invoked by the isolated completion-action canary"]
fn action_child_echo_stdin() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    println!("received {} bytes", input.len());
}

#[test]
#[ignore = "invoked by the isolated completion-action canary"]
fn action_child_append_effect_marker() {
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("effect-marker.txt")
        .unwrap()
        .write_all(b"x")
        .unwrap();
}

#[test]
#[ignore = "invoked by the isolated completion-action canary"]
fn action_child_append_effect_then_wait() {
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("effect-marker.txt")
        .unwrap()
        .write_all(b"x")
        .unwrap();
    std::thread::sleep(std::time::Duration::from_secs(10));
}

#[test]
#[ignore = "invoked by the isolated completion-action canary"]
fn action_child_fail() {
    panic!("canary nonzero exit");
}

#[test]
#[ignore = "invoked by the isolated completion-action canary"]
fn action_child_overflow() {
    println!("{}", "x".repeat(8192));
}

#[test]
#[ignore = "invoked by the isolated completion-action canary"]
fn action_child_hang() {
    std::thread::sleep(std::time::Duration::from_secs(10));
}

#[test]
#[ignore = "invoked by the isolated completion-action canary"]
fn action_child_requires_kernel_job_before_first_effect() {
    use windows_sys::Win32::System::JobObjects::IsProcessInJob;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut contained = 0;
    unsafe {
        assert_ne!(
            IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut contained),
            0
        );
    }
    assert_ne!(
        contained, 0,
        "action began executing before Job Object containment"
    );
    fs::write("contained-first-effect.txt", b"contained").unwrap();
}

struct Harness {
    root: PathBuf,
    db_path: PathBuf,
    registry_path: PathBuf,
    store: Option<Store>,
    registry: Arc<ActionRegistry>,
}

impl Harness {
    fn new(
        child_test: &str,
        idempotent: bool,
        max_attempts: u32,
        timeout_ms: u64,
        max_output_bytes: usize,
        include_result: bool,
        include_batch_action: bool,
    ) -> Self {
        let root = std::env::temp_dir().join(format!("coord-actions-{}", Uuid::new_v4()));
        let trusted = root.join("trusted");
        fs::create_dir_all(&trusted).unwrap();
        let executable = trusted.join("action-canary.exe");
        fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
        let args = vec![
            "--exact".to_string(),
            child_test.to_string(),
            "--ignored".to_string(),
            "--nocapture".to_string(),
        ];
        let command = json!({
            "executable": executable,
            "working_directory": trusted,
            "args": args,
            "include_result": include_result,
            "timeout_ms": timeout_ms,
            "max_output_bytes": max_output_bytes,
            "max_attempts": max_attempts,
            "idempotent": idempotent,
        });
        let registry_path = root.join("actions.json");
        let profile = json!({
            "version": 1,
            "on_task_completed": command,
            "on_batch_completed": if include_batch_action { Some(command.clone()) } else { None },
        });
        fs::write(
            &registry_path,
            serde_json::to_vec(&json!({
                "version": 1,
                "isolated_in_process": true,
                "allowed_root": trusted,
                "profiles": { "canary": profile },
            }))
            .unwrap(),
        )
        .unwrap();
        let registry = Arc::new(ActionRegistry::load(&registry_path).unwrap());
        let db_path = root.join("state.db");
        let store = Store::open_with_actions(&db_path, registry.clone()).unwrap();
        Self {
            root,
            db_path,
            registry_path,
            store: Some(store),
            registry,
        }
    }

    fn store(&self) -> &Store {
        self.store.as_ref().unwrap()
    }

    fn reopen(&mut self) {
        self.store.take();
        self.store = Some(Store::open_with_actions(&self.db_path, self.registry.clone()).unwrap());
    }

    fn create_batch(&self, validator: Option<ValidatorConfig>) -> Uuid {
        let config = BatchConfig {
            action_profile: Some("canary".into()),
            validator,
            ..BatchConfig::default()
        };
        let value = self
            .store()
            .create_batch("phase-g-isolated-canary", "", config)
            .unwrap();
        Uuid::parse_str(value["id"].as_str().unwrap()).unwrap()
    }

    fn add_task(&self, batch_id: Uuid, key: &str) -> Uuid {
        self.store()
            .add_tasks(
                batch_id,
                vec![TaskInput {
                    key: key.into(),
                    name: key.into(),
                    payload: json!({}),
                }],
            )
            .unwrap()
            .remove(0)
            .id
    }

    fn claim(&self, batch_id: Uuid, agent_id: &str) -> coord::core::types::Task {
        self.store()
            .claim_next(agent_id, Some(batch_id), Some(30))
            .unwrap()
            .unwrap()
    }

    fn complete(&self, task: &coord::core::types::Task, result: Value) {
        let outcome = self
            .store()
            .complete_task(
                task.id,
                task.claimed_by.as_deref().unwrap(),
                task.lease_token.as_deref().unwrap(),
                result,
                None,
            )
            .unwrap();
        assert!(matches!(outcome, CompleteOutcome::Completed { .. }));
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.store.take();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn action_registry_rejects_untrusted_paths_dynamic_placeholders_and_shell_fields() {
    let root = std::env::temp_dir().join(format!("coord-action-policy-{}", Uuid::new_v4()));
    let trusted = root.join("trusted");
    fs::create_dir_all(&trusted).unwrap();
    let current_exe = std::env::current_exe().unwrap();
    let trusted_exe = trusted.join("action.exe");
    let outside_exe = root.join("outside.exe");
    fs::copy(&current_exe, &trusted_exe).unwrap();
    fs::copy(current_exe, &outside_exe).unwrap();
    let working_directory = trusted.clone();
    let registry_path = root.join("actions.json");
    let write_registry = |command: Value| {
        fs::write(
            &registry_path,
            serde_json::to_vec(&json!({
                "version": 1,
                "allowed_root": trusted,
                "profiles": {
                    "canary": {
                        "version": 1,
                        "on_task_completed": command,
                        "on_batch_completed": null
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
    };
    let command = |executable: PathBuf, args: Vec<&str>| {
        json!({
            "executable": executable,
            "working_directory": working_directory,
            "args": args,
            "include_result": false,
            "timeout_ms": 1000,
            "max_output_bytes": 1024,
            "max_attempts": 1,
            "idempotent": false
        })
    };

    write_registry(command(outside_exe, vec![]));
    assert!(ActionRegistry::load(&registry_path).is_err());

    write_registry(command(trusted_exe.clone(), vec!["{untrusted}"]));
    assert!(ActionRegistry::load(&registry_path).is_err());

    let mut shell_command = command(trusted_exe, vec![]);
    shell_command["shell"] = json!(true);
    write_registry(shell_command);
    assert!(ActionRegistry::load(&registry_path).is_err());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn completion_and_outbox_are_atomic_and_replay_does_not_duplicate_events() {
    let harness = Harness::new(
        "action_child_echo_stdin",
        true,
        2,
        2_000,
        32_768,
        true,
        true,
    );
    let batch = harness.create_batch(None);
    let id = harness.add_task(batch, "one");
    let task = harness.claim(batch, "A0");
    assert_eq!(task.id, id);
    let result = json!({
        "safe": "kept",
        "lease_token": "canary-secret-marker",
        "nested": { "api_token": "another-marker", "visible": true }
    });
    harness.complete(&task, result.clone());
    let replay = harness
        .store()
        .complete_task(
            task.id,
            task.claimed_by.as_deref().unwrap(),
            task.lease_token.as_deref().unwrap(),
            result,
            None,
        )
        .unwrap();
    assert!(matches!(replay, CompleteOutcome::Completed { .. }));

    let status = harness.store().action_status(batch).unwrap();
    assert_eq!(status["actions"].as_array().unwrap().len(), 2);
    assert!(!status.to_string().contains("lease_token"));
    assert!(!status.to_string().contains("canary-secret-marker"));

    let db = Connection::open(&harness.db_path).unwrap();
    let payload: String = db
        .query_row(
            "SELECT payload FROM completion_actions WHERE action_kind='on_task_completed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!payload.contains("lease_token"));
    assert!(!payload.contains("canary-secret-marker"));
    assert!(payload.contains("\"safe\":\"kept\""));
}

#[test]
fn outbox_insert_failure_rolls_back_task_completion() {
    let harness = Harness::new(
        "action_child_echo_stdin",
        true,
        1,
        2_000,
        32_768,
        false,
        false,
    );
    let batch = harness.create_batch(None);
    harness.add_task(batch, "one");
    let task = harness.claim(batch, "A0");
    let db = Connection::open(&harness.db_path).unwrap();
    db.execute_batch(
        "CREATE TRIGGER reject_action_insert BEFORE INSERT ON completion_actions
         BEGIN SELECT RAISE(ABORT,'injected outbox failure'); END;",
    )
    .unwrap();
    let error = harness
        .store()
        .complete_task(
            task.id,
            "A0",
            task.lease_token.as_deref().unwrap(),
            json!({"ok":true}),
            None,
        )
        .unwrap_err();
    assert!(error.to_string().contains("injected outbox failure"));
    let current = harness.store().get_task(task.id).unwrap().unwrap();
    assert_eq!(current.state.as_str(), "claimed");
    assert!(harness.store().action_status(batch).unwrap()["actions"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn validator_pass_enqueues_the_task_action_and_batch_action_once() {
    let harness = Harness::new(
        "action_child_echo_stdin",
        true,
        1,
        2_000,
        32_768,
        true,
        true,
    );
    let validator = ValidatorConfig {
        required: vec!["accepted".into()],
        ..ValidatorConfig::default()
    };
    let batch = harness.create_batch(Some(validator));
    harness.add_task(batch, "one");
    let task = harness.claim(batch, "A0");
    let submitted = harness
        .store()
        .complete_task(
            task.id,
            "A0",
            task.lease_token.as_deref().unwrap(),
            json!({"accepted":true}),
            None,
        )
        .unwrap();
    assert!(matches!(submitted, CompleteOutcome::Submitted { .. }));
    let validation = harness.store().begin_validation().unwrap().unwrap();
    assert!(harness
        .store()
        .finish_validation(&validation, Validation::Pass)
        .unwrap());
    let status = harness.store().action_status(batch).unwrap();
    assert_eq!(status["actions"].as_array().unwrap().len(), 2);
    assert_eq!(status["actions"][0]["state"], "pending");
}

#[test]
fn idempotent_action_lease_recovers_after_restart_and_fences_stale_worker() {
    let mut harness = Harness::new(
        "action_child_append_effect_marker",
        true,
        2,
        2_000,
        32_768,
        false,
        false,
    );
    let batch = harness.create_batch(None);
    harness.add_task(batch, "one");
    let task = harness.claim(batch, "A0");
    harness.complete(&task, json!({"ok":true}));
    let stale = harness
        .store()
        .begin_action("worker-before-crash")
        .unwrap()
        .unwrap();
    let first_execution = harness.registry.execute(&stale);
    assert_eq!(first_execution.state, ActionState::Succeeded);
    let db = Connection::open(&harness.db_path).unwrap();
    db.execute(
        "UPDATE completion_actions SET lease_until=datetime('now','-1 second') WHERE id=?1",
        [stale.id.as_str()],
    )
    .unwrap();
    drop(db);
    harness.reopen();
    let recovered = harness
        .store()
        .begin_action("worker-after-restart")
        .unwrap()
        .unwrap();
    assert_eq!(recovered.id, stale.id);
    assert_ne!(recovered.lease_token, stale.lease_token);
    let success = harness.registry.execute(&recovered);
    assert_eq!(success.state, ActionState::Succeeded);
    assert_eq!(
        fs::read_to_string(harness.root.join("trusted/effect-marker.txt")).unwrap(),
        "xx"
    );
    assert!(!harness.store().finish_action(&stale, &success).unwrap());
    assert!(harness.store().finish_action(&recovered, &success).unwrap());
    let task_state = harness.store().get_task(task.id).unwrap().unwrap().state;
    assert_eq!(task_state.as_str(), "completed");
}

#[test]
fn non_idempotent_expired_action_becomes_uncertain_without_replay() {
    let mut harness = Harness::new(
        "action_child_append_effect_marker",
        false,
        1,
        2_000,
        32_768,
        false,
        false,
    );
    let batch = harness.create_batch(None);
    harness.add_task(batch, "one");
    let task = harness.claim(batch, "A0");
    harness.complete(&task, json!({"ok":true}));
    let leased = harness
        .store()
        .begin_action("worker-before-crash")
        .unwrap()
        .unwrap();
    let execution = harness.registry.execute(&leased);
    assert_eq!(execution.state, ActionState::Succeeded);
    let db = Connection::open(&harness.db_path).unwrap();
    db.execute(
        "UPDATE completion_actions SET lease_until=datetime('now','-1 second')",
        [],
    )
    .unwrap();
    drop(db);
    harness.reopen();
    assert!(harness
        .store()
        .begin_action("worker-after-restart")
        .unwrap()
        .is_none());
    let status = harness.store().action_status(batch).unwrap();
    assert_eq!(status["actions"][0]["state"], "uncertain");
    assert_eq!(
        fs::read_to_string(harness.root.join("trusted/effect-marker.txt")).unwrap(),
        "x"
    );
    assert_eq!(
        harness
            .store()
            .get_task(task.id)
            .unwrap()
            .unwrap()
            .state
            .as_str(),
        "completed"
    );
}

#[test]
fn daemon_crash_after_action_side_effect_recovers_as_uncertain_without_replay() {
    let harness = Harness::new(
        "action_child_append_effect_then_wait",
        false,
        1,
        20_000,
        32_768,
        false,
        false,
    );
    let daemon = common::Daemon::with_actions_config(harness.registry_path.clone());
    let batch = daemon.rpc(
        "batches/create",
        json!({"name":"post-side-effect crash canary","config":{"action_profile":"canary","max_attempts":1}}),
    );
    let batch_id = batch["id"].as_str().unwrap();
    daemon.rpc(
        "batches/add_tasks",
        json!({"batch_id":batch_id,"tasks":[{"key":"one","name":"one"}]}),
    );
    let task = daemon.rpc(
        "scheduler/claim",
        json!({"agent_id":"A0","batch_id":batch_id,"lease_seconds":30}),
    )["task"]
        .clone();
    daemon.rpc(
        "scheduler/submit",
        json!({"id":task["id"],"agent_id":"A0","lease_token":task["lease_token"],"result":{"accepted":true}}),
    );

    let marker = harness.root.join("trusted/effect-marker.txt");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !marker.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "action child did not write its side effect: {:?}",
            daemon.root.join("daemon.log")
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(fs::read_to_string(&marker).unwrap(), "x");

    let mut daemon = daemon;
    daemon.stop();
    let db = Connection::open(daemon.root.join("state.db")).unwrap();
    db.execute(
        "UPDATE completion_actions SET lease_until='2000-01-01T00:00:00Z' WHERE state='running'",
        [],
    )
    .unwrap();
    drop(db);
    daemon.start();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let status = daemon.rpc("actions/status", json!({"batch_id":batch_id}));
        if status["actions"][0]["state"] == "uncertain" {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "action did not recover to uncertain: {status}"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(fs::read_to_string(marker).unwrap(), "x");
    assert_eq!(
        daemon.rpc("tasks/get", json!({"id":task["id"]}))["state"],
        "completed"
    );
}

#[test]
fn idempotent_failures_retry_with_a_new_fence_without_reopening_the_task() {
    let harness = Harness::new("action_child_fail", true, 2, 2_000, 32_768, false, false);
    let batch = harness.create_batch(None);
    let task_id = harness.add_task(batch, "one");
    let task = harness.claim(batch, "A0");
    harness.complete(&task, json!({"ok":true}));

    let first = harness
        .store()
        .begin_action("retry-worker")
        .unwrap()
        .unwrap();
    let failure = harness.registry.execute(&first);
    assert_eq!(failure.state, ActionState::Failed);
    assert_eq!(failure.code, "nonzero_exit_retryable");
    assert!(harness.store().finish_action(&first, &failure).unwrap());
    let db = Connection::open(&harness.db_path).unwrap();
    db.execute(
        "UPDATE completion_actions SET available_at='2000-01-01T00:00:00Z' WHERE id=?1",
        [first.id.as_str()],
    )
    .unwrap();
    drop(db);

    let second = harness
        .store()
        .begin_action("retry-worker")
        .unwrap()
        .unwrap();
    assert_ne!(first.lease_token, second.lease_token);
    let final_failure = harness.registry.execute(&second);
    assert!(harness
        .store()
        .finish_action(&second, &final_failure)
        .unwrap());
    let status = harness.store().action_status(batch).unwrap();
    assert_eq!(status["actions"][0]["state"], "failed");
    assert_eq!(status["actions"][0]["attempt_count"], 2);
    assert_eq!(
        harness
            .store()
            .get_task(task_id)
            .unwrap()
            .unwrap()
            .state
            .as_str(),
        "completed"
    );
}

#[test]
fn timeout_and_output_overflow_are_bounded_action_failures() {
    let harness = Harness::new("action_child_overflow", false, 1, 2_000, 128, false, false);
    let batch = harness.create_batch(None);
    harness.add_task(batch, "one");
    let task = harness.claim(batch, "A0");
    harness.complete(&task, json!({"ok":true}));
    let job = harness
        .store()
        .begin_action("bounded-worker")
        .unwrap()
        .unwrap();
    let overflow = harness.registry.execute(&job);
    assert_eq!(overflow.code, "output_limit");
    assert!(overflow.output_bytes > 128);
    assert!(harness.store().finish_action(&job, &overflow).unwrap());
    assert_eq!(
        harness.store().action_status(batch).unwrap()["actions"][0]["state"],
        "uncertain"
    );

    let timeout = Harness::new("action_child_hang", false, 1, 100, 32_768, false, false);
    let batch = timeout.create_batch(None);
    timeout.add_task(batch, "one");
    let task = timeout.claim(batch, "A0");
    timeout.complete(&task, json!({"ok":true}));
    let job = timeout
        .store()
        .begin_action("bounded-worker")
        .unwrap()
        .unwrap();
    let execution = timeout.registry.execute(&job);
    assert_eq!(execution.code, "timeout");
    assert_eq!(execution.state, ActionState::Uncertain);
}

#[test]
fn isolated_daemon_runs_background_completion_action_end_to_end() {
    let harness = Harness::new(
        "action_child_echo_stdin",
        true,
        1,
        2_000,
        32_768,
        true,
        false,
    );
    let daemon = common::Daemon::with_actions_config(harness.registry_path.clone());
    let batch = daemon.rpc(
        "batches/create",
        json!({"name":"isolated action daemon","config":{"action_profile":"canary","max_attempts":1}}),
    );
    let batch_id = batch["id"].as_str().unwrap();
    daemon.rpc(
        "batches/add_tasks",
        json!({"batch_id":batch_id,"tasks":[{"key":"one","name":"one"}]}),
    );
    let task = daemon.rpc(
        "scheduler/claim",
        json!({"agent_id":"A0","batch_id":batch_id,"lease_seconds":30}),
    )["task"]
        .clone();
    daemon.rpc(
        "scheduler/submit",
        json!({
            "id":task["id"],
            "agent_id":"A0",
            "lease_token":task["lease_token"],
            "result":{"accepted":true}
        }),
    );

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let status = daemon.rpc("actions/status", json!({"batch_id":batch_id}));
        if status["actions"][0]["state"] == "succeeded" {
            assert_eq!(status["actions"].as_array().unwrap().len(), 1);
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "action did not finish: {status}; log={:?}",
            daemon.root.join("daemon.log")
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let task = daemon.rpc("tasks/get", json!({"id":task["id"]}));
    assert_eq!(task["state"], "completed");
}

#[test]
fn native_child_is_contained_before_its_first_effect() {
    let harness = Harness::new(
        "action_child_requires_kernel_job_before_first_effect",
        true,
        1,
        2000,
        32768,
        false,
        false,
    );
    let batch = harness.create_batch(None);
    harness.add_task(batch, "one");
    let task = harness.claim(batch, "A0");
    harness.complete(&task, json!({"ok":true}));
    let job = harness
        .store()
        .begin_action("contained-worker")
        .unwrap()
        .unwrap();
    let outcome = harness.registry.execute(&job);
    assert_eq!(outcome.state, ActionState::Succeeded);
    assert_eq!(
        fs::read(harness.root.join("trusted/contained-first-effect.txt")).unwrap(),
        b"contained"
    );
}

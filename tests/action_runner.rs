mod common;
use coord::core::{
    actions::{ActionRegistry, ActionState},
    store::{CompleteOutcome, Store},
    types::{BatchConfig, TaskInput},
    windows_security::{file_sha256, process_identity},
};
use serde_json::{json, Value};
use std::{fs, path::PathBuf, sync::Arc};
use uuid::Uuid;

struct Fixture {
    root: PathBuf,
    registry: Arc<ActionRegistry>,
    path: PathBuf,
    db: PathBuf,
    store: Store,
    owner: String,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("coord-runner-{}", Uuid::new_v4()));
        for suffix in [
            "trusted/bin",
            "trusted/work",
            "business/refs/arknights/icons",
            "business/results/.receipts",
            "business/results/review_queue",
        ] {
            fs::create_dir_all(root.join(suffix)).unwrap();
        }
        fs::write(
            root.join("business/refs/arknights/icons/one.png"),
            b"test reference",
        )
        .unwrap();
        let executable = root.join("trusted/bin/coord-emote-action.exe");
        let publisher = std::env::var_os("COORD_TEST_PUBLISHER_BIN")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_coord-emote-action").into());
        fs::copy(publisher, &executable).unwrap();
        let key = root.join("broker.key");
        fs::write(&key, "17".repeat(32)).unwrap();
        let management_key = root.join("management.key");
        fs::write(&management_key, "29".repeat(32)).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let runner_sid = "S-1-5-21-111-222-333-9999";
        let command = json!({"executable":executable,"sha256":file_sha256(&executable).unwrap(),"working_directory":root.join("trusted/work"),
            "args":["--root",root.join("business")],"include_result":true,"timeout_ms":2000,"max_output_bytes":65536,"max_attempts":2,"idempotent":true});
        let registry_json = json!({"version":1,"allowed_root":root.join("trusted"),"external_runner":{"broker_addr":format!("127.0.0.1:{port}"),"key_file":key,"daemon_rpc_key_file":management_key,"runner_sid":runner_sid,"daemon_sid":process_identity().unwrap().sid},
            "profiles":{"emote":{"version":1,"on_task_completed":command,"on_batch_completed":command}}});
        let path = root.join("registry.json");
        fs::write(&path, serde_json::to_vec(&registry_json).unwrap()).unwrap();
        let registry = Arc::new(ActionRegistry::load(&path).unwrap());
        let db = root.join("state.db");
        let store = Store::open_with_actions(&db, registry.clone()).unwrap();
        Self {
            root,
            registry,
            path,
            db,
            store,
            owner: format!("runner:{runner_sid}"),
        }
    }
    fn add_completed(&self, group: &str) -> (Uuid, coord::core::types::Task) {
        let batch = self
            .store
            .create_batch(
                "isolated runner test",
                "",
                BatchConfig {
                    action_profile: Some("emote".into()),
                    ..BatchConfig::default()
                },
            )
            .unwrap();
        let batch = Uuid::parse_str(batch["id"].as_str().unwrap()).unwrap();
        self.store
            .add_tasks(
                batch,
                vec![TaskInput {
                    key: group.into(),
                    name: group.into(),
                    payload: json!({"group_id":group,"source_image_ids":["1"]}),
                }],
            )
            .unwrap();
        let task = self
            .store
            .claim_next("A0", Some(batch), Some(30))
            .unwrap()
            .unwrap();
        assert!(matches!(
            self.store
                .complete_task(
                    task.id,
                    "A0",
                    task.lease_token.as_deref().unwrap(),
                    result(group),
                    None
                )
                .unwrap(),
            CompleteOutcome::Completed { .. }
        ));
        (batch, task)
    }
    fn job(&self) -> coord::core::actions::ActionJob {
        let job = self.store.begin_action(&self.owner).unwrap().unwrap();
        self.store
            .runner_action(&job.id, &job.lease_token, &self.owner, true)
            .unwrap()
            .unwrap()
    }
}
fn result(group: &str) -> Value {
    json!({"schema_version":1,"group_id":group,"agent_id":"A0","attempt_no":1,"source_image_ids":["1"],"characters":[],"uncertain":[{
    "image_id":"1","visible_features":"Visible test character evidence","reference_candidates":[],"checked_paths":["refs/arknights/icons/one.png"],"check_conclusion":"Checked reference without confident match","reason":"Insufficient visible identity information","review_priority":"high"}],"general_notes":""})
}

#[test]
fn native_publisher_and_index_are_receipt_checked_atomic_and_idempotent() {
    let fixture = Fixture::new();
    let (batch, task) = fixture.add_completed("00");
    let job = fixture.job();
    assert_eq!(job.action_kind, "on_task_completed");
    let first = fixture.registry.execute(&job);
    assert_eq!(first.state, ActionState::Succeeded);
    let duplicate = fixture.registry.execute(&job);
    assert_eq!(duplicate.state, ActionState::Succeeded);
    assert!(fixture.store.finish_action(&job, &first).unwrap());
    assert!(!fixture.store.finish_action(&job, &duplicate).unwrap());
    let index = fixture.job();
    assert_eq!(index.action_kind, "on_batch_completed");
    let outcome = fixture.registry.execute(&index);
    assert_eq!(outcome.state, ActionState::Succeeded);
    assert!(fixture.store.finish_action(&index, &outcome).unwrap());
    let published: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("business/results/00.json")).unwrap())
            .unwrap();
    assert_eq!(published, result("00"));
    let index: Value = serde_json::from_slice(
        &fs::read(fixture.root.join("business/results/index.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(index["available_results"], 1);
    assert_eq!(
        fixture
            .store
            .get_task(task.id)
            .unwrap()
            .unwrap()
            .state
            .as_str(),
        "completed"
    );
    assert_eq!(
        fixture.store.action_status(batch).unwrap()["actions"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn broker_rechecks_current_receipt_attempt_and_action_fence() {
    let fixture = Fixture::new();
    let (_, task) = fixture.add_completed("00");
    let job = fixture.job();
    assert!(fixture
        .store
        .runner_action(&job.id, "wrong", &fixture.owner, true)
        .unwrap()
        .is_none());
    assert!(fixture
        .store
        .runner_action(&job.id, &job.lease_token, "other runner", true)
        .unwrap()
        .is_none());
    let db = rusqlite::Connection::open(&fixture.db).unwrap();
    db.execute(
        "DELETE FROM task_receipts WHERE task_id=?1",
        [task.id.to_string()],
    )
    .unwrap();
    assert!(fixture
        .store
        .runner_action(&job.id, &job.lease_token, &fixture.owner, true)
        .is_err());
    db.execute(
        "UPDATE completion_actions SET lease_until=datetime('now','+10 seconds') WHERE id=?1",
        [&job.id],
    )
    .unwrap();
    assert!(fixture
        .store
        .runner_action(&job.id, &job.lease_token, &fixture.owner, true)
        .unwrap()
        .is_none());
}

#[test]
fn malicious_result_paths_and_unknown_fields_never_publish() {
    let fixture = Fixture::new();
    fixture.add_completed("00");
    let mut job = fixture.job();
    job.payload["result"]["uncertain"][0]["checked_paths"] = json!(["refs/../../outside.png"]);
    assert!(coord::core::emote::execute(&fixture.root.join("business"), &job.payload).is_err());
    job.payload["result"] = result("00");
    job.payload["result"]["executable"] = json!("cmd.exe");
    assert!(coord::core::emote::execute(&fixture.root.join("business"), &job.payload).is_err());
    job.payload["result"] = result("00");
    job.payload["authorization"]["tasks"][0]["receipt_confirmed"] = json!(false);
    assert!(coord::core::emote::execute(&fixture.root.join("business"), &job.payload).is_err());
    assert!(!fixture.root.join("business/results/00.json").exists());
}

#[test]
fn hash_pin_is_rechecked_after_registry_load() {
    let fixture = Fixture::new();
    fixture.add_completed("00");
    let job = fixture.job();
    use std::io::Write;
    fs::OpenOptions::new()
        .append(true)
        .open(fixture.root.join("trusted/bin/coord-emote-action.exe"))
        .unwrap()
        .write_all(b"changed")
        .unwrap();
    let outcome = fixture.registry.execute(&job);
    assert_eq!(outcome.code, "executable_integrity_failed");
    assert!(ActionRegistry::load(&fixture.path).is_err());
}

#[test]
fn simultaneous_publications_never_replace_conflicting_results() {
    let fixture = Fixture::new();
    let target = Arc::new(fixture.root.join("business/results/concurrent.json"));
    let threads = (0..8)
        .map(|_| {
            let target = target.clone();
            std::thread::spawn(move || coord::core::emote::publish_new(&target, b"identical"))
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap().unwrap();
    }
    assert!(coord::core::emote::publish_new(&target, b"different").is_err());
    assert_eq!(fs::read(&*target).unwrap(), b"identical");
}

#[test]
fn junction_in_reference_ancestor_and_existing_output_are_rejected() {
    let fixture = Fixture::new();
    fixture.add_completed("00");
    let job = fixture.job();
    let original = fixture.root.join("business/refs/arknights");
    let moved = fixture.root.join("reference-target");
    fs::rename(&original, &moved).unwrap();
    let status = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(original.to_string_lossy().replace('/', "\\"))
        .arg(moved.to_string_lossy().replace('/', "\\"))
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "junction creation failed: {} {}",
        String::from_utf8_lossy(&status.stdout),
        String::from_utf8_lossy(&status.stderr)
    );
    assert!(coord::core::emote::execute(&fixture.root.join("business"), &job.payload).is_err());
    fs::remove_dir(&original).unwrap();
    fs::rename(&moved, &original).unwrap();
    fs::write(
        fixture.root.join("business/results/00.json"),
        b"previous attempt",
    )
    .unwrap();
    assert!(coord::core::emote::execute(&fixture.root.join("business"), &job.payload).is_err());
    assert_eq!(
        fs::read(fixture.root.join("business/results/00.json")).unwrap(),
        b"previous attempt"
    );
}

#[test]
fn external_daemon_only_exposes_authenticated_queue_broker_and_never_spawns() {
    let fixture = Fixture::new();
    let daemon = common::Daemon::with_actions_config(fixture.path.clone());
    let batch = daemon.rpc(
        "batches/create",
        json!({"name":"isolated external broker","config":{"action_profile":"emote"}}),
    );
    let batch = batch["id"].as_str().unwrap();
    daemon.rpc("batches/add_tasks",json!({"batch_id":batch,"tasks":[{"key":"00","name":"test","payload":{"group_id":"00","source_image_ids":["1"]}}]}));
    let task = daemon.rpc(
        "scheduler/claim",
        json!({"agent_id":"A0","batch_id":batch,"lease_seconds":30}),
    )["task"]
        .clone();
    daemon.rpc("scheduler/submit",json!({"id":task["id"],"agent_id":"A0","lease_token":task["lease_token"],"result":result("00")}));
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(!fixture.root.join("business/results/00.json").exists());
    let config = fixture.registry.external_runner.as_ref().unwrap();
    let base = format!("http://{}", config.broker_addr);
    let key = "17".repeat(32);
    assert_eq!(daemon.http.post(&daemon.url).json(&json!({"jsonrpc":"2.0","id":1,"method":"tasks/list","params":{"skip_reclaim":true}})).send().unwrap().status(),401);
    assert_eq!(daemon.http.post(&daemon.url).bearer_auth(&key).json(&json!({"jsonrpc":"2.0","id":1,"method":"tasks/list","params":{"skip_reclaim":true}})).send().unwrap().status(),401);
    assert_eq!(
        daemon
            .http
            .post(format!("{base}/claim"))
            .send()
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        daemon
            .http
            .post(&base)
            .bearer_auth(&key)
            .json(&json!({"method":"batches/create"}))
            .send()
            .unwrap()
            .status(),
        404
    );
    let job: coord::core::actions::ActionJob = daemon
        .http
        .post(format!("{base}/claim"))
        .bearer_auth(&key)
        .send()
        .unwrap()
        .json()
        .unwrap();
    let authorized: coord::core::actions::ActionJob = daemon
        .http
        .post(format!("{base}/authorize"))
        .bearer_auth(&key)
        .json(&json!({"id":job.id,"lease_token":job.lease_token}))
        .send()
        .unwrap()
        .json()
        .unwrap();
    // Protocol fixture only: current-user execution is not low-privilege evidence.
    let outcome = fixture.registry.execute(&authorized);
    assert_eq!(outcome.state, ActionState::Succeeded);
    let done:bool=daemon.http.post(format!("{base}/finish")).bearer_auth(&key).json(&json!({"fence":{"id":job.id,"lease_token":job.lease_token},"state":"succeeded","code":"exit_zero","exit_code":0,"output_bytes":outcome.output_bytes})).send().unwrap().json().unwrap();
    assert!(done);
}

#[test]
fn native_runner_rejects_the_daemon_identity_before_network_access() {
    let identity = process_identity().unwrap();
    assert!(
        coord::core::windows_security::require_runner_identity(&identity.sid, &identity.sid)
            .is_err()
    );
    let fixture = Fixture::new();
    let runner = std::env::var_os("COORD_TEST_RUNNER_BIN")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_coord-action-runner").into());
    let output = std::process::Command::new(runner)
        .arg("--registry")
        .arg(&fixture.path)
        .arg("--check-identity")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("distinct dedicated non-admin"));
}

#[test]
fn native_aggregator_includes_group_120_and_emits_only_review_candidates() {
    let fixture = Fixture::new();
    let batch = fixture
        .store
        .create_batch(
            "two native groups",
            "",
            BatchConfig {
                action_profile: Some("emote".into()),
                ..BatchConfig::default()
            },
        )
        .unwrap();
    let batch = Uuid::parse_str(batch["id"].as_str().unwrap()).unwrap();
    fixture
        .store
        .add_tasks(
            batch,
            ["00", "120"]
                .into_iter()
                .map(|group| TaskInput {
                    key: group.into(),
                    name: group.into(),
                    payload: json!({"group_id":group,"source_image_ids":["1"]}),
                })
                .collect(),
        )
        .unwrap();
    for _ in 0..2 {
        let task = fixture
            .store
            .claim_next("A0", Some(batch), Some(30))
            .unwrap()
            .unwrap();
        let mut value = result(task.payload["group_id"].as_str().unwrap());
        value["uncertain"] = json!([]);
        value["characters"] = json!([{"local_character_id":"local-one","image_ids":["1"],"representative_image_id":"1","visual_description":"Visible silver hair and a dark test coat.","game":"arknights","reference_match":{"match_status":"candidate","candidates":[{"game":"arknights","operator_name":"Test Operator","icon_path":"refs/arknights/icons/one.png"}],"checked_paths":["refs/arknights/icons/one.png"],"conclusion":"Test reference is a candidate requiring review."},"confidence":0.72,"notes":"Compare visual evidence before confirming identity."}]);
        fixture
            .store
            .complete_task(
                task.id,
                "A0",
                task.lease_token.as_deref().unwrap(),
                value,
                None,
            )
            .unwrap();
    }
    for _ in 0..3 {
        let job = fixture.job();
        let outcome = fixture.registry.execute(&job);
        assert_eq!(outcome.state, ActionState::Succeeded);
        assert!(fixture.store.finish_action(&job, &outcome).unwrap());
    }
    let index: Value = serde_json::from_slice(
        &fs::read(fixture.root.join("business/results/index.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(index["available_results"], 2);
    assert_eq!(index["results"][1]["group_id"], "120");
    let review: Value = serde_json::from_slice(
        &fs::read(
            fixture
                .root
                .join("business/results/review_queue/candidates.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(review["candidate_count"], 1);
    assert!(review["candidates"][0].get("global_character_id").is_none());
}

#[test]
fn broker_never_forwards_arbitrary_task_payload_or_secret_sentinels() {
    let fixture = Fixture::new();
    let (_, task) = fixture.add_completed("00");
    let db = rusqlite::Connection::open(&fixture.db).unwrap();
    let payload = json!({"group_id":"00","source_image_ids":["1"],"password":"g6-secret-sentinel","nested":{"api_key":"g6-secret-sentinel"},"executable":"cmd.exe"});
    db.execute(
        "UPDATE tasks SET payload=?1 WHERE id=?2",
        rusqlite::params![payload.to_string(), task.id.to_string()],
    )
    .unwrap();
    let job = fixture.job();
    let encoded = serde_json::to_string(&job.payload).unwrap();
    assert!(!encoded.contains("g6-secret-sentinel"));
    assert!(!encoded.contains("cmd.exe"));
    assert!(job.payload.get("task_context").is_none());
    assert_eq!(
        job.payload["authorization"]["tasks"][0]["input"],
        json!({"group_id":"00","source_image_ids":["1"]})
    );
}

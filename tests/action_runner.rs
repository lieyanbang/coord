mod common;
use base64::Engine as _;
use coord::core::{
    actions::{ActionRegistry, ActionState},
    store::{CompleteOutcome, Store},
    types::{BatchConfig, TaskInput},
    windows_security::{file_sha256, process_identity},
};
use ring::signature::KeyPair;
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
    config: coord::core::action_runner::RunnerConfig,
    key_pair: ring::signature::Ed25519KeyPair,
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
        let management_key = root.join("management.key");
        fs::write(&management_key, "29".repeat(32)).unwrap();
        let private_key_path = root.join("runner-private.pk8");
        let public_key_path = root.join("runner-public.key");
        let pkcs8 =
            ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
                .unwrap();
        let key_pair = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        fs::write(
            &private_key_path,
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(pkcs8.as_ref()),
        )
        .unwrap();
        fs::write(
            &public_key_path,
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key_pair.public_key().as_ref()),
        )
        .unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let runner_sid = "S-1-5-21-111-222-333-9999";
        let command = json!({"executable":executable,"sha256":file_sha256(&executable).unwrap(),"working_directory":root.join("trusted/work"),
            "args":["--root",root.join("business")],"include_result":true,"timeout_ms":2000,"max_output_bytes":65536,"max_attempts":2,"idempotent":true});
        let registry_json = json!({"version":1,"allowed_root":root.join("trusted"),"management_rpc_key_file":management_key,"external_runner":{"broker_addr":format!("127.0.0.1:{port}"),"runner_sid":runner_sid,"daemon_sid":process_identity().unwrap().sid,"runner_key_id":"test-runner-v1","runner_public_key_file":public_key_path},
            "profiles":{"emote":{"version":1,"on_task_completed":command,"on_batch_completed":command}}});
        let path = root.join("registry.json");
        fs::write(&path, serde_json::to_vec(&registry_json).unwrap()).unwrap();
        let registry = Arc::new(ActionRegistry::load(&path).unwrap());
        let config = registry.runner_config().unwrap().unwrap();
        let db = root.join("state.db");
        let store = Store::open_with_actions(&db, registry.clone()).unwrap();
        Self {
            root,
            registry,
            path,
            db,
            store,
            owner: format!("runner:{runner_sid}"),
            config,
            key_pair,
        }
    }
    fn add_completed(&self, group: &str) -> (Uuid, coord::core::types::Task) {
        self.add_completed_for(group, "A0")
    }
    fn add_completed_for(&self, group: &str, agent: &str) -> (Uuid, coord::core::types::Task) {
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
            .claim_next(agent, Some(batch), Some(30))
            .unwrap()
            .unwrap();
        let mut accepted = result(group);
        accepted["agent_id"] = agent.into();
        assert!(matches!(
            self.store
                .complete_task(
                    task.id,
                    agent,
                    task.lease_token.as_deref().unwrap(),
                    accepted,
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

    fn signed_headers(
        &self,
        method: &str,
        signed_path: &str,
        body: &[u8],
        timestamp_ms: i64,
        nonce: &[u8; 16],
    ) -> reqwest::header::HeaderMap {
        use ring::digest::{digest, SHA256};
        let nonce = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(nonce);
        let body_hash = digest(&SHA256, body)
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let timestamp = timestamp_ms.to_string();
        let canonical = format!(
            "coord-runner-request-v1\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            self.config.runner_key_id,
            self.config.runner_sid,
            method,
            signed_path,
            timestamp,
            nonce,
            body_hash,
        );
        let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(self.key_pair.sign(canonical.as_bytes()).as_ref());
        let mut headers = reqwest::header::HeaderMap::new();
        for (name, value) in [
            ("x-coord-protocol", "1".to_owned()),
            ("x-coord-key-id", self.config.runner_key_id.clone()),
            ("x-coord-runner-sid", self.config.runner_sid.clone()),
            ("x-coord-timestamp", timestamp),
            ("x-coord-nonce", nonce),
            ("x-coord-signature", signature),
        ] {
            headers.insert(
                name,
                reqwest::header::HeaderValue::from_str(&value).unwrap(),
            );
        }
        headers
    }

    fn signed_post(
        &self,
        daemon: &common::Daemon,
        path: &str,
        body: &Value,
        nonce: [u8; 16],
    ) -> reqwest::blocking::Response {
        let body = serde_json::to_vec(body).unwrap();
        let headers = self.signed_headers(
            "POST",
            path,
            &body,
            chrono::Utc::now().timestamp_millis(),
            &nonce,
        );
        daemon
            .http
            .post(format!("http://{}{}", self.config.broker_addr, path))
            .headers(headers)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
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
fn logical_worker_publication_keeps_exact_receipt_owner() {
    let fixture = Fixture::new();
    fixture.add_completed_for("00", "emote-ABCD");
    let job = fixture.job();
    let outcome = fixture.registry.execute(&job);
    assert_eq!(outcome.state, ActionState::Succeeded);
    assert!(fixture.store.finish_action(&job, &outcome).unwrap());
    let published: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("business/results/00.json")).unwrap())
            .unwrap();
    assert_eq!(published["agent_id"], "emote-ABCD");
    let receipt: Value = serde_json::from_slice(
        &fs::read(fixture.root.join("business/results/.receipts/00.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(receipt["agent_id"], "emote-ABCD");
    let index = fixture.job();
    let outcome = fixture.registry.execute(&index);
    assert_eq!(outcome.state, ActionState::Succeeded);
    assert!(fixture.store.finish_action(&index, &outcome).unwrap());
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
    let base = format!("http://{}", fixture.config.broker_addr);
    assert_eq!(daemon.http.post(&daemon.url).json(&json!({"jsonrpc":"2.0","id":1,"method":"tasks/list","params":{"skip_reclaim":true}})).send().unwrap().status(),401);
    assert_eq!(daemon.http.post(&daemon.url).bearer_auth("legacy-shared-broker-secret").json(&json!({"jsonrpc":"2.0","id":1,"method":"tasks/list","params":{"skip_reclaim":true}})).send().unwrap().status(),401);
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
            .post(format!("{base}/claim"))
            .bearer_auth("legacy-shared-broker-secret")
            .json(&json!({"method":"batches/create"}))
            .send()
            .unwrap()
            .status(),
        401
    );
    let job: coord::core::actions::ActionJob = fixture
        .signed_post(&daemon, "/claim", &json!({}), *Uuid::new_v4().as_bytes())
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    let fence = json!({"id":job.id,"lease_token":job.lease_token});
    let authorized: coord::core::actions::ActionJob = fixture
        .signed_post(&daemon, "/authorize", &fence, *Uuid::new_v4().as_bytes())
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    // Protocol fixture only: current-user execution is not low-privilege evidence.
    let outcome = fixture.registry.execute(&authorized);
    assert_eq!(outcome.state, ActionState::Succeeded);
    let done: bool = fixture
        .signed_post(
            &daemon,
            "/finish",
            &json!({"fence":{"id":job.id,"lease_token":job.lease_token},"state":"succeeded","code":"exit_zero","exit_code":0,"output_bytes":outcome.output_bytes}),
            *Uuid::new_v4().as_bytes(),
        )
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert!(done);
}

#[test]
fn signed_runner_auth_rejects_tampering_expiry_and_restart_replay() {
    let fixture = Fixture::new();
    let mut daemon = common::Daemon::with_actions_config(fixture.path.clone());
    let now = chrono::Utc::now().timestamp_millis();
    let body = serde_json::to_vec(&json!({})).unwrap();

    for (method, path, timestamp, nonce, sent_body, alter) in [
        (
            "POST",
            "/claim",
            now,
            *Uuid::new_v4().as_bytes(),
            body.clone(),
            "legacy",
        ),
        (
            "POST",
            "/authorize",
            now,
            *Uuid::new_v4().as_bytes(),
            body.clone(),
            "wrong_path",
        ),
        (
            "POST",
            "/claim",
            now,
            *Uuid::new_v4().as_bytes(),
            body.clone(),
            "wrong_body",
        ),
        (
            "POST",
            "/claim",
            now - 30_001,
            *Uuid::new_v4().as_bytes(),
            body.clone(),
            "expired",
        ),
        (
            "GET",
            "/claim",
            now,
            *Uuid::new_v4().as_bytes(),
            body.clone(),
            "wrong_method",
        ),
        (
            "POST",
            "/claim",
            now,
            *Uuid::new_v4().as_bytes(),
            body.clone(),
            "wrong_key_id",
        ),
        (
            "POST",
            "/claim",
            now,
            *Uuid::new_v4().as_bytes(),
            body.clone(),
            "wrong_runner",
        ),
    ] {
        let signed_path = if alter == "wrong_path" {
            "/claim"
        } else {
            path
        };
        let signed_method = if alter == "wrong_method" {
            "POST"
        } else {
            method
        };
        let signed_body = if alter == "wrong_body" {
            br#"{"changed":true}"#.as_slice()
        } else {
            sent_body.as_slice()
        };
        let mut headers =
            fixture.signed_headers(signed_method, signed_path, signed_body, timestamp, &nonce);
        let actual_body = if alter == "wrong_body" {
            br#"{"changed":false}"#.as_slice()
        } else {
            sent_body.as_slice()
        };
        match alter {
            "legacy" => {
                let response = daemon
                    .http
                    .post(format!("http://{}{}", fixture.config.broker_addr, path))
                    .bearer_auth("legacy-shared-broker-secret")
                    .body(sent_body.clone())
                    .send()
                    .unwrap();
                assert_eq!(response.status(), 401);
                continue;
            }
            "wrong_key_id" => {
                headers.insert("x-coord-key-id", "other-key-id".parse().unwrap());
            }
            "wrong_runner" => {
                headers.insert(
                    "x-coord-runner-sid",
                    "S-1-5-21-111-222-333-0000".parse().unwrap(),
                );
            }
            _ => {}
        }
        let response = daemon
            .http
            .request(
                method.parse::<reqwest::Method>().unwrap(),
                format!("http://{}{}", fixture.config.broker_addr, path),
            )
            .headers(headers)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(actual_body.to_vec())
            .send()
            .unwrap();
        assert_eq!(response.status(), 401, "negative case {alter}");
    }

    let rejected_health: Value = daemon
        .http
        .get(format!("{}/healthz", daemon.url))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(rejected_health["status"], "DEGRADED");
    assert_eq!(rejected_health["runner"]["status"], "SECURITY_ERROR");
    assert_eq!(
        rejected_health["runner"]["reason"],
        "signed_request_rejected"
    );

    let nonce = *Uuid::new_v4().as_bytes();
    let headers = fixture.signed_headers("POST", "/claim", &body, now, &nonce);
    let endpoint = format!("http://{}{}", fixture.config.broker_addr, "/claim");
    for attempt in 0..2 {
        let response = daemon
            .http
            .post(&endpoint)
            .headers(headers.clone())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.clone())
            .send()
            .unwrap();
        if attempt == 0 {
            assert_eq!(response.status(), 200);
            let job: Option<coord::core::actions::ActionJob> = response.json().unwrap();
            assert!(job.is_none());
        } else {
            assert_eq!(response.status(), 401);
        }
    }
    let accepted_health: Value = daemon
        .http
        .get(format!("{}/healthz", daemon.url))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(accepted_health["status"], "READY");
    assert_eq!(accepted_health["runner"]["status"], "READY");
    daemon.restart();
    let replay = daemon
        .http
        .post(&endpoint)
        .headers(headers.clone())
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body.clone())
        .send()
        .unwrap();
    assert_eq!(replay.status(), 401, "nonce replay survived Core restart");

    let other_fixture = Fixture::new();
    let other_daemon = common::Daemon::with_actions_config(other_fixture.path.clone());
    let cross_service = other_daemon
        .http
        .post(format!(
            "http://{}{}",
            other_fixture.config.broker_addr, "/claim"
        ))
        .headers(headers)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body.clone())
        .send()
        .unwrap();
    assert_eq!(
        cross_service.status(),
        401,
        "Runner key crossed service boundary"
    );
}

#[test]
fn runner_key_rotation_rejects_old_key_and_accepts_matching_new_key() {
    let mut fixture = Fixture::new();
    let mut daemon = common::Daemon::with_actions_config(fixture.path.clone());
    let body = serde_json::to_vec(&json!({})).unwrap();
    let old_nonce = *Uuid::new_v4().as_bytes();
    let old_headers = fixture.signed_headers(
        "POST",
        "/claim",
        &body,
        chrono::Utc::now().timestamp_millis(),
        &old_nonce,
    );
    let endpoint = format!("http://{}{}", fixture.config.broker_addr, "/claim");
    assert_eq!(
        daemon
            .http
            .post(&endpoint)
            .headers(old_headers.clone())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.clone())
            .send()
            .unwrap()
            .status(),
        200
    );

    let new_private = fixture.root.join("runner-v2-private.pk8");
    let new_public = fixture.root.join("runner-v2-public.key");
    let new_pkcs8 =
        ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
    let new_key_pair = ring::signature::Ed25519KeyPair::from_pkcs8(new_pkcs8.as_ref()).unwrap();
    fs::write(
        &new_private,
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(new_pkcs8.as_ref()),
    )
    .unwrap();
    fs::write(
        &new_public,
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(new_key_pair.public_key().as_ref()),
    )
    .unwrap();
    let mut registry: Value = serde_json::from_slice(&fs::read(&fixture.path).unwrap()).unwrap();
    registry["external_runner"]["runner_key_id"] = json!("test-runner-v2");
    registry["external_runner"]["runner_public_key_file"] =
        json!(new_public.to_string_lossy().to_string());
    fs::write(&fixture.path, serde_json::to_vec(&registry).unwrap()).unwrap();
    fixture.config.runner_key_id = "test-runner-v2".into();
    fixture.config.runner_public_key_file = new_public;
    fixture.key_pair = new_key_pair;
    daemon.restart();

    let old_key_after_rotation = daemon
        .http
        .post(&endpoint)
        .headers(old_headers)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body.clone())
        .send()
        .unwrap();
    assert_eq!(old_key_after_rotation.status(), 401);
    assert_eq!(
        fixture
            .signed_post(&daemon, "/claim", &json!({}), *Uuid::new_v4().as_bytes())
            .status(),
        200
    );
}

#[test]
fn broker_startup_and_port_failures_do_not_take_scheduler_down() {
    let fixture = Fixture::new();
    let blocker = std::net::TcpListener::bind(fixture.config.broker_addr).unwrap();
    let mut daemon = common::Daemon::with_actions_config(fixture.path.clone());
    let status: Value = daemon
        .http
        .get(format!("{}/healthz", daemon.url))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(status["status"], "DEGRADED");
    assert_eq!(status["core"]["status"], "READY");
    assert_eq!(status["database"]["status"], "READY");
    assert_eq!(status["broker"]["status"], "UNAVAILABLE");
    assert_eq!(status["runner"]["status"], "UNAVAILABLE");
    assert_eq!(
        daemon
            .rpc("tasks/list", json!({"limit":1,"skip_reclaim":true}))
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let db = fixture.db.to_string_lossy().to_string();
    let status_path = fixture.root.join("health.json");
    let status_path_arg = status_path.to_string_lossy().to_string();
    let health = daemon.cli(&["health", "--db", &db, "--status", &status_path_arg]);
    assert_eq!(health["status"], "DEGRADED");
    assert_eq!(health["components"]["core"]["status"], "READY");
    assert_eq!(health["components"]["database"]["status"], "READY");
    assert_eq!(health["components"]["broker"]["status"], "UNAVAILABLE");
    assert_eq!(health["components"]["runner"]["status"], "UNAVAILABLE");
    drop(blocker);
    daemon.restart();
    let recovered: Value = daemon
        .http
        .get(format!("{}/healthz", daemon.url))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(recovered["broker"]["status"], "READY");
    assert_eq!(recovered["runner"]["status"], "UNAVAILABLE");
}

#[test]
fn legacy_bearer_config_degrades_broker_without_blocking_core_or_reading_key() {
    let fixture = Fixture::new();
    let mut registry: Value = serde_json::from_slice(&fs::read(&fixture.path).unwrap()).unwrap();
    let broker_port = fixture.config.broker_addr.port();
    registry["external_runner"] = json!({
        "broker_addr": format!("127.0.0.1:{broker_port}"),
        "key_file": fixture.root.join("missing-old-broker.key"),
        "runner_sid": fixture.config.runner_sid,
        "daemon_sid": fixture.config.daemon_sid
    });
    let old_registry_path = fixture.root.join("legacy-registry.json");
    fs::write(&old_registry_path, serde_json::to_vec(&registry).unwrap()).unwrap();
    let daemon = common::Daemon::with_actions_config(old_registry_path);
    let status: Value = daemon
        .http
        .get(format!("{}/healthz", daemon.url))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(status["core"]["status"], "READY");
    assert_eq!(status["database"]["status"], "READY");
    assert_eq!(status["broker"]["status"], "SECURITY_ERROR");
    assert_eq!(status["status"], "DEGRADED");
    assert_eq!(status["runner"]["status"], "SECURITY_ERROR");
    assert!(!fixture.root.join("missing-old-broker.key").exists());
    let tasks: Value = daemon.rpc("tasks/list", json!({"limit":1,"skip_reclaim":true}));
    assert!(tasks.as_array().unwrap().is_empty());
}

#[test]
fn missing_public_key_is_a_broker_security_error_only() {
    let fixture = Fixture::new();
    let mut registry: Value = serde_json::from_slice(&fs::read(&fixture.path).unwrap()).unwrap();
    registry["external_runner"]["runner_public_key_file"] = fixture
        .root
        .join("missing-runner-public.key")
        .to_string_lossy()
        .into_owned()
        .into();
    let broken_registry = fixture.root.join("missing-public-key-registry.json");
    fs::write(&broken_registry, serde_json::to_vec(&registry).unwrap()).unwrap();
    let daemon = common::Daemon::with_actions_config(broken_registry);
    let status: Value = daemon
        .http
        .get(format!("{}/healthz", daemon.url))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(status["core"]["status"], "READY");
    assert_eq!(status["database"]["status"], "READY");
    assert_eq!(status["broker"]["status"], "SECURITY_ERROR");
    assert_eq!(status["status"], "DEGRADED");
    assert_eq!(status["runner"]["status"], "SECURITY_ERROR");
    assert!(daemon
        .rpc("tasks/list", json!({"limit":1,"skip_reclaim":true}))
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn malformed_runner_sid_is_a_component_security_error() {
    let fixture = Fixture::new();
    let mut registry: Value = serde_json::from_slice(&fs::read(&fixture.path).unwrap()).unwrap();
    registry["external_runner"]["runner_sid"] =
        json!("S-1-5-21-111-222-333-9999\nS-1-5-21-1-2-3-4");
    let bad_registry = fixture.root.join("malformed-sid-registry.json");
    fs::write(&bad_registry, serde_json::to_vec(&registry).unwrap()).unwrap();
    let daemon = common::Daemon::with_actions_config(bad_registry);
    let status: Value = daemon
        .http
        .get(format!("{}/healthz", daemon.url))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(status["core"]["status"], "READY");
    assert_eq!(status["database"]["status"], "READY");
    assert_eq!(status["broker"]["status"], "SECURITY_ERROR");
    assert_eq!(status["runner"]["status"], "SECURITY_ERROR");
    assert!(daemon
        .rpc("tasks/list", json!({"limit":1,"skip_reclaim":true}))
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn management_key_path_cannot_alias_the_legacy_broker_key() {
    let fixture = Fixture::new();
    let mut registry_json: Value =
        serde_json::from_slice(&fs::read(&fixture.path).unwrap()).unwrap();
    let management_path = registry_json["management_rpc_key_file"].as_str().unwrap();
    let legacy_alias = management_path.replace('\\', "/").to_ascii_uppercase();
    registry_json["external_runner"]["key_file"] = json!(legacy_alias);
    let registry_path = fixture.root.join("aliased-management-key-registry.json");
    fs::write(&registry_path, serde_json::to_vec(&registry_json).unwrap()).unwrap();
    let error = ActionRegistry::load(&registry_path).unwrap_err();
    assert!(error.to_string().contains("must not reuse"));
}

#[test]
fn legacy_management_path_is_optional_in_runner_config_and_remains_a_core_fallback() {
    let fixture = Fixture::new();
    let config = fixture.registry.runner_config().unwrap().unwrap();
    assert!(config.legacy_management_key_path.is_none());

    let mut registry_json: Value =
        serde_json::from_slice(&fs::read(&fixture.path).unwrap()).unwrap();
    let management_path = registry_json["management_rpc_key_file"].clone();
    registry_json
        .as_object_mut()
        .unwrap()
        .remove("management_rpc_key_file");
    registry_json["external_runner"]["daemon_rpc_key_file"] = management_path.clone();
    let legacy_path = fixture.root.join("legacy-management-registry.json");
    fs::write(&legacy_path, serde_json::to_vec(&registry_json).unwrap()).unwrap();

    let legacy_registry = ActionRegistry::load(&legacy_path).unwrap();
    let config = legacy_registry.runner_config().unwrap().unwrap();
    assert_eq!(
        config.legacy_management_key_path,
        Some(PathBuf::from(management_path.as_str().unwrap()))
    );
    assert_eq!(
        legacy_registry.management_key_path().unwrap(),
        Some(PathBuf::from(management_path.as_str().unwrap()))
    );
}

#[test]
fn missing_management_key_path_is_a_startup_error() {
    let fixture = Fixture::new();
    let mut registry_json: Value =
        serde_json::from_slice(&fs::read(&fixture.path).unwrap()).unwrap();
    registry_json
        .as_object_mut()
        .unwrap()
        .remove("management_rpc_key_file");
    let registry_path = fixture.root.join("missing-management-key-registry.json");
    fs::write(&registry_path, serde_json::to_vec(&registry_json).unwrap()).unwrap();
    let error = ActionRegistry::load(&registry_path).unwrap_err();
    assert!(error
        .to_string()
        .contains("management RPC key path required"));
}

#[test]
fn scheduler_compatibility_keeps_management_auth_with_broker_disabled() {
    let fixture = Fixture::new();
    let mut registry_json: Value =
        serde_json::from_slice(&fs::read(&fixture.path).unwrap()).unwrap();
    registry_json["external_runner"] = Value::Null;
    let registry_path = fixture.root.join("broker-disabled-registry.json");
    fs::write(&registry_path, serde_json::to_vec(&registry_json).unwrap()).unwrap();
    let daemon = common::Daemon::with_actions_config(registry_path);

    let health: Value = daemon
        .http
        .get(format!("{}/healthz", daemon.url))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(health["status"], "DEGRADED");
    assert_eq!(health["broker"]["status"], "UNAVAILABLE");
    assert_eq!(health["runner"]["status"], "UNAVAILABLE");

    let body = json!({"jsonrpc":"2.0","id":1,"method":"tasks/list","params":{"limit":1,"skip_reclaim":true}});
    let unauthenticated = daemon.http.post(&daemon.url).json(&body).send().unwrap();
    assert_eq!(unauthenticated.status(), 401);
    let authenticated = daemon
        .http
        .post(&daemon.url)
        .bearer_auth("29".repeat(32))
        .json(&body)
        .send()
        .unwrap();
    assert_eq!(authenticated.status(), 200);
}

#[test]
fn additive_nonce_migration_preserves_legacy_scheduler_rows() {
    let root = std::env::temp_dir().join(format!("coord-r5-nonce-migration-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let db_path = root.join("legacy.db");
    let store = Store::open(&db_path).unwrap();
    let batch = store
        .create_batch("legacy migration fixture", "", BatchConfig::default())
        .unwrap();
    let batch_id = Uuid::parse_str(batch["id"].as_str().unwrap()).unwrap();
    store
        .add_tasks(
            batch_id,
            vec![TaskInput {
                key: "preserved".into(),
                name: "legacy task".into(),
                payload: json!({"synthetic":true}),
            }],
        )
        .unwrap();
    drop(store);

    let legacy = rusqlite::Connection::open(&db_path).unwrap();
    let prior_task_count: i64 = legacy
        .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))
        .unwrap();
    legacy.execute_batch("DROP TABLE runner_nonces;").unwrap();
    drop(legacy);

    let migrated = Store::open(&db_path).unwrap();
    let check = rusqlite::Connection::open(&db_path).unwrap();
    let integrity: String = check
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    let task_count: i64 = check
        .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))
        .unwrap();
    let nonce_table: i64 = check
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='runner_nonces'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(integrity, "ok");
    assert_eq!(task_count, prior_task_count);
    assert_eq!(nonce_table, 1);
    drop(check);
    drop(migrated);
    fs::remove_dir_all(root).unwrap();
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

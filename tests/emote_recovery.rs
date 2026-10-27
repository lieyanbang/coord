use coord::core::{
    actions::{ActionRegistry, ActionState},
    emote_v2,
    store::Store,
    types::{BatchConfig, TaskInput},
    windows_security::{file_sha256, sha256},
};
use serde_json::{json, Value};
use std::{fs, path::PathBuf, sync::Arc};
use uuid::Uuid;

fn hash(v: &Value) -> String {
    sha256(&serde_json::to_vec(v).unwrap()).unwrap()
}
struct Recovery {
    root: PathBuf,
    db: PathBuf,
    registry_path: PathBuf,
    registry: Value,
    result: Value,
    supplement: Value,
    recovery: Value,
    event: Value,
    batch: Uuid,
    source: String,
}
impl Recovery {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("coord-review-{}", Uuid::new_v4()));
        for p in [
            "trusted/bin",
            "trusted/work",
            "business/task/00",
            "business/refs/arknights/icons",
            "business/results/.receipts",
            "business/results/.reviews",
            "business/results/review_queue",
            "supplements",
        ] {
            fs::create_dir_all(root.join(p)).unwrap();
        }
        fs::write(root.join("business/task/00/1.webp"), b"source fixture").unwrap();
        fs::write(
            root.join("business/refs/arknights/icons/one.png"),
            b"reference fixture",
        )
        .unwrap();
        let old = root.join("trusted/bin/v1.exe");
        let new = root.join("trusted/bin/v2.exe");
        fs::copy(env!("CARGO_BIN_EXE_coord-emote-action"), &old).unwrap();
        fs::copy(env!("CARGO_BIN_EXE_coord-emote-action-v2"), &new).unwrap();
        let command = |exe: &PathBuf, args: Value, attempts: u32, idempotent: bool| json!({"executable":exe,"working_directory":root.join("trusted/work"),"args":args,"sha256":file_sha256(exe).unwrap(),"include_result":true,"timeout_ms":2000,"max_output_bytes":65536,"max_attempts":attempts,"idempotent":idempotent});
        let v1 = command(&old, json!(["--root", root.join("business")]), 2, true);
        let v2 = command(
            &new,
            json!([
                "--root",
                root.join("business"),
                "--supplements",
                root.join("supplements")
            ]),
            1,
            false,
        );
        let mut registry = json!({"version":1,"allowed_root":root.join("trusted"),"isolated_in_process":true,"profiles":{"v1":{"version":1,"on_task_completed":v1},"v2":{"version":2,"submission_validation":"emote-v2","on_task_completed":v2}}});
        let registry_path = root.join("registry.json");
        fs::write(&registry_path, serde_json::to_vec(&registry).unwrap()).unwrap();
        let db = root.join("state.db");
        let store =
            Store::open_with_actions(&db, Arc::new(ActionRegistry::load(&registry_path).unwrap()))
                .unwrap();
        let batch = store
            .create_batch(
                "recovery fixture",
                "",
                BatchConfig {
                    action_profile: Some("v1".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        let batch = Uuid::parse_str(batch["id"].as_str().unwrap()).unwrap();
        store
            .add_tasks(
                batch,
                vec![TaskInput {
                    key: "00".into(),
                    name: "00".into(),
                    payload: json!({"group_id":"00","source_image_ids":["1"]}),
                }],
            )
            .unwrap();
        let task = store
            .claim_next("A0", Some(batch), Some(600))
            .unwrap()
            .unwrap();
        let result = json!({"schema_version":1,"group_id":"00","agent_id":"A0","attempt_no":1,"source_image_ids":["1"],"characters":[{"local_character_id":"c1","image_ids":["1"],"representative_image_id":"1","visual_description":"Visible features provide preliminary grouping evidence only","game":"unknown","reference_match":{"match_status":"no_match","candidates":[],"checked_paths":["refs/arknights/icons/one.png"],"conclusion":"Reference checked without enough matching identity features"},"confidence":0.3,"notes":"short"}],"uncertain":[],"general_notes":""});
        store
            .complete_task(
                task.id,
                "A0",
                task.lease_token.as_deref().unwrap(),
                result.clone(),
                None,
            )
            .unwrap();
        let mut original = store.begin_action("fixture").unwrap().unwrap();
        let outcome = ActionRegistry::load(&registry_path)
            .unwrap()
            .execute(&original);
        assert_eq!(outcome.state, ActionState::Failed);
        store.finish_action(&original, &outcome).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(130));
        original = store.begin_action("fixture").unwrap().unwrap();
        let outcome = ActionRegistry::load(&registry_path)
            .unwrap()
            .execute(&original);
        store.finish_action(&original, &outcome).unwrap();
        assert_eq!(
            store.action_status(batch).unwrap()["actions"][0]["state"],
            "failed"
        );
        let storage: String = rusqlite::Connection::open(&db)
            .unwrap()
            .query_row(
                "SELECT result FROM tasks WHERE id=?1",
                [task.id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        let supplement = json!({"schema_version":1,"supplement_revision":1,"supplement_id":Uuid::new_v4(),"batch_id":batch,"task_id":task.id,"original_action_id":original.id,"original_event_id":original.event_id,"accepted_result_sha256":hash(&result),"accepted_result_storage_sha256":sha256(storage.as_bytes()).unwrap(),"group_id":"00","review_status":"APPROVED","created_at":chrono::Utc::now(),"entries":[{"character_index":0,"local_character_id":"c1","character_sha256":hash(&result["characters"][0]),"image_ids":["1"],"review_status":"APPROVED","review_notes":"Actually checked source and reference; insufficient identifying features remain unresolved.","reviewer":"fixture-reviewer","reviewed_at":chrono::Utc::now(),"evidence":[{"path":"task/00/1.webp","sha256":file_sha256(&root.join("business/task/00/1.webp")).unwrap()},{"path":"refs/arknights/icons/one.png","sha256":file_sha256(&root.join("business/refs/arknights/icons/one.png")).unwrap()}]}]});
        let recovery = json!({"schema_version":1,"batch_id":batch,"task_id":task.id,"original_action_id":original.id,"original_event_id":original.event_id,"result_sha256":hash(&result),"result_storage_sha256":sha256(storage.as_bytes()).unwrap(),"supplement_sha256":hash(&supplement)});
        registry["profiles"]["v2"]["recovery"] = json!({"batch_id":batch,"source_profile":"v1","expires_at":chrono::Utc::now()+chrono::Duration::hours(1),"actions":{original.id.clone():{"result_sha256":recovery["result_sha256"],"result_storage_sha256":recovery["result_storage_sha256"],"supplement_sha256":hash(&supplement)}}});
        fs::write(&registry_path, serde_json::to_vec(&registry).unwrap()).unwrap();
        let proof = json!({"task_id":task.id,"batch_id":batch,"agent_id":"A0","attempt_no":1,"input":task.payload,"receipt_confirmed":true,"result_sha256":hash(&result),"result_storage_sha256":recovery["result_storage_sha256"]});
        let event_id = Uuid::new_v4();
        let event = json!({"event_id":event_id,"task_id":task.id,"batch_id":batch,"attempt_no":1,"action_kind":"on_task_completed","result":result,"recovery":recovery,"authorization":{"schema_version":1,"event_id":event_id,"tasks":[proof]}});
        let fixture = Self {
            root,
            db,
            registry_path,
            registry,
            result,
            supplement,
            recovery,
            event,
            batch,
            source: original.id,
        };
        fixture.write_supplement(&fixture.supplement);
        fixture
    }
    fn write_supplement(&self, s: &Value) {
        fs::write(
            self.root.join("supplements").join(format!(
                "{}.json",
                self.recovery["original_event_id"].as_str().unwrap()
            )),
            serde_json::to_vec(s).unwrap(),
        )
        .unwrap();
    }
    fn store(&self) -> Store {
        Store::open_with_actions(
            &self.db,
            Arc::new(ActionRegistry::load(&self.registry_path).unwrap()),
        )
        .unwrap()
    }
    fn run(&self, event: &Value) -> anyhow::Result<()> {
        emote_v2::execute(
            &self.root.join("business"),
            &self.root.join("supplements"),
            event,
        )
    }
}
impl Drop for Recovery {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn independent_recovery_preserves_source_and_receipt_and_replays_only_its_new_event() {
    let f = Recovery::new();
    let store = f.store();
    let before = store.action_status(f.batch).unwrap();
    assert!(store.recover_action(Uuid::new_v4(), "v2").is_err());
    let action = store
        .recover_action(Uuid::parse_str(&f.source).unwrap(), "v2")
        .unwrap();
    let replay = store
        .recover_action(Uuid::parse_str(&f.source).unwrap(), "v2")
        .unwrap();
    assert_eq!(action["id"], replay["id"]);
    assert_eq!(replay["replayed"], true);
    let job = store.begin_action("fixture").unwrap().unwrap();
    let job = store
        .runner_action(&job.id, &job.lease_token, "fixture", true)
        .unwrap()
        .unwrap();
    let outcome = ActionRegistry::load(&f.registry_path)
        .unwrap()
        .execute(&job);
    assert_eq!(outcome.state, ActionState::Succeeded);
    assert!(store.finish_action(&job, &outcome).unwrap());
    let after = store.action_status(f.batch).unwrap();
    assert_eq!(
        before["actions"][0],
        after["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == f.source)
            .unwrap()
            .clone()
    );
    let published: Value =
        serde_json::from_slice(&fs::read(f.root.join("business/results/00.json")).unwrap())
            .unwrap();
    assert_eq!(published, f.result);
    assert_eq!(published["characters"][0]["notes"], "short");
    assert!(f.run(&job.payload).is_ok());
    assert!(f.run(&f.event).is_err()); // same source with a different publication event cannot overwrite provenance
}

#[test]
fn review_binding_status_bounds_and_sources_are_checked_before_any_output() {
    let f = Recovery::new();
    for change in [
        "missing",
        "status",
        "short",
        "character",
        "images",
        "source",
        "binding",
        "encoding",
    ] {
        let mut s = f.supplement.clone();
        match change {
            "missing" => s["entries"] = json!([]),
            "status" => s["entries"][0]["review_status"] = "CONFLICT".into(),
            "short" => s["entries"][0]["review_notes"] = "still short".into(),
            "character" => s["entries"][0]["character_sha256"] = "a".repeat(64).into(),
            "images" => s["entries"][0]["image_ids"] = json!(["2"]),
            "source" => s["entries"][0]["evidence"][0]["sha256"] = "b".repeat(64).into(),
            "binding" => s["task_id"] = json!(Uuid::new_v4()),
            "encoding" => s["entries"][0]["review_notes"] = "?".repeat(50).into(),
            _ => unreachable!(),
        }
        f.write_supplement(&s);
        let mut event = f.event.clone();
        event["recovery"]["supplement_sha256"] = hash(&s).into();
        assert!(f.run(&event).is_err(), "{change}");
        assert!(!f.root.join("business/results/00.json").exists());
    }
    f.write_supplement(&f.supplement);
    let mut event = f.event.clone();
    event["recovery"]["supplement_sha256"] = "c".repeat(64).into();
    assert!(f.run(&event).is_err());
    let mut event = f.event.clone();
    event.as_object_mut().unwrap().remove("recovery");
    assert!(f.run(&event).is_err());
    fs::write(
        f.root.join("business/results/00.json"),
        serde_json::to_vec(&f.result).unwrap(),
    )
    .unwrap();
    assert!(f
        .run(&f.event)
        .unwrap_err()
        .to_string()
        .contains("uncertain"));
    assert!(!f.root.join("business/results/.receipts/00.json").exists());
}

#[test]
fn submission_preflight_is_opt_in_and_rejection_keeps_the_live_claim_correctable() {
    let f = Recovery::new();
    let store = f.store();
    let batch = store
        .create_batch(
            "future-v2",
            "",
            BatchConfig {
                action_profile: Some("v2".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let batch = Uuid::parse_str(batch["id"].as_str().unwrap()).unwrap();
    store
        .add_tasks(
            batch,
            vec![TaskInput {
                key: "00".into(),
                name: "00".into(),
                payload: json!({"group_id":"00","source_image_ids":["1"]}),
            }],
        )
        .unwrap();
    let task = store
        .claim_next("A0", Some(batch), Some(600))
        .unwrap()
        .unwrap();
    assert!(store
        .complete_task(
            task.id,
            "A0",
            task.lease_token.as_deref().unwrap(),
            f.result.clone(),
            None
        )
        .is_err());
    let unchanged = store.get_task(task.id).unwrap().unwrap();
    assert_eq!(unchanged.state.as_str(), "claimed");
    assert_eq!(unchanged.lease_token, task.lease_token);
    assert_eq!(unchanged.failure_count, 0);
    assert!(unchanged.result.is_none());
    let mut corrected = f.result.clone();
    corrected["characters"][0]["notes"] =
        "The inspected reference does not provide sufficient distinctive identity evidence.".into();
    store
        .complete_task(
            task.id,
            "A0",
            task.lease_token.as_deref().unwrap(),
            corrected.clone(),
            None,
        )
        .unwrap();
    assert_eq!(
        store.get_task(task.id).unwrap().unwrap().result,
        Some(corrected)
    );
}

#[test]
fn v2_index_accepts_legacy_receipts_and_review_overlays_without_mutating_results() {
    let f = Recovery::new();
    f.run(&f.event).unwrap();
    let mut legacy = f.event.clone();
    legacy.as_object_mut().unwrap().remove("recovery");
    let mut result = f.result.clone();
    result["group_id"] = "01".into();
    result["characters"][0]["notes"] =
        "Visible source compared with reference; no confident identity match.".into();
    let mut proof = legacy["authorization"]["tasks"][0].clone();
    proof["task_id"] = json!(Uuid::new_v4());
    proof["input"]["group_id"] = "01".into();
    proof["result_sha256"] = hash(&result).into();
    legacy["task_id"] = proof["task_id"].clone();
    legacy["result"] = result;
    legacy["authorization"]["tasks"] = json!([proof]);
    coord::core::emote::execute(&f.root.join("business"), &legacy).unwrap();
    let event_id = Uuid::new_v4();
    let batch = json!({"action_kind":"on_batch_completed","event_id":event_id,"batch_id":f.batch,"authorization":{"schema_version":1,"event_id":event_id,"tasks":[f.event["authorization"]["tasks"][0],proof]}});
    f.run(&batch).unwrap();
    f.run(&batch).unwrap();
    let index: Value =
        serde_json::from_slice(&fs::read(f.root.join("business/results/index.json")).unwrap())
            .unwrap();
    assert_eq!(index["available_results"], 2);
    let review_path = f.root.join("business/results/.reviews/00.json");
    let mut review: Value = serde_json::from_slice(&fs::read(&review_path).unwrap()).unwrap();
    review["supplement"]["review_status"] = "CONFLICT".into();
    fs::write(review_path, serde_json::to_vec(&review).unwrap()).unwrap();
    assert!(f.run(&batch).is_err());
}

#[test]
fn expired_or_changed_recovery_authority_cannot_publish() {
    let mut f = Recovery::new();
    let store = f.store();
    store
        .recover_action(Uuid::parse_str(&f.source).unwrap(), "v2")
        .unwrap();
    drop(store);
    f.registry["profiles"]["v2"]["recovery"]["expires_at"] =
        json!(chrono::Utc::now() - chrono::Duration::seconds(1));
    fs::write(&f.registry_path, serde_json::to_vec(&f.registry).unwrap()).unwrap();
    let store = f.store();
    assert!(store
        .recover_action(Uuid::parse_str(&f.source).unwrap(), "v2")
        .is_err());
    let job = store.begin_action("fixture").unwrap().unwrap();
    assert!(store
        .runner_action(&job.id, &job.lease_token, "fixture", true)
        .is_err());
    assert!(!f.root.join("business/results/00.json").exists());
}

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
            "business/results/.corrections",
            "business/results/review_queue",
            "supplements",
            "supplements/qa",
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

fn correction_fixture(f: &Recovery) -> (Value, Value, Value) {
    let mut effective = f.result.clone();
    effective["characters"][0]["notes"] =
        "Reference and source were independently checked; identity remains a candidate.".into();
    effective["characters"][0]["game"] = "arknights".into();
    effective["characters"][0]["reference_match"]["match_status"] = "candidate".into();
    effective["characters"][0]["reference_match"]["candidates"] = json!([{"game":"arknights","operator_name":"Fixture Identity","icon_path":"refs/arknights/icons/one.png","visual_notes":""}]);
    let correction_id = Uuid::new_v4();
    let qa = json!({"schema_version":1,"status":"APPROVED","reviewer":"independent-qa","correction_id":correction_id,"group_id":"00","accepted_result_sha256":hash(&f.result),"effective_result_sha256":hash(&effective)});
    let qa_bytes = serde_json::to_vec(&qa).unwrap();
    fs::write(f.root.join("supplements/qa/record.json"), &qa_bytes).unwrap();
    let record = json!({"schema_version":2,"status":"APPROVED","revision":1,"correction_id":correction_id,"author":"editor","group_id":"00","batch_id":f.recovery["batch_id"],"task_id":f.recovery["task_id"],"original_action_id":f.recovery["original_action_id"],"original_event_id":f.recovery["original_event_id"],"accepted_result_sha256":hash(&f.result),"accepted_result_storage_sha256":f.recovery["result_storage_sha256"],"changes":[{"field":"characters","before":f.result["characters"],"after":effective["characters"]}],"evidence":[{"path":"task/00/1.webp","sha256":file_sha256(&f.root.join("business/task/00/1.webp")).unwrap()},{"path":"refs/arknights/icons/one.png","sha256":file_sha256(&f.root.join("business/refs/arknights/icons/one.png")).unwrap()}],"qa":{"path":"qa/record.json","sha256":sha256(&qa_bytes).unwrap(),"reviewer":"independent-qa"}});
    let mut event = f.event.clone();
    event["recovery"]["supplement_sha256"] = hash(&record).into();
    f.write_supplement(&record);
    (record, event, effective)
}

#[test]
fn correction_publishes_effective_value_and_preserves_original_history() {
    let f = Recovery::new();
    let before = f.store().action_status(f.batch).unwrap();
    let (record, event, effective) = correction_fixture(&f);
    f.run(&event).unwrap();
    f.run(&event).unwrap();
    assert_eq!(before, f.store().action_status(f.batch).unwrap());
    assert_eq!(
        serde_json::from_slice::<Value>(
            &fs::read(f.root.join("business/results/00.json")).unwrap()
        )
        .unwrap(),
        f.result
    );
    let receipt = fs::read(f.root.join("business/results/.receipts/00.json")).unwrap();
    let event_id = Uuid::new_v4();
    let mut proof = event["authorization"]["tasks"][0].clone();
    proof["recoveries"] = json!([event["recovery"]]);
    let batch = json!({"action_kind":"on_batch_completed","event_id":event_id,"batch_id":f.batch,"authorization":{"schema_version":1,"event_id":event_id,"tasks":[proof]}});
    f.run(&batch).unwrap();
    let index: Value =
        serde_json::from_slice(&fs::read(f.root.join("business/results/index.json")).unwrap())
            .unwrap();
    assert_eq!(index["results"][0]["sha256"], hash(&f.result));
    assert_eq!(index["results"][0]["effective_sha256"], hash(&effective));
    assert_eq!(
        index["results"][0]["correction"]["correction_id"],
        record["correction_id"]
    );
    assert_eq!(
        index["results"][0]["correction"]["qa_sha256"],
        record["qa"]["sha256"]
    );
    assert_eq!(
        fs::read(f.root.join("business/results/.receipts/00.json")).unwrap(),
        receipt
    );
    fs::remove_file(f.root.join("business/results/.corrections/00.json")).unwrap();
    assert!(
        f.run(&batch).is_err(),
        "a missing authorized correction must not fall back"
    );
}

#[test]
fn correction_rejects_wrong_bindings_evidence_and_conflicting_values() {
    let f = Recovery::new();
    let (record, event, _) = correction_fixture(&f);
    for case in [
        "task",
        "event",
        "storage",
        "before",
        "missing_image",
        "missing_ref",
        "author",
        "qa_hash",
        "status",
    ] {
        let mut changed = record.clone();
        match case {
            "task" => changed["task_id"] = json!(Uuid::new_v4()),
            "event" => changed["original_event_id"] = json!(Uuid::new_v4()),
            "storage" => changed["accepted_result_storage_sha256"] = "0".repeat(64).into(),
            "before" => changed["changes"][0]["before"] = json!([]),
            "missing_image" => {
                changed["evidence"].as_array_mut().unwrap().remove(0);
            }
            "missing_ref" => {
                changed["evidence"].as_array_mut().unwrap().pop();
            }
            "author" => changed["author"] = "independent-qa".into(),
            "qa_hash" => changed["qa"]["sha256"] = "0".repeat(64).into(),
            "status" => changed["status"] = "CONFLICT".into(),
            _ => unreachable!(),
        }
        f.write_supplement(&changed);
        let mut changed_event = event.clone();
        changed_event["recovery"]["supplement_sha256"] = hash(&changed).into();
        assert!(f.run(&changed_event).is_err(), "{case}");
        assert!(!f.root.join("business/results/00.json").exists(), "{case}");
    }
    f.write_supplement(&record);
    fs::write(f.root.join("business/task/00/1.webp"), b"changed").unwrap();
    assert!(f.run(&event).is_err());
}

#[test]
fn correction_can_record_unchecked_reference_without_asserting_identity() {
    let f = Recovery::new();
    let (mut record, mut event, mut effective) = correction_fixture(&f);
    let character = &mut effective["characters"][0];
    character["game"] = "unknown".into();
    character["reference_match"] = json!({"match_status":"no_match","candidates":[],"checked_paths":[],"conclusion":"No reference was checked for this corrected image."});
    character["notes"] = "The source was examined; no reference identity has been verified.".into();
    record["changes"][0]["after"] = effective["characters"].clone();
    let qa_path = f.root.join("supplements/qa/record.json");
    let mut qa: Value = serde_json::from_slice(&fs::read(&qa_path).unwrap()).unwrap();
    qa["effective_result_sha256"] = hash(&effective).into();
    let bytes = serde_json::to_vec(&qa).unwrap();
    fs::write(&qa_path, &bytes).unwrap();
    record["qa"]["sha256"] = sha256(&bytes).unwrap().into();
    record["evidence"].as_array_mut().unwrap().pop();
    f.write_supplement(&record);
    event["recovery"]["supplement_sha256"] = hash(&record).into();
    f.run(&event).unwrap();

    let mut invalid = record.clone();
    invalid["changes"][0]["after"][0]["game"] = "arknights".into();
    f.write_supplement(&invalid);
    event["recovery"]["supplement_sha256"] = hash(&invalid).into();
    assert!(f
        .run(&event)
        .unwrap_err()
        .to_string()
        .contains("unchecked correction"));
}

#[test]
fn correction_concurrent_retry_conflict_and_partial_are_safe() {
    let f = Recovery::new();
    let (record, event, _) = correction_fixture(&f);
    let root = f.root.clone();
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let root = root.clone();
            let event = event.clone();
            std::thread::spawn(move || {
                emote_v2::execute(&root.join("business"), &root.join("supplements"), &event)
            })
        })
        .collect();
    let outcomes: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(outcomes.iter().any(Result::is_ok));
    f.run(&event).unwrap();
    let mut other = record.clone();
    other["revision"] = json!(2);
    f.write_supplement(&other);
    let mut other_event = event.clone();
    other_event["recovery"]["supplement_sha256"] = hash(&other).into();
    assert!(f.run(&other_event).is_err());
    f.write_supplement(&record);
    fs::write(
        f.root.join("business/results/.corrections/00.lock"),
        b"interrupted",
    )
    .unwrap();
    assert!(f.run(&event).is_err());
    let id = Uuid::new_v4();
    let batch = json!({"action_kind":"on_batch_completed","event_id":id,"batch_id":f.batch,"authorization":{"schema_version":1,"event_id":id,"tasks":[event["authorization"]["tasks"][0]]}});
    assert!(f.run(&batch).is_err());
}

#[test]
fn correction_preserves_a_preexisting_v2_receipt_and_indexes_corrected_candidates() {
    let f = Recovery::new();
    f.run(&f.event).unwrap();
    let receipt_path = f.root.join("business/results/.receipts/00.json");
    let review_path = f.root.join("business/results/.reviews/00.json");
    let receipt = fs::read(&receipt_path).unwrap();
    let review = fs::read(&review_path).unwrap();
    let (_, event, effective) = correction_fixture(&f);
    f.run(&event).unwrap();
    assert_eq!(fs::read(receipt_path).unwrap(), receipt);
    assert_eq!(fs::read(review_path).unwrap(), review);

    let mut other_result = effective.clone();
    other_result["group_id"] = "01".into();
    let mut other_proof = event["authorization"]["tasks"][0].clone();
    other_proof["task_id"] = json!(Uuid::new_v4());
    other_proof["input"]["group_id"] = "01".into();
    other_proof["result_sha256"] = hash(&other_result).into();
    let mut other_event = event.clone();
    other_event.as_object_mut().unwrap().remove("recovery");
    other_event["task_id"] = other_proof["task_id"].clone();
    other_event["result"] = other_result;
    other_event["authorization"]["tasks"] = json!([other_proof]);
    coord::core::emote::execute(&f.root.join("business"), &other_event).unwrap();
    let id = Uuid::new_v4();
    let mut proof = event["authorization"]["tasks"][0].clone();
    proof["recoveries"] = json!([event["recovery"]]);
    let batch = json!({"action_kind":"on_batch_completed","event_id":id,"batch_id":f.batch,"authorization":{"schema_version":1,"event_id":id,"tasks":[proof,other_proof]}});
    f.run(&batch).unwrap();
    let queue: Value = serde_json::from_slice(
        &fs::read(f.root.join("business/results/review_queue/candidates.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(queue["candidate_count"], 1);
    assert_eq!(
        queue["candidates"][0]["groups"][0]["local_characters"][0]["operator_name"],
        "Fixture Identity"
    );
}

#[test]
fn correction_can_regroup_images_and_keep_unobservable_fields_uncertain() {
    let mut f = Recovery::new();
    fs::write(f.root.join("business/task/00/2.webp"), b"second image").unwrap();
    f.result["source_image_ids"] = json!(["1", "2"]);
    let mut second = f.result["characters"][0].clone();
    second["local_character_id"] = "c2".into();
    second["image_ids"] = json!(["2"]);
    second["representative_image_id"] = "2".into();
    f.result["characters"].as_array_mut().unwrap().push(second);
    f.recovery["result_sha256"] = hash(&f.result).into();
    f.event["result"] = f.result.clone();
    f.event["recovery"] = f.recovery.clone();
    f.event["authorization"]["tasks"][0]["result_sha256"] = hash(&f.result).into();
    f.event["authorization"]["tasks"][0]["input"]["source_image_ids"] = json!(["1", "2"]);
    let (mut record, mut event, mut effective) = correction_fixture(&f);
    effective["characters"][0]["image_ids"] = json!(["1", "2"]);
    effective["characters"].as_array_mut().unwrap().pop();
    effective["uncertain"] = json!([{"image_id":"2","visible_features":"The subject is visible but the eye is closed.","reference_candidates":[],"checked_paths":["refs/arknights/icons/one.png"],"check_conclusion":"Reference cannot establish the hidden eye colour.","reason":"The relevant feature is obscured in the source.","review_priority":"medium"}]);
    record["changes"][0]["after"] = effective["characters"].clone();
    record["changes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"field":"uncertain","before":[],"after":effective["uncertain"]}));
    record["evidence"].as_array_mut().unwrap().push(json!({"path":"task/00/2.webp","sha256":file_sha256(&f.root.join("business/task/00/2.webp")).unwrap()}));
    let qa_path = f.root.join("supplements/qa/record.json");
    let mut qa: Value = serde_json::from_slice(&fs::read(&qa_path).unwrap()).unwrap();
    qa["effective_result_sha256"] = hash(&effective).into();
    let bytes = serde_json::to_vec(&qa).unwrap();
    fs::write(qa_path, &bytes).unwrap();
    record["qa"]["sha256"] = sha256(&bytes).unwrap().into();
    event["recovery"]["supplement_sha256"] = hash(&record).into();
    f.write_supplement(&record);
    f.run(&event).unwrap();
    let id = Uuid::new_v4();
    let mut proof = event["authorization"]["tasks"][0].clone();
    proof["recoveries"] = json!([event["recovery"]]);
    let batch = json!({"action_kind":"on_batch_completed","event_id":id,"batch_id":f.batch,"authorization":{"schema_version":1,"event_id":id,"tasks":[proof]}});
    f.run(&batch).unwrap();
    let index: Value =
        serde_json::from_slice(&fs::read(f.root.join("business/results/index.json")).unwrap())
            .unwrap();
    assert_eq!(index["results"][0]["character_count"], 1);
    assert_eq!(index["results"][0]["uncertain_count"], 1);
    assert_eq!(index["results"][0]["effective_sha256"], hash(&effective));
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
    // Business publication completion does not assert independent full-batch QA.
    assert_eq!(index["state"], "STRUCTURALLY_COMPLETE");
    assert!(index["scope"].as_str().unwrap().contains("preliminary"));
    let mut effective = f.result.clone();
    effective["characters"][0]["notes"] = f.supplement["entries"][0]["review_notes"].clone();
    assert_eq!(index["results"][0]["sha256"], hash(&f.result));
    assert_eq!(index["results"][0]["effective_sha256"], hash(&effective));
    assert!(index["results"][1].get("effective_sha256").is_none());
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

#[test]
fn trusted_emote_workers_keep_the_result_identity_through_resume_release_and_publish() {
    let f = Recovery::new();
    let store = f.store();
    let batch = store
        .create_batch(
            "trusted-emote",
            "",
            BatchConfig {
                action_profile: Some("v2".into()),
                require_progress: true,
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
    let first = store
        .claim_trusted(batch, "0000", Some(600))
        .unwrap()
        .unwrap();
    assert_eq!(first.claimed_by.as_deref(), Some("emote-0000"));
    assert_eq!(
        store.resume_trusted(batch, "0000").unwrap()["task"]["id"],
        first.id.to_string()
    );
    assert!(store
        .progress_trusted(
            first.id,
            batch,
            "0001",
            first.attempt_no,
            "Wrong worker",
            Some(600)
        )
        .unwrap()
        .is_none());
    store
        .progress_trusted(
            first.id,
            batch,
            "0000",
            first.attempt_no,
            "Inspected fixture source and reference",
            Some(600),
        )
        .unwrap()
        .unwrap();
    store
        .release_trusted(
            first.id,
            batch,
            "0000",
            first.attempt_no,
            Some("Reassign isolated fixture"),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        store.resume_trusted(batch, "0000").unwrap()["state"],
        "released"
    );
    std::thread::sleep(std::time::Duration::from_millis(130));
    let task = store
        .claim_trusted(batch, "0001", Some(600))
        .unwrap()
        .unwrap();
    assert_eq!(task.claimed_by.as_deref(), Some("emote-0001"));
    assert!(task.attempt_no > first.attempt_no);
    let mut result = f.result.clone();
    result["agent_id"] = "emote-0001".into();
    result["attempt_no"] = task.attempt_no.into();
    store
        .progress_trusted(
            task.id,
            batch,
            "0001",
            task.attempt_no,
            "Source/reference evidence checked",
            Some(600),
        )
        .unwrap()
        .unwrap();
    assert!(store
        .complete_trusted(
            task.id,
            batch,
            "0001",
            task.attempt_no,
            result.clone(),
            None
        )
        .is_err());
    assert_eq!(
        store.get_task(task.id).unwrap().unwrap().state.as_str(),
        "claimed"
    );
    result["characters"][0]["notes"] =
        "Inspected source and reference; identifying features remain insufficient.".into();
    let accepted = store
        .complete_trusted(
            task.id,
            batch,
            "0001",
            task.attempt_no,
            result.clone(),
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(accepted["state"], "completed");
    assert_eq!(
        store
            .complete_trusted(
                task.id,
                batch,
                "0001",
                task.attempt_no,
                result.clone(),
                None
            )
            .unwrap()
            .unwrap()["replayed"],
        true
    );
    assert_eq!(
        store.resume_trusted(batch, "0001").unwrap()["state"],
        "completed"
    );
    let pending = store.begin_action("fixture").unwrap().unwrap();
    let job = store
        .runner_action(&pending.id, &pending.lease_token, "fixture", true)
        .unwrap()
        .unwrap();
    let execution = ActionRegistry::load(&f.registry_path)
        .unwrap()
        .execute(&job);
    assert_eq!(execution.state, ActionState::Succeeded);
    store.finish_action(&job, &execution).unwrap();
    let published: Value =
        serde_json::from_slice(&fs::read(f.root.join("business/results/00.json")).unwrap())
            .unwrap();
    assert_eq!(published, result);
    // A batch without the emote preflight keeps its historical raw worker label.
    let generic = store
        .create_batch("generic", "", BatchConfig::default())
        .unwrap();
    let generic = Uuid::parse_str(generic["id"].as_str().unwrap()).unwrap();
    store
        .add_tasks(
            generic,
            vec![TaskInput {
                key: "text".into(),
                name: "text".into(),
                payload: json!({}),
            }],
        )
        .unwrap();
    let text = store
        .claim_trusted(generic, "0000", Some(600))
        .unwrap()
        .unwrap();
    assert_eq!(text.claimed_by.as_deref(), Some("0000"));
}

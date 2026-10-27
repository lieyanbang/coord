//! Native Phase G publisher. Inputs are broker-authorized database results,
//! never paths or executable arguments supplied by an Agent.
use super::windows_security::{safe_existing_path, sha256};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::Path,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Candidate {
    game: String,
    operator_name: String,
    icon_path: String,
    #[serde(default)]
    visual_notes: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceMatch {
    match_status: String,
    candidates: Vec<Candidate>,
    checked_paths: Vec<String>,
    conclusion: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Character {
    local_character_id: String,
    image_ids: Vec<String>,
    representative_image_id: String,
    visual_description: String,
    game: String,
    reference_match: ReferenceMatch,
    confidence: f64,
    notes: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Uncertain {
    image_id: String,
    visible_features: String,
    reference_candidates: Vec<Candidate>,
    checked_paths: Vec<String>,
    check_conclusion: String,
    reason: String,
    review_priority: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmoteResult {
    schema_version: u32,
    group_id: String,
    agent_id: String,
    attempt_no: u64,
    source_image_ids: Vec<String>,
    characters: Vec<Character>,
    uncertain: Vec<Uncertain>,
    general_notes: String,
}

fn text(value: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&value.trim().chars().count()) && value.chars().count() <= max
}
fn unique(values: &[String]) -> bool {
    values.iter().collect::<BTreeSet<_>>().len() == values.len()
}
fn numeric(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}
pub(super) fn group(value: &str) -> bool {
    (value.len() == 2 || value.len() == 3)
        && numeric(value)
        && value.parse::<u32>().is_ok_and(|number| {
            number <= 120
                && if number < 100 {
                    value.len() == 2
                } else {
                    value.len() == 3
                }
        })
}
fn ref_file(root: Option<&Path>, value: &str) -> Result<()> {
    if value.len() > 512
        || !value.starts_with("refs/")
        || value.contains(['\\', ':'])
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        bail!("unsafe reference path");
    }
    let Some(root) = root else {
        return Ok(());
    };
    let refs = safe_existing_path(&root.join("refs"))?;
    let path = safe_existing_path(&root.join(value))?;
    if !path.starts_with(refs) || !path.is_file() {
        bail!("reference is not a contained regular file");
    }
    Ok(())
}
fn candidates(root: Option<&Path>, values: &[Candidate]) -> Result<()> {
    if values.len() > 20 {
        bail!("too many reference candidates");
    }
    for value in values {
        let prefix = format!("refs/{}/icons/", value.game);
        if !["arknights", "endfield"].contains(&value.game.as_str())
            || !text(&value.operator_name, 1, 128)
            || value.visual_notes.chars().count() > 1000
            || !value.icon_path.starts_with(&prefix)
            || value.icon_path[prefix.len()..].contains('/')
        {
            bail!("invalid reference candidate");
        }
        ref_file(root, &value.icon_path)?;
    }
    Ok(())
}
fn paths(root: Option<&Path>, values: &[String]) -> Result<()> {
    if values.is_empty() || !unique(values) {
        bail!("checked paths must be nonempty and unique");
    }
    for path in values {
        ref_file(root, path)?;
    }
    Ok(())
}

fn valid_agent_id(id: &str) -> bool {
    ["A0", "A1", "A2", "A3", "A4", "A5"].contains(&id)
        || id.strip_prefix("emote-").is_some_and(|worker| {
            worker.len() == 4
                && worker
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte))
        })
}

/// Same required fields, coverage, evidence and bounds as G4's Python validator;
/// additionally enforces the schema's candidate icon shape and all reparse ancestors.
pub fn validate_result(root: &Path, value: &Value, proof: &Value) -> Result<()> {
    validate_inner(Some(root), value, proof)
}

/// Submission preflight shares every semantic rule with the publisher. Core
/// does not require filesystem access to the private images/reference library.
pub fn validate_semantics(value: &Value, proof: &Value) -> Result<()> {
    validate_inner(None, value, proof)
}

fn validate_inner(root: Option<&Path>, value: &Value, proof: &Value) -> Result<()> {
    let result: EmoteResult =
        serde_json::from_value(value.clone()).context("strict emote schema")?;
    let input = &proof["input"];
    if result.schema_version != 1
        || !group(&result.group_id)
        || value["group_id"] != input["group_id"]
        || value["source_image_ids"] != input["source_image_ids"]
        || !valid_agent_id(&result.agent_id)
        || value["agent_id"] != proof["agent_id"]
        || result.attempt_no == 0
        || result.attempt_no > 1_000_000_000
        || value["attempt_no"] != proof["attempt_no"]
        || result.source_image_ids.is_empty()
        || result.source_image_ids.len() > 10
        || !unique(&result.source_image_ids)
        || !result.source_image_ids.iter().all(|id| numeric(id))
        || result.characters.len() > 20
        || result.uncertain.len() > 10
        || result.general_notes.chars().count() > 4000
    {
        bail!("result does not match current task context");
    }
    let allowed = result
        .source_image_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut covered = BTreeSet::new();
    let mut local_ids = BTreeSet::new();
    for character in &result.characters {
        let id = &character.local_character_id;
        if id.is_empty()
            || id.len() > 64
            || !id.as_bytes()[0].is_ascii_alphanumeric()
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
            || !local_ids.insert(id)
            || character.image_ids.is_empty()
            || character.image_ids.len() > 10
            || !unique(&character.image_ids)
            || !character.image_ids.iter().all(|id| allowed.contains(id))
            || !character
                .image_ids
                .contains(&character.representative_image_id)
            || !text(&character.visual_description, 20, 4000)
            || !["arknights", "endfield", "unknown"].contains(&character.game.as_str())
            || !(0.0..=1.0).contains(&character.confidence)
            || character.notes.chars().count() > 2000
            || (character.confidence < 0.6 && !text(&character.notes, 20, 2000))
        {
            bail!("invalid character or incomplete review evidence");
        }
        covered.extend(character.image_ids.iter().cloned());
        let reference = &character.reference_match;
        if !["matched", "candidate", "no_match"].contains(&reference.match_status.as_str())
            || !text(&reference.conclusion, 10, 2000)
        {
            bail!("invalid checked reference conclusion");
        }
        candidates(root, &reference.candidates)?;
        paths(root, &reference.checked_paths)?;
    }
    let mut uncertain_ids = BTreeSet::new();
    for item in &result.uncertain {
        if !allowed.contains(&item.image_id)
            || !uncertain_ids.insert(&item.image_id)
            || !text(&item.visible_features, 10, 2000)
            || !text(&item.check_conclusion, 10, 2000)
            || !text(&item.reason, 10, 2000)
            || !["low", "medium", "high", "urgent"].contains(&item.review_priority.as_str())
        {
            bail!("invalid uncertain evidence");
        }
        candidates(root, &item.reference_candidates)?;
        paths(root, &item.checked_paths)?;
        covered.insert(item.image_id.clone());
    }
    if covered != allowed {
        bail!("result image coverage mismatch");
    }
    Ok(())
}

pub(super) fn read_json(path: &Path) -> Result<Value> {
    let safe = safe_existing_path(path)?;
    if !safe.is_file() || fs::metadata(&safe)?.len() > 4 * 1024 * 1024 {
        bail!("invalid bounded JSON input");
    }
    Ok(serde_json::from_slice(&fs::read(safe)?)?)
}
/// Atomic no-replace publication. Identical retries succeed; conflicting data
/// never overwrites a prior attempt. Temp and target are on the same volume.
pub fn publish_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = safe_existing_path(path.parent().context("output parent")?)?;
    let filename = path.file_name().context("output name")?;
    let target = parent.join(filename);
    if fs::symlink_metadata(&target).is_ok() {
        let existing = safe_existing_path(&target)?;
        if existing.is_file() && fs::read(existing)? == bytes {
            return Ok(());
        }
        bail!("existing publication conflicts with current result");
    }
    let temporary = parent.join(format!(".coord-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        match fs::hard_link(&temporary, &target) {
            Ok(()) => Ok(()),
            Err(error) if target.exists() => {
                let existing = safe_existing_path(&target)?;
                if existing.is_file() && fs::read(existing)? == bytes {
                    Ok(())
                } else {
                    Err(error.into())
                }
            }
            Err(error) => Err(error.into()),
        }
    })();
    let _ = fs::remove_file(temporary);
    result
}
pub(super) fn proof_valid(event: &Value, proof: &Value) -> Result<()> {
    if proof["receipt_confirmed"] != true
        || proof["batch_id"] != event["batch_id"]
        || !proof["task_id"].is_string()
        || event["authorization"]["event_id"] != event["event_id"]
        || event["authorization"]["schema_version"] != 1
    {
        bail!("missing broker receipt authorization");
    }
    uuid::Uuid::parse_str(event["event_id"].as_str().context("event ID")?)?;
    uuid::Uuid::parse_str(proof["task_id"].as_str().context("task ID")?)?;
    Ok(())
}
pub fn execute(root: &Path, event: &Value) -> Result<()> {
    execute_using(root, event, validate_result, |outputs| {
        for (path, bytes) in outputs {
            publish_new(path, bytes)?;
        }
        Ok(())
    })
}

pub(super) fn execute_using(
    root: &Path,
    event: &Value,
    validate: impl Fn(&Path, &Value, &Value) -> Result<()>,
    publish: impl Fn(&[(std::path::PathBuf, Vec<u8>)]) -> Result<()>,
) -> Result<()> {
    let root = safe_existing_path(root)?;
    let results = root.join("results");
    safe_existing_path(&results)?;
    let proofs = event["authorization"]["tasks"]
        .as_array()
        .context("broker task proofs missing")?;
    if proofs.is_empty() || proofs.len() > 10_000 {
        bail!("invalid authorized task set");
    }
    match event["action_kind"].as_str() {
        Some("on_task_completed") => {
            if proofs.len() != 1 {
                bail!("task action requires one receipt");
            }
            let proof = &proofs[0];
            proof_valid(event, proof)?;
            if event["task_id"] != proof["task_id"] || event["attempt_no"] != proof["attempt_no"] {
                bail!("task event fence mismatch");
            }
            let result = &event["result"];
            validate(&root, result, proof)?;
            let bytes = serde_json::to_vec(result)?;
            let digest = sha256(&bytes)?;
            if proof["result_sha256"] != digest {
                bail!("result does not match submit receipt digest");
            }
            let group = result["group_id"].as_str().context("group")?;
            let receipt = json!({"schema_version":1,"task_id":proof["task_id"],"batch_id":proof["batch_id"],"agent_id":proof["agent_id"],"attempt_no":proof["attempt_no"],"result_sha256":digest});
            // Receipt and result are individually atomic. Idempotent replay
            // repairs an interrupted pair only when the bytes match exactly.
            publish(&[
                (results.join(format!("{group}.json")), bytes),
                (
                    results.join(".receipts").join(format!("{group}.json")),
                    serde_json::to_vec(&receipt)?,
                ),
            ])?;
        }
        Some("on_batch_completed") => {
            let mut entries = Vec::new();
            let mut seen = BTreeSet::new();
            let mut references: BTreeMap<(String, String), BTreeMap<String, Vec<Value>>> =
                BTreeMap::new();
            for proof in proofs {
                proof_valid(event, proof)?;
                let group = proof["input"]["group_id"].as_str().context("group")?;
                if !self::group(group) || !seen.insert(group) {
                    bail!("invalid/duplicate authorized group");
                }
                let result = read_json(&results.join(format!("{group}.json")))?;
                validate(&root, &result, proof)?;
                let digest = sha256(&serde_json::to_vec(&result)?)?;
                let receipt = read_json(&results.join(".receipts").join(format!("{group}.json")))?;
                for key in [
                    "task_id",
                    "batch_id",
                    "agent_id",
                    "attempt_no",
                    "result_sha256",
                ] {
                    if receipt[key] != proof[key] {
                        bail!("published receipt does not match current completed attempt");
                    }
                }
                if proof["result_sha256"] != digest {
                    bail!("published result digest mismatch");
                }
                entries.push(json!({"group_id":group,"path":format!("{group}.json"),"sha256":digest,"character_count":result["characters"].as_array().unwrap().len(),"uncertain_count":result["uncertain"].as_array().unwrap().len()}));
                for character in result["characters"].as_array().unwrap() {
                    for candidate in character["reference_match"]["candidates"]
                        .as_array()
                        .unwrap()
                    {
                        let key = (
                            candidate["game"].as_str().unwrap().to_string(),
                            candidate["operator_name"]
                                .as_str()
                                .unwrap()
                                .trim()
                                .to_lowercase(),
                        );
                        references.entry(key).or_default().entry(group.to_string()).or_default().push(json!({"local_character_id":character["local_character_id"],"image_ids":character["image_ids"],"visual_description":character["visual_description"],"confidence":character["confidence"],"operator_name":candidate["operator_name"],"icon_path":candidate["icon_path"]}));
                    }
                }
            }
            entries
                .sort_by_key(|entry| entry["group_id"].as_str().unwrap().parse::<u32>().unwrap());
            let mut candidates = Vec::new();
            for ((game, name), groups) in references {
                if groups.len() < 2 {
                    continue;
                }
                let names = groups
                    .values()
                    .flatten()
                    .filter_map(|entry| entry["operator_name"].as_str())
                    .collect::<BTreeSet<_>>();
                candidates.push(json!({"candidate_key":format!("shared-reference:{game}:{name}"),"game":game,"reference_name_samples":names,"groups":groups.iter().map(|(group,entries)|json!({"group_id":group,"local_characters":entries})).collect::<Vec<_>>(),"review_priority":"medium","reason":"Compare visual evidence before any merge."}));
            }
            let index = json!({"schema_version":1,"state":"STRUCTURALLY_COMPLETE","scope":"Receipt-checked preliminary index; no global character merge.","expected_groups":proofs.len(),"available_results":entries.len(),"missing_groups":[],"results":entries});
            let review = json!({"schema_version":1,"state":"candidate_review","candidate_count":candidates.len(),"candidates":candidates,"missing_groups":[],"note":"Candidates only; do not assign global IDs or merge automatically."});
            publish(&[
                (results.join("index.json"), serde_json::to_vec(&index)?),
                (
                    results.join("review_queue/candidates.json"),
                    serde_json::to_vec(&review)?,
                ),
            ])?;
        }
        _ => bail!("unsupported completion action"),
    }
    Ok(())
}

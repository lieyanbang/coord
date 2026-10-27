//! Versioned review overlay. Accepted results and v1 publications stay immutable.
use super::{
    emote,
    windows_security::{safe_existing_path, sha256},
};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

fn digest(value: &Value) -> Result<String> {
    sha256(&serde_json::to_vec(value)?)
}
fn notes_ok(value: &Value) -> bool {
    value.as_str().is_some_and(|s| {
        (20..=2000).contains(&s.trim().chars().count())
            && s.chars().count() <= 2000
            && !s.contains("???")
            && !s.contains('\u{fffd}')
    })
}

/// Only the trusted, signed recovery event selects a review file and its hash.
fn reviewed_result(
    root: &Path,
    result: &Value,
    proof: &Value,
    recovery: &Value,
    supplement: &Value,
) -> Result<Value> {
    if supplement["schema_version"] != 1
        || supplement["review_status"] != "APPROVED"
        || supplement["accepted_result_sha256"] != digest(result)?
        || recovery["result_sha256"] != supplement["accepted_result_sha256"]
        || supplement["accepted_result_storage_sha256"] != proof["result_storage_sha256"]
        || recovery["result_storage_sha256"] != proof["result_storage_sha256"]
        || supplement["group_id"] != result["group_id"]
    {
        bail!("review status or accepted result binding mismatch");
    }
    for key in ["batch_id", "task_id"] {
        if supplement[key] != proof[key] || recovery[key] != proof[key] {
            bail!("review task binding mismatch");
        }
    }
    for key in ["original_action_id", "original_event_id"] {
        let id = recovery[key]
            .as_str()
            .context("recovery source ID missing")?;
        uuid::Uuid::parse_str(id)?;
        if supplement[key] != recovery[key] {
            bail!("review event binding mismatch");
        }
    }
    uuid::Uuid::parse_str(supplement["supplement_id"].as_str().context("review ID")?)?;
    let characters = result["characters"].as_array().context("characters")?;
    let required: BTreeSet<usize> = characters
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            c["confidence"].as_f64().is_some_and(|n| n < 0.6) && !notes_ok(&c["notes"])
        })
        .map(|(i, _)| i)
        .collect();
    if required.is_empty() {
        bail!("review overlay has no deficient entries");
    }
    let entries = supplement["entries"].as_array().context("review entries")?;
    let mut seen = BTreeSet::new();
    let mut effective = result.clone();
    for entry in entries {
        let i = usize::try_from(
            entry["character_index"]
                .as_u64()
                .context("review character index")?,
        )?;
        if !required.contains(&i) || !seen.insert(i) {
            bail!("review character set mismatch");
        }
        let character = &characters[i];
        if entry["review_status"] != "APPROVED"
            || !notes_ok(&entry["review_notes"])
            || entry["character_sha256"] != digest(character)?
            || entry["local_character_id"] != character["local_character_id"]
            || entry["image_ids"] != character["image_ids"]
            || entry["reviewer"]
                .as_str()
                .is_none_or(|s| s.trim().is_empty())
            || entry["reviewed_at"]
                .as_str()
                .is_none_or(|s| chrono::DateTime::parse_from_rfc3339(s).is_err())
        {
            bail!("incomplete or mismatched review entry");
        }
        let mut checked = BTreeSet::new();
        let prefix = format!("task/{}/", result["group_id"].as_str().context("group")?);
        let mut source_ids = BTreeSet::new();
        let mut has_refs = false;
        for evidence in entry["evidence"].as_array().context("review evidence")? {
            let relative = evidence["path"].as_str().context("evidence path")?;
            if relative.contains(['\\', ':'])
                || relative
                    .split('/')
                    .any(|p| p.is_empty() || p == "." || p == "..")
                || !(relative.starts_with(&prefix) || relative.starts_with("refs/"))
                || !checked.insert(relative)
            {
                bail!("unsafe or duplicate review evidence path");
            }
            let path = safe_existing_path(&root.join(relative))?;
            if !path.starts_with(root)
                || !path.is_file()
                || evidence["sha256"] != super::windows_security::file_sha256(&path)?
            {
                bail!("review source changed");
            }
            if relative.starts_with(&prefix) {
                source_ids.insert(
                    Path::new(relative)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .context("source image ID")?
                        .to_owned(),
                );
            } else {
                has_refs = true;
            }
        }
        if !has_refs
            || character["image_ids"]
                .as_array()
                .context("image IDs")?
                .iter()
                .any(|id| !source_ids.contains(id.as_str().unwrap_or("")))
        {
            bail!("review must cover every source image and actual references");
        }
        effective["characters"][i]["notes"] = entry["review_notes"].clone();
    }
    if seen != required {
        bail!("missing review entries");
    }
    emote::validate_result(root, &effective, proof)?;
    Ok(effective)
}

/// A partially published set is uncertain: refuse automatic repair. Operators
/// reconcile it first. Fully identical sets are safe idempotent readbacks.
fn publish_set(outputs: &[(PathBuf, Vec<u8>)]) -> Result<()> {
    let mut existing = 0;
    for (path, bytes) in outputs {
        safe_existing_path(path.parent().context("output parent")?)?;
        match fs::symlink_metadata(path) {
            Ok(_) => {
                let safe = safe_existing_path(path)?;
                if !safe.is_file() || fs::read(safe)? != *bytes {
                    bail!("publication conflict");
                }
                existing += 1;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    if existing == outputs.len() {
        return Ok(());
    }
    if existing != 0 {
        bail!("uncertain incomplete publication; reconcile before retry");
    }
    for (path, bytes) in outputs {
        emote::publish_new(path, bytes)?;
    }
    Ok(())
}

pub fn execute(root: &Path, supplements: &Path, event: &Value) -> Result<()> {
    let root = safe_existing_path(root)?;
    if event["action_kind"] == "on_batch_completed" {
        return emote::execute_using(
            &root,
            event,
            |root, result, proof| {
                let group = result["group_id"].as_str().context("group")?;
                if !emote::group(group) {
                    bail!("invalid group");
                }
                let receipt = emote::read_json(
                    &root.join("results/.receipts").join(format!("{group}.json")),
                )?;
                if receipt["schema_version"] == 1 {
                    return emote::validate_result(root, result, proof);
                }
                if receipt["schema_version"] != 2 || receipt["publisher_version"] != 2 {
                    bail!("unknown published receipt version");
                }
                let review =
                    emote::read_json(&root.join("results/.reviews").join(format!("{group}.json")))?;
                if receipt["review_sha256"] != digest(&review)? {
                    bail!("published review digest mismatch");
                }
                if review["recovery"].is_null() {
                    emote::validate_result(root, result, proof)
                } else {
                    if review["recovery"]["supplement_sha256"] != digest(&review["supplement"])? {
                        bail!("supplement digest mismatch");
                    }
                    reviewed_result(
                        root,
                        result,
                        proof,
                        &review["recovery"],
                        &review["supplement"],
                    )
                    .map(|_| ())
                }
            },
            publish_set,
        );
    }
    if event["action_kind"] != "on_task_completed" {
        bail!("unsupported v2 action");
    }
    let proofs = event["authorization"]["tasks"]
        .as_array()
        .context("broker proofs")?;
    if proofs.len() != 1 {
        bail!("one task proof required");
    }
    let proof = &proofs[0];
    emote::proof_valid(event, proof)?;
    let result = &event["result"];
    if event["task_id"] != proof["task_id"]
        || event["attempt_no"] != proof["attempt_no"]
        || digest(result)? != proof["result_sha256"]
    {
        bail!("event does not match accepted receipt");
    }
    let recovery = &event["recovery"];
    let supplement = if recovery.is_null() {
        emote::validate_result(&root, result, proof)?;
        Value::Null
    } else {
        let id = recovery["original_event_id"]
            .as_str()
            .context("original event")?;
        uuid::Uuid::parse_str(id)?;
        let path = safe_existing_path(&supplements.join(format!("{id}.json")))?;
        if !path.is_file() || fs::metadata(&path)?.len() > 4 * 1024 * 1024 {
            bail!("invalid bounded review input");
        }
        let bytes = fs::read(path)?;
        if bytes.len() > 4 * 1024 * 1024 || sha256(&bytes)? != recovery["supplement_sha256"] {
            bail!("unapproved supplement hash");
        }
        let supplement: Value = serde_json::from_slice(&bytes)?;
        reviewed_result(&root, result, proof, recovery, &supplement)?;
        supplement
    };
    let group = result["group_id"].as_str().context("group")?;
    let review = json!({"schema_version":1,"publisher_version":2,"publication_event_id":event["event_id"],"recovery":recovery,"supplement":supplement});
    let receipt = json!({"schema_version":2,"publisher_version":2,"task_id":proof["task_id"],"batch_id":proof["batch_id"],"agent_id":proof["agent_id"],"attempt_no":proof["attempt_no"],"result_sha256":proof["result_sha256"],"review_sha256":digest(&review)?,"publication_event_id":event["event_id"]});
    let results = root.join("results");
    publish_set(&[
        (
            results.join(format!("{group}.json")),
            serde_json::to_vec(result)?,
        ),
        (
            results.join(".reviews").join(format!("{group}.json")),
            serde_json::to_vec(&review)?,
        ),
        (
            results.join(".receipts").join(format!("{group}.json")),
            serde_json::to_vec(&receipt)?,
        ),
    ])
}

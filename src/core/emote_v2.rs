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
fn present(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
fn recovery_binding(proof: &Value, actual: Option<&Value>) -> Result<()> {
    if let Some(value) = proof.get("recoveries") {
        let recoveries = value.as_array().context("invalid recovery proofs")?;
        match actual {
            Some(recovery) if recoveries.len() == 1 && recoveries[0] == *recovery => (),
            None if recoveries.is_empty() => (),
            _ => bail!("published recovery does not match authorized successful recovery"),
        }
    }
    Ok(())
}
fn notes_ok(value: &Value) -> bool {
    value.as_str().is_some_and(|s| {
        (20..=2000).contains(&s.trim().chars().count())
            && s.chars().count() <= 2000
            && !s.contains("???")
            && !s.contains('\u{fffd}')
    })
}

// A correction is independently selected by the signed recovery supplement hash.
// QA explicitly attests the complete effective result, rather than just a note.
fn corrected_result(
    root: &Path,
    supplements: &Path,
    result: &Value,
    proof: &Value,
    recovery: &Value,
    record: &Value,
) -> Result<Value> {
    if record["schema_version"] != 2
        || record["status"] != "APPROVED"
        || record["revision"].as_u64().is_none_or(|v| v == 0)
        || record["group_id"] != result["group_id"]
        || record["accepted_result_sha256"] != digest(result)?
        || record["accepted_result_storage_sha256"] != proof["result_storage_sha256"]
        || recovery["result_sha256"] != record["accepted_result_sha256"]
        || recovery["result_storage_sha256"] != proof["result_storage_sha256"]
    {
        bail!("correction result binding mismatch");
    }
    uuid::Uuid::parse_str(record["correction_id"].as_str().context("correction ID")?)?;
    for key in ["batch_id", "task_id"] {
        if record[key] != proof[key] || recovery[key] != proof[key] {
            bail!("correction task binding mismatch");
        }
    }
    for key in ["original_action_id", "original_event_id"] {
        uuid::Uuid::parse_str(recovery[key].as_str().context("source ID")?)?;
        if record[key] != recovery[key] {
            bail!("correction event binding mismatch");
        }
    }
    let author = record["author"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .context("correction author")?;
    let mut effective = result.clone();
    let changes = record["changes"].as_array().context("correction changes")?;
    if changes.is_empty() || changes.len() > 3 {
        bail!("invalid correction changes");
    }
    let mut fields = BTreeSet::new();
    for change in changes {
        let field = change["field"].as_str().context("correction field")?;
        if !["characters", "uncertain", "general_notes"].contains(&field)
            || !fields.insert(field)
            || change.get("before") != result.get(field)
            || change.get("after").is_none()
            || change["before"] == change["after"]
        {
            bail!("correction conflict or unsupported field");
        }
        effective[field] = change["after"].clone();
    }
    emote::validate_correction(root, &effective, proof)?;
    let prefix = format!("task/{}/", result["group_id"].as_str().context("group")?);
    let mut paths = BTreeSet::new();
    let mut image_ids = BTreeSet::new();
    let evidence = record["evidence"]
        .as_array()
        .context("correction evidence")?;
    for item in evidence {
        let relative = item["path"].as_str().context("evidence path")?;
        if relative.contains(['\\', ':'])
            || relative
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == "..")
            || !(relative.starts_with(&prefix) || relative.starts_with("refs/"))
            || !paths.insert(relative)
        {
            bail!("unsafe correction evidence");
        }
        let path = safe_existing_path(&root.join(relative))?;
        if !path.starts_with(root)
            || !path.is_file()
            || item["sha256"] != super::windows_security::file_sha256(&path)?
        {
            bail!("correction evidence changed");
        }
        if relative.starts_with(&prefix) {
            if relative[prefix.len()..].contains('/') {
                bail!("invalid source image path");
            }
            let id = Path::new(relative)
                .file_stem()
                .and_then(|v| v.to_str())
                .context("source image")?;
            if !image_ids.insert(id.to_owned()) {
                bail!("duplicate source image");
            }
        }
    }
    let expected: BTreeSet<String> = result["source_image_ids"]
        .as_array()
        .context("source IDs")?
        .iter()
        .map(|v| v.as_str().unwrap_or("").to_owned())
        .collect();
    if image_ids != expected {
        bail!("correction source coverage mismatch");
    }
    for character in effective["characters"].as_array().context("characters")? {
        for p in character["reference_match"]["checked_paths"]
            .as_array()
            .context("checked paths")?
        {
            if !paths.contains(p.as_str().context("reference path")?) {
                bail!("missing reference evidence");
            }
        }
        for candidate in character["reference_match"]["candidates"]
            .as_array()
            .context("candidates")?
        {
            if !paths.contains(candidate["icon_path"].as_str().context("candidate path")?) {
                bail!("missing candidate evidence");
            }
        }
    }
    for item in effective["uncertain"].as_array().context("uncertain")? {
        for p in item["checked_paths"].as_array().context("checked paths")? {
            if !paths.contains(p.as_str().context("reference path")?) {
                bail!("missing reference evidence");
            }
        }
        for candidate in item["reference_candidates"]
            .as_array()
            .context("candidates")?
        {
            if !paths.contains(candidate["icon_path"].as_str().context("candidate path")?) {
                bail!("missing candidate evidence");
            }
        }
    }
    let qa = &record["qa"];
    let qa_path = qa["path"].as_str().context("QA path")?;
    if !qa_path.starts_with("qa/")
        || qa_path.contains(['\\', ':'])
        || qa_path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        bail!("unsafe QA path");
    }
    let base = safe_existing_path(supplements)?;
    let path = safe_existing_path(&base.join(qa_path))?;
    if !path.starts_with(&base) || !path.is_file() || fs::metadata(&path)?.len() > 4 * 1024 * 1024 {
        bail!("invalid QA record");
    }
    let bytes = fs::read(path)?;
    if qa["sha256"] != sha256(&bytes)? {
        bail!("QA record changed");
    }
    let attestation: Value = serde_json::from_slice(&bytes)?;
    let reviewer = attestation["reviewer"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .context("QA reviewer")?;
    if reviewer == author
        || qa["reviewer"] != reviewer
        || attestation["schema_version"] != 1
        || attestation["status"] != "APPROVED"
        || attestation["correction_id"] != record["correction_id"]
        || attestation["group_id"] != record["group_id"]
        || attestation["accepted_result_sha256"] != record["accepted_result_sha256"]
        || attestation["effective_result_sha256"] != digest(&effective)?
    {
        bail!("independent QA binding mismatch");
    }
    Ok(effective)
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

fn publish_correction(
    root: &Path,
    result: &Value,
    proof: &Value,
    event: &Value,
    record: &Value,
) -> Result<()> {
    let results = root.join("results");
    let dir = safe_existing_path(&results.join(".corrections"))?;
    let group = result["group_id"].as_str().context("group")?;
    if !emote::group(group) {
        bail!("invalid group");
    }
    let lock = dir.join(format!("{group}.lock"));
    let guard = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)
        .context("correction publication busy or uncertain; reconcile lock")?;
    drop(guard);
    let mut started = false;
    let operation = (|| -> Result<()> {
        let original = results.join(format!("{group}.json"));
        let receipt_path = results.join(".receipts").join(format!("{group}.json"));
        let correction_path = dir.join(format!("{group}.json"));
        let wrapper = json!({"schema_version":1,"publication_event_id":event["event_id"],"recovery":event["recovery"],"record":record});
        let correction_bytes = serde_json::to_vec(&wrapper)?;
        if present(&correction_path)?
            && fs::read(safe_existing_path(&correction_path)?)? != correction_bytes
        {
            bail!("correction publication conflict");
        }
        let have_original = present(&original)?;
        let have_receipt = present(&receipt_path)?;
        if have_original != have_receipt {
            bail!("uncertain incomplete original publication");
        }
        if have_original {
            if emote::read_json(&original)? != *result {
                bail!("original publication conflict");
            }
            let receipt = emote::read_json(&receipt_path)?;
            for key in [
                "task_id",
                "batch_id",
                "agent_id",
                "attempt_no",
                "result_sha256",
            ] {
                if receipt[key] != proof[key] {
                    bail!("original receipt conflict");
                }
            }
            if receipt["schema_version"] == 2 && receipt["publisher_version"] == 2 {
                let review =
                    emote::read_json(&results.join(".reviews").join(format!("{group}.json")))?;
                if receipt["review_sha256"] != digest(&review)? {
                    bail!("original review conflict");
                }
            } else if receipt["schema_version"] != 1 {
                bail!("unknown original receipt version");
            }
        } else {
            if present(&correction_path)? {
                bail!("uncertain correction without original");
            }
            let receipt = json!({"schema_version":1,"task_id":proof["task_id"],"batch_id":proof["batch_id"],"agent_id":proof["agent_id"],"attempt_no":proof["attempt_no"],"result_sha256":proof["result_sha256"]});
            started = true;
            emote::publish_new(&original, &serde_json::to_vec(result)?)?;
            emote::publish_new(&receipt_path, &serde_json::to_vec(&receipt)?)?;
        }
        if !present(&correction_path)? {
            started = true;
        }
        emote::publish_new(&correction_path, &correction_bytes)?;
        Ok(())
    })();
    // A failed write or crash leaves an explicit uncertain marker for reconciliation.
    if operation.is_ok() || !started {
        fs::remove_file(lock)?;
    }
    operation
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
                let correction_path = root
                    .join("results/.corrections")
                    .join(format!("{group}.json"));
                let lock_path = root
                    .join("results/.corrections")
                    .join(format!("{group}.lock"));
                if present(&lock_path)? {
                    bail!("uncertain correction publication");
                }
                if present(&correction_path)? {
                    let wrapper = emote::read_json(&correction_path)?;
                    if proof.get("recoveries").is_none() {
                        bail!("correction requires broker recovery proof");
                    }
                    recovery_binding(proof, Some(&wrapper["recovery"]))?;
                    if wrapper["schema_version"] != 1
                        || wrapper["recovery"]["supplement_sha256"] != digest(&wrapper["record"])?
                    {
                        bail!("correction digest mismatch");
                    }
                    uuid::Uuid::parse_str(
                        wrapper["publication_event_id"]
                            .as_str()
                            .context("publication event")?,
                    )?;
                    let effective = corrected_result(
                        root,
                        supplements,
                        result,
                        proof,
                        &wrapper["recovery"],
                        &wrapper["record"],
                    )?;
                    let metadata = json!({"path":format!(".corrections/{group}.json"),"sha256":digest(&wrapper)?,"correction_id":wrapper["record"]["correction_id"],"revision":wrapper["record"]["revision"],"qa_sha256":wrapper["record"]["qa"]["sha256"]});
                    return Ok((effective, Some(metadata)));
                }
                let receipt = emote::read_json(
                    &root.join("results/.receipts").join(format!("{group}.json")),
                )?;
                if receipt["schema_version"] == 1 {
                    recovery_binding(proof, None)?;
                    emote::validate_result(root, result, proof)?;
                    return Ok((result.clone(), None));
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
                    recovery_binding(proof, None)?;
                    emote::validate_result(root, result, proof)?;
                    Ok((result.clone(), None))
                } else {
                    recovery_binding(proof, Some(&review["recovery"]))?;
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
                    .map(|result| (result, None))
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
        if supplement["schema_version"] == 2 {
            if digest(&supplement)? != recovery["supplement_sha256"] {
                bail!("correction must use canonical JSON");
            }
            corrected_result(&root, supplements, result, proof, recovery, &supplement)?;
            return publish_correction(&root, result, proof, event, &supplement);
        }
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

#!/usr/bin/env python3
"""Validate one Phase G result against its task IDs and refs directory."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path, PurePosixPath
from typing import Any


AGENTS = {f"A{number}" for number in range(6)}
LOGICAL_AGENT_RE = re.compile(r"^emote-[0-9A-F]{4}$")
GROUP_RE = re.compile(r"^(0[0-9]|[1-9][0-9]|1[01][0-9]|120)$")
IMAGE_RE = re.compile(r"^[0-9]+$")
CHARACTER_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")
GAMES = {"arknights", "endfield", "unknown"}
REVIEW_PRIORITIES = {"low", "medium", "high", "urgent"}


def is_object(value: Any) -> bool:
    return isinstance(value, dict)


def exact_keys(value: Any, required: set[str], allowed: set[str], where: str, errors: list[str]) -> bool:
    if not is_object(value):
        errors.append(f"{where}: expected object")
        return False
    missing = required - value.keys()
    extra = value.keys() - allowed
    if missing:
        errors.append(f"{where}: missing keys {sorted(missing)}")
    if extra:
        errors.append(f"{where}: unknown keys {sorted(extra)}")
    return not missing and not extra


def safe_ref_file(refs_root: Path, value: Any, where: str, errors: list[str]) -> bool:
    if not isinstance(value, str) or not value.startswith("refs/") or "\\" in value or ":" in value:
        errors.append(f"{where}: reference path must be a relative refs/... path")
        return False
    relative = PurePosixPath(value)
    if relative.is_absolute() or any(part in ("", ".", "..") for part in relative.parts):
        errors.append(f"{where}: unsafe reference path")
        return False
    suffix = Path(*relative.parts[1:])
    root = refs_root.resolve(strict=True)
    target = refs_root / suffix
    if target.is_symlink() or not target.is_file():
        errors.append(f"{where}: referenced file is missing or a symlink")
        return False
    try:
        resolved = target.resolve(strict=True)
    except OSError:
        errors.append(f"{where}: referenced file cannot be resolved")
        return False
    if not resolved.is_relative_to(root):
        errors.append(f"{where}: reference escapes refs/")
        return False
    return True


def validate_candidate(candidate: Any, refs_root: Path, where: str, errors: list[str]) -> None:
    required = {"game", "operator_name", "icon_path"}
    allowed = required | {"visual_notes"}
    exact_keys(candidate, required, allowed, where, errors)
    if not is_object(candidate):
        return
    if not isinstance(candidate.get("game"), str) or candidate.get("game") not in {"arknights", "endfield"}:
        errors.append(f"{where}.game: unsupported game")
    name = candidate.get("operator_name")
    if not isinstance(name, str) or not name.strip() or len(name) > 128:
        errors.append(f"{where}.operator_name: invalid name")
    notes = candidate.get("visual_notes", "")
    if not isinstance(notes, str) or len(notes) > 1000:
        errors.append(f"{where}.visual_notes: invalid notes")
    safe_ref_file(refs_root, candidate.get("icon_path"), f"{where}.icon_path", errors)


def validate_paths(paths: Any, refs_root: Path, where: str, errors: list[str]) -> None:
    if not isinstance(paths, list) or not paths:
        errors.append(f"{where}: at least one checked refs path is required")
        return
    if any(not isinstance(path, str) for path in paths):
        errors.append(f"{where}: checked paths must be strings")
    elif len(paths) != len(set(paths)):
        errors.append(f"{where}: duplicate checked path")
    for index, path in enumerate(paths):
        safe_ref_file(refs_root, path, f"{where}[{index}]", errors)


def validate_reference_match(value: Any, refs_root: Path, where: str, errors: list[str]) -> None:
    required = {"match_status", "candidates", "checked_paths", "conclusion"}
    exact_keys(value, required, required, where, errors)
    if not is_object(value):
        return
    if not isinstance(value.get("match_status"), str) or value.get("match_status") not in {"matched", "candidate", "no_match"}:
        errors.append(f"{where}.match_status: invalid status")
    candidates = value.get("candidates")
    if not isinstance(candidates, list) or len(candidates) > 20:
        errors.append(f"{where}.candidates: expected array of at most 20 items")
    else:
        for index, candidate in enumerate(candidates):
            validate_candidate(candidate, refs_root, f"{where}.candidates[{index}]", errors)
    validate_paths(value.get("checked_paths"), refs_root, f"{where}.checked_paths", errors)
    conclusion = value.get("conclusion")
    if not isinstance(conclusion, str) or not 10 <= len(conclusion.strip()) <= 2000:
        errors.append(f"{where}.conclusion: provide a concise checked conclusion")


def validate_result(result: Any, expected: dict[str, Any], refs_root: Path) -> list[str]:
    errors: list[str] = []
    if not isinstance(expected, dict):
        return ["expected: task context must be an object"]
    required = {
        "schema_version",
        "group_id",
        "agent_id",
        "attempt_no",
        "source_image_ids",
        "characters",
        "uncertain",
        "general_notes",
    }
    if not exact_keys(result, required, required, "result", errors):
        return errors
    if result["schema_version"] != 1 or isinstance(result["schema_version"], bool):
        errors.append("schema_version: expected 1")
    group_id = result["group_id"]
    if not isinstance(group_id, str) or not GROUP_RE.fullmatch(group_id):
        errors.append("group_id: expected a group from 00 through 120")
    if group_id != expected.get("group_id"):
        errors.append("group_id: does not match the claimed task")
    agent_id = result["agent_id"]
    if not isinstance(agent_id, str) or (agent_id not in AGENTS and not LOGICAL_AGENT_RE.fullmatch(agent_id)):
        errors.append("agent_id: expected emote-<4 uppercase hex> or legacy A0 through A5")
    if expected.get("agent_id") is not None and agent_id != expected["agent_id"]:
        errors.append("agent_id: does not match the expected worker label")
    attempt_no = result["attempt_no"]
    if not isinstance(attempt_no, int) or isinstance(attempt_no, bool) or attempt_no < 1:
        errors.append("attempt_no: expected a positive integer")
    if expected.get("attempt_no") is not None and attempt_no != expected["attempt_no"]:
        errors.append("attempt_no: does not match the current claim")

    source_ids = result["source_image_ids"]
    expected_ids = expected.get("source_image_ids")
    if not isinstance(source_ids, list) or not source_ids or len(source_ids) > 10:
        errors.append("source_image_ids: expected one to ten IDs")
        source_ids = []
    source_ids_valid = all(
        isinstance(image_id, str) and IMAGE_RE.fullmatch(image_id) for image_id in source_ids
    )
    if not source_ids_valid:
        errors.append("source_image_ids: IDs must be numeric strings")
    if source_ids_valid and len(source_ids) != len(set(source_ids)):
        errors.append("source_image_ids: duplicate IDs")
    if isinstance(expected_ids, list) and source_ids != expected_ids:
        errors.append("source_image_ids: list does not exactly match the claimed task")
    expected_ids_valid = isinstance(expected_ids, list) and all(
        isinstance(image_id, str) and IMAGE_RE.fullmatch(image_id) for image_id in expected_ids
    )
    allowed_ids = set(expected_ids) if expected_ids_valid else set(source_ids) if source_ids_valid else set()

    characters = result["characters"]
    if not isinstance(characters, list) or len(characters) > 20:
        errors.append("characters: expected an array of at most 20 entries")
        characters = []
    covered: set[str] = set()
    local_ids: set[str] = set()
    character_required = {
        "local_character_id",
        "image_ids",
        "representative_image_id",
        "visual_description",
        "game",
        "reference_match",
        "confidence",
        "notes",
    }
    for index, character in enumerate(characters):
        where = f"characters[{index}]"
        exact_keys(character, character_required, character_required, where, errors)
        if not is_object(character):
            continue
        local_id = character.get("local_character_id")
        if not isinstance(local_id, str) or not CHARACTER_RE.fullmatch(local_id) or local_id in local_ids:
            errors.append(f"{where}.local_character_id: invalid or duplicate local ID")
        else:
            local_ids.add(local_id)
        image_ids = character.get("image_ids")
        if not isinstance(image_ids, list) or not image_ids or len(image_ids) > 10:
            errors.append(f"{where}.image_ids: expected one to ten image IDs")
            image_ids = []
        image_ids_valid = all(isinstance(image_id, str) for image_id in image_ids)
        if image_ids_valid and len(image_ids) != len(set(image_ids)):
            errors.append(f"{where}.image_ids: duplicate IDs")
        if any(not isinstance(image_id, str) or image_id not in allowed_ids for image_id in image_ids):
            errors.append(f"{where}.image_ids: contains an ID outside this task")
        covered.update(image_id for image_id in image_ids if isinstance(image_id, str))
        representative = character.get("representative_image_id")
        if representative not in image_ids:
            errors.append(f"{where}.representative_image_id: must belong to image_ids")
        description = character.get("visual_description")
        if not isinstance(description, str) or not 20 <= len(description.strip()) <= 4000:
            errors.append(f"{where}.visual_description: describe visible traits, not only a name")
        if not isinstance(character.get("game"), str) or character.get("game") not in GAMES:
            errors.append(f"{where}.game: unsupported value")
        confidence = character.get("confidence")
        if not isinstance(confidence, (int, float)) or isinstance(confidence, bool) or not 0 <= confidence <= 1:
            errors.append(f"{where}.confidence: expected a value from 0 to 1")
        notes = character.get("notes")
        if not isinstance(notes, str) or len(notes) > 2000:
            errors.append(f"{where}.notes: invalid notes")
        elif isinstance(confidence, (int, float)) and confidence < 0.6 and len(notes.strip()) < 20:
            errors.append(f"{where}.notes: low-confidence matches need review context")
        validate_reference_match(character.get("reference_match"), refs_root, f"{where}.reference_match", errors)

    uncertain = result["uncertain"]
    if not isinstance(uncertain, list) or len(uncertain) > 10:
        errors.append("uncertain: expected an array of at most ten entries")
        uncertain = []
    uncertain_ids: set[str] = set()
    uncertain_required = {
        "image_id",
        "visible_features",
        "reference_candidates",
        "checked_paths",
        "check_conclusion",
        "reason",
        "review_priority",
    }
    for index, item in enumerate(uncertain):
        where = f"uncertain[{index}]"
        exact_keys(item, uncertain_required, uncertain_required, where, errors)
        if not is_object(item):
            continue
        image_id = item.get("image_id")
        if not isinstance(image_id, str) or image_id not in allowed_ids:
            errors.append(f"{where}.image_id: outside this task")
        elif image_id in uncertain_ids:
            errors.append(f"{where}.image_id: duplicate uncertain entry")
        else:
            uncertain_ids.add(image_id)
            covered.add(image_id)
        visible = item.get("visible_features")
        if not isinstance(visible, str) or not 10 <= len(visible.strip()) <= 2000:
            errors.append(f"{where}.visible_features: describe visible evidence")
        candidates = item.get("reference_candidates")
        if not isinstance(candidates, list) or len(candidates) > 20:
            errors.append(f"{where}.reference_candidates: expected an array of at most 20 items")
        else:
            for candidate_index, candidate in enumerate(candidates):
                validate_candidate(candidate, refs_root, f"{where}.reference_candidates[{candidate_index}]", errors)
        validate_paths(item.get("checked_paths"), refs_root, f"{where}.checked_paths", errors)
        for field in ("check_conclusion", "reason"):
            text = item.get(field)
            if not isinstance(text, str) or not 10 <= len(text.strip()) <= 2000:
                errors.append(f"{where}.{field}: provide checked evidence")
        if not isinstance(item.get("review_priority"), str) or item.get("review_priority") not in REVIEW_PRIORITIES:
            errors.append(f"{where}.review_priority: invalid priority")

    if covered != allowed_ids:
        missing = sorted(allowed_ids - covered, key=int)
        extra = sorted(covered - allowed_ids)
        errors.append(f"image coverage mismatch: missing={missing}, extra={extra}")
    general_notes = result["general_notes"]
    if not isinstance(general_notes, str) or len(general_notes) > 4000:
        errors.append("general_notes: expected a string up to 4000 characters")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--result", type=Path, required=True)
    parser.add_argument("--expected", type=Path, required=True, help="task context with group_id/source_image_ids and optional agent_id/attempt_no")
    parser.add_argument("--refs-root", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = json.loads(args.result.read_text(encoding="utf-8"))
        expected = json.loads(args.expected.read_text(encoding="utf-8"))
        refs_root = args.refs_root.resolve(strict=True)
        errors = validate_result(result, expected, refs_root)
        output = {"ok": not errors, "group_id": result.get("group_id") if is_object(result) else None, "errors": errors}
        print(json.dumps(output, ensure_ascii=False, indent=2))
        return 0 if not errors else 1
    except (OSError, json.JSONDecodeError, TypeError) as error:
        print(json.dumps({"ok": False, "errors": [str(error)]}, ensure_ascii=False), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())

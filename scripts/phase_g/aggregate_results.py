#!/usr/bin/env python3
"""Build preliminary indexes and human-review candidates; never assigns global IDs."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any

from validate_result import validate_result


class AggregateError(ValueError):
    pass


def read_json(path: Path) -> Any:
    if path.is_symlink() or not path.is_file():
        raise AggregateError(f"input is missing, not a file, or a symlink: {path.name}")
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise AggregateError(f"cannot read {path.name}: {error}") from error


def build_indexes(request_path: Path, results_dir: Path, refs_root: Path) -> tuple[dict[str, Any], dict[str, Any]]:
    request = read_json(request_path)
    if not isinstance(request, dict) or not isinstance(request.get("tasks"), list):
        raise AggregateError("request must contain a tasks array")
    expected: dict[str, dict[str, Any]] = {}
    for task in request["tasks"]:
        if not isinstance(task, dict) or not isinstance(task.get("payload"), dict):
            raise AggregateError("request has an invalid task entry")
        payload = task["payload"]
        group = payload.get("group_id")
        ids = payload.get("source_image_ids")
        if not isinstance(group, str) or group in expected or not isinstance(ids, list):
            raise AggregateError("request has duplicate/invalid group IDs")
        expected[group] = {"group_id": group, "source_image_ids": ids}

    results_root = results_dir.resolve(strict=True)
    refs_root = refs_root.resolve(strict=True)
    result_files = sorted(path for path in results_root.glob("*.json")
                          if path.stem.isascii() and path.stem.isdecimal())
    seen_groups: set[str] = set()
    result_entries: list[dict[str, Any]] = []
    reference_index: dict[tuple[str, str], dict[str, list[dict[str, Any]]]] = defaultdict(lambda: defaultdict(list))
    for path in result_files:
        if path.is_symlink() or not path.resolve(strict=True).is_relative_to(results_root):
            raise AggregateError(f"unsafe result path: {path.name}")
        group = path.stem
        if group not in expected or group in seen_groups:
            raise AggregateError(f"unexpected or duplicate result group: {group}")
        result = read_json(path)
        if not isinstance(result, dict):
            raise AggregateError(f"result must be a JSON object: {path.name}")
        if result.get("group_id") != group:
            raise AggregateError(f"filename and result group differ: {path.name}")
        errors = validate_result(result, expected[group], refs_root)
        if errors:
            raise AggregateError(f"invalid result {path.name}: {'; '.join(errors)}")
        seen_groups.add(group)
        raw = path.read_bytes()
        characters = result["characters"]
        result_entries.append(
            {
                "group_id": group,
                "path": path.name,
                "sha256": hashlib.sha256(raw).hexdigest(),
                "character_count": len(characters),
                "uncertain_count": len(result["uncertain"]),
            }
        )
        for character in characters:
            match = character["reference_match"]
            for candidate in match["candidates"]:
                key = (candidate["game"], candidate["operator_name"].strip().casefold())
                reference_index[key][group].append(
                    {
                        "local_character_id": character["local_character_id"],
                        "image_ids": character["image_ids"],
                        "visual_description": character["visual_description"],
                        "confidence": character["confidence"],
                        "operator_name": candidate["operator_name"],
                        "icon_path": candidate["icon_path"],
                    }
                )

    missing = sorted(set(expected) - seen_groups)
    candidates = []
    for (game, normalized_name), groups in sorted(reference_index.items()):
        if len(groups) < 2:
            continue
        display_names = [entry["operator_name"] for entries in groups.values() for entry in entries]
        candidates.append(
            {
                "candidate_key": f"shared-reference:{game}:{normalized_name}",
                "game": game,
                "reference_name_samples": sorted(set(display_names)),
                "groups": [
                    {"group_id": group, "local_characters": entries}
                    for group, entries in sorted(groups.items())
                ],
                "review_priority": "medium",
                "reason": "The same reference candidate appears in multiple groups; compare visual evidence before any merge.",
            }
        )

    state = "STRUCTURALLY_COMPLETE" if not missing else "PARTIAL"
    index = {
        "schema_version": 1,
        "state": state,
        "scope": "preliminary index only; no authenticated identity, completion receipt, or global character merge is asserted",
        "expected_groups": len(expected),
        "available_results": len(result_entries),
        "missing_groups": missing,
        "results": sorted(result_entries, key=lambda row: int(row["group_id"])),
    }
    review_queue = {
        "schema_version": 1,
        "state": "candidate_review",
        "candidate_count": len(candidates),
        "candidates": candidates,
        "missing_groups": missing,
        "note": "Candidates are suggestions only. Do not assign global IDs or merge characters automatically.",
    }
    return index, review_queue


def write_new(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    encoded = json.dumps(value, ensure_ascii=False, indent=2).encode("utf-8")
    with path.open("xb") as stream:
        stream.write(encoded)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--request", type=Path, required=True, help="prepared request emitted by prepare_initial_tasks.py")
    parser.add_argument("--results-dir", type=Path, required=True)
    parser.add_argument("--refs-root", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--write", action="store_true", help="write index.json and review_queue/candidates.json; default is dry-run")
    args = parser.parse_args()
    try:
        index, review_queue = build_indexes(args.request, args.results_dir, args.refs_root)
        if args.write:
            targets = [args.out_dir / "index.json", args.out_dir / "review_queue" / "candidates.json"]
            if any(path.exists() for path in targets):
                raise AggregateError("an output already exists; refusing to overwrite")
            write_new(targets[0], index)
            write_new(targets[1], review_queue)
            state = "written"
        else:
            state = "dry_run"
        print(
            json.dumps(
                {
                    "state": state,
                    "index_state": index["state"],
                    "available_results": index["available_results"],
                    "expected_groups": index["expected_groups"],
                    "missing_groups": index["missing_groups"],
                    "candidate_count": review_queue["candidate_count"],
                    "outputs": [str(args.out_dir / "index.json"), str(args.out_dir / "review_queue" / "candidates.json")],
                },
                ensure_ascii=False,
                indent=2,
            )
        )
        return 0
    except (AggregateError, OSError, json.JSONDecodeError) as error:
        print(json.dumps({"state": "rejected", "error": str(error)}, ensure_ascii=False), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())

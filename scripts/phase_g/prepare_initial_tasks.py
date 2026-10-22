#!/usr/bin/env python3
"""Build a read-only, dry-run Coord request for the 121 emote groups."""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import sys
from pathlib import Path
from typing import Any


EXPECTED_GROUPS = {f"{number:02d}": 10 for number in range(120)} | {"120": 6}
EXPECTED_UNIQUE_IMAGES = 1206
EXPECTED_DUPLICATE_IMAGES = 12


class InputError(ValueError):
    pass


def read_csv(path: Path) -> list[dict[str, str]]:
    try:
        with path.open("r", encoding="utf-8-sig", newline="") as stream:
            reader = csv.DictReader(stream)
            if not reader.fieldnames:
                raise InputError(f"CSV has no header: {path.name}")
            return list(reader)
    except OSError as error:
        raise InputError(f"cannot read {path.name}: {error}") from error


def contained_file(root: Path, relative_name: str) -> Path:
    relative = Path(relative_name)
    if relative.is_absolute() or any(part in ("", ".", "..") for part in relative.parts):
        raise InputError(f"unsafe source path in manifest: {relative_name!r}")
    path = root / relative
    if path.is_symlink() or not path.is_file():
        raise InputError(f"manifest source is missing or is a symlink: {relative_name}")
    resolved = path.resolve(strict=True)
    if not resolved.is_relative_to(root):
        raise InputError(f"manifest source escapes the source root: {relative_name}")
    return resolved


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest().upper()


def validate_inventory(root: Path) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    manifest_path = root / "image_manifest.csv"
    inventory_path = root / "task_inventory.csv"
    manifest = read_csv(manifest_path)
    inventory = read_csv(inventory_path)
    required_manifest = {"id", "source_name", "sha256", "status", "duplicate_of", "task"}
    required_inventory = {"task", "count"}
    if not manifest or not required_manifest.issubset(manifest[0]):
        raise InputError("image_manifest.csv has unexpected columns")
    if not inventory or not required_inventory.issubset(inventory[0]):
        raise InputError("task_inventory.csv has unexpected columns")

    ids: dict[str, dict[str, str]] = {}
    unique: list[dict[str, str]] = []
    duplicates: list[dict[str, str]] = []
    for row in manifest:
        image_id = row["id"].strip()
        if not image_id.isdigit() or image_id in ids:
            raise InputError(f"invalid or duplicate image ID: {image_id!r}")
        row["id"] = image_id
        ids[image_id] = row
        status = row["status"].strip().lower()
        if status == "unique":
            unique.append(row)
        elif status == "duplicate":
            duplicates.append(row)
        else:
            raise InputError(f"unsupported manifest status for image {image_id}: {status!r}")

    if len(manifest) != EXPECTED_UNIQUE_IMAGES + EXPECTED_DUPLICATE_IMAGES:
        raise InputError(f"expected 1218 manifest rows, found {len(manifest)}")
    if len(unique) != EXPECTED_UNIQUE_IMAGES or len(duplicates) != EXPECTED_DUPLICATE_IMAGES:
        raise InputError("manifest unique/duplicate counts do not match the audited source set")

    inventory_counts: dict[str, int] = {}
    for row in inventory:
        group = row["task"].strip()
        if group in inventory_counts:
            raise InputError(f"duplicate task inventory row: {group}")
        try:
            inventory_counts[group] = int(row["count"])
        except ValueError as error:
            raise InputError(f"invalid count for task {group}") from error
    expected_inventory = {f"{number:02d}": EXPECTED_GROUPS.get(f"{number:02d}", 0) for number in range(123)}
    if inventory_counts != expected_inventory:
        raise InputError("task_inventory.csv must describe groups 00-119 (10 each), 120 (6), and 121-122 (0)")

    group_rows: dict[str, list[dict[str, str]]] = {group: [] for group in EXPECTED_GROUPS}
    for row in unique:
        group = row["task"].strip()
        if group not in group_rows:
            raise InputError(f"unique image {row['id']} is assigned outside groups 00-120")
        group_rows[group].append(row)
    if {group: len(rows) for group, rows in group_rows.items()} != EXPECTED_GROUPS:
        raise InputError("unique manifest assignments do not match task_inventory.csv")

    for row in duplicates:
        target_id = row["duplicate_of"].strip()
        target = ids.get(target_id)
        if not target or target["status"].strip().lower() != "unique":
            raise InputError(f"duplicate image {row['id']} does not point to a unique image")
        if row["sha256"].upper() != target["sha256"].upper():
            raise InputError(f"duplicate image {row['id']} SHA-256 differs from its target")
        if row["task"].strip():
            raise InputError(f"duplicate image {row['id']} must not be assigned to a task")

    for row in manifest:
        path = contained_file(root, row["source_name"].strip())
        if sha256(path) != row["sha256"].strip().upper():
            raise InputError(f"SHA-256 mismatch for source image ID {row['id']}")

    tasks: list[dict[str, Any]] = []
    for group, expected_count in EXPECTED_GROUPS.items():
        task_dir = root / "task" / group
        if task_dir.is_symlink() or not task_dir.is_dir():
            raise InputError(f"task directory is missing or a symlink: {group}")
        expected_rows = group_rows[group]
        task_names = {
            f"{row['id']}{Path(row['source_name'].strip()).suffix}": row
            for row in expected_rows
        }
        actual_files = list(task_dir.iterdir())
        if any(path.is_symlink() or not path.is_file() for path in actual_files):
            raise InputError(f"task directory {group} contains a non-file or symlink")
        if {path.name for path in actual_files} != set(task_names) or len(actual_files) != expected_count:
            raise InputError(f"task directory {group} does not match its manifest assignment")
        by_name = {path.name: path for path in actual_files}
        for row in expected_rows:
            task_name = f"{row['id']}{Path(row['source_name'].strip()).suffix}"
            if sha256(by_name[task_name]) != row["sha256"].strip().upper():
                raise InputError(f"task copy SHA-256 mismatch for group {group}, image {row['id']}")
        image_ids = sorted((row["id"] for row in expected_rows), key=int)
        tasks.append(
            {
                "key": f"group-{group}",
                "name": f"Categorize emote group {group}",
                "payload": {
                    "schema_version": 1,
                    "group_id": group,
                    "expected_image_count": expected_count,
                    "source_image_ids": image_ids,
                },
            }
        )

    empty_dirs = []
    for name in [*(f"A{number}" for number in range(6)), "task/121", "task/122"]:
        path = root / name
        if path.is_symlink() or not path.is_dir():
            raise InputError(f"required empty workspace directory is missing or a symlink: {name}")
        if any(path.rglob("*")):
            raise InputError(f"workspace directory is not empty: {name}")
        empty_dirs.append(name)

    request = {
        "state": "prepared_not_submitted",
        "batch": {
            "name": "Emote image categorization v1",
            "description": "First-pass visual classification for manifest unique image groups 00-120.",
            "config": {
                "max_attempts": 3,
                "require_progress": True,
                "agent_lease_budget_seconds": 3600,
                "action_profile": "emote-publisher-v1",
                "validator": {
                    "required": [
                        "schema_version",
                        "group_id",
                        "agent_id",
                        "attempt_no",
                        "source_image_ids",
                        "characters",
                        "uncertain",
                    ],
                    "timeout_ms": 1000,
                    "max_error_attempts": 3,
                },
            },
        },
        "tasks": tasks,
    }
    summary = {
        "manifest_rows": len(manifest),
        "unique_images": len(unique),
        "duplicate_mappings": len(duplicates),
        "groups": len(tasks),
        "task_image_count": sum(EXPECTED_GROUPS.values()),
        "empty_directories_verified": empty_dirs,
    }
    return request, summary


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument(
        "--write-request",
        type=Path,
        help="write the prepared JSON request outside the source tree; never submits it to coord",
    )
    args = parser.parse_args()
    try:
        root = args.source_root.resolve(strict=True)
        if not root.is_dir():
            raise InputError("source root must be a directory")
        request, summary = validate_inventory(root)
        encoded = json.dumps(request, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")
        if args.write_request:
            output = args.write_request.resolve(strict=False)
            if output.is_relative_to(root):
                raise InputError("request output must be outside the read-only source tree")
            if output.exists():
                raise InputError("request output already exists; refusing to overwrite")
            output.parent.mkdir(parents=True, exist_ok=True)
            with output.open("xb") as stream:
                stream.write(encoded)
            state = "request_written_not_submitted"
        else:
            state = "dry_run"
        print(
            json.dumps(
                {
                    "state": state,
                    "summary": summary,
                    "request_sha256": hashlib.sha256(encoded).hexdigest(),
                    "request": request,
                },
                ensure_ascii=False,
                indent=2,
            )
        )
        return 0
    except (InputError, OSError, json.JSONDecodeError) as error:
        print(json.dumps({"state": "rejected", "error": str(error)}, ensure_ascii=False), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())

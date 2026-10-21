#!/usr/bin/env python3
"""Create a non-overwriting SQLite online backup and verify its contents."""

import argparse
import hashlib
import json
import os
import sqlite3
from pathlib import Path
from urllib.parse import quote


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--destination", required=True, type=Path)
    parser.add_argument("--verify-only", action="store_true")
    args = parser.parse_args()

    source = args.source.resolve(strict=True)
    destination = args.destination.absolute()
    if args.verify_only and not destination.is_file():
        parser.error("verification destination does not exist")
    if not args.verify_only and (destination.exists() or destination.is_symlink()):
        parser.error("destination already exists")
    if not destination.parent.is_dir():
        parser.error("destination parent directory does not exist")

    source_uri = "file:" + quote(source.as_posix(), safe="/:") + "?mode=ro"
    destination_uri = "file:" + quote(destination.as_posix(), safe="/:") + "?mode=ro"
    source_db = sqlite3.connect(source_uri, uri=True, timeout=10)
    destination_db = (
        sqlite3.connect(destination_uri, uri=True, timeout=10)
        if args.verify_only
        else sqlite3.connect(destination, timeout=10)
    )
    try:
        if not args.verify_only:
            source_db.backup(destination_db, sleep=0.1)
            destination_db.commit()
        quick_check, quick_check_reason = "NOT_RUN", ""
        try:
            quick_check = destination_db.execute("PRAGMA quick_check").fetchone()[0]
            if quick_check != "ok":
                raise RuntimeError("backup integrity check failed")
        except sqlite3.OperationalError as error:
            if "no such collation sequence" not in str(error):
                raise
            quick_check_reason = "custom_collation_unavailable"
        source_counts = row_counts(source_db)
        destination_counts = row_counts(destination_db)
        if source_counts != destination_counts:
            raise RuntimeError("source and destination table row counts differ")
    finally:
        destination_db.close()
        source_db.close()

    digest = hashlib.sha256(destination.read_bytes()).hexdigest()
    print(
        json.dumps(
            {
                "quick_check": quick_check,
                "quick_check_reason": quick_check_reason,
                "user_table_count": len(source_counts),
                "row_counts_match": True,
                "size_bytes": os.path.getsize(destination),
                "sha256": digest,
            },
            separators=(",", ":"),
        )
    )
    return 0


def row_counts(connection: sqlite3.Connection) -> dict[str, int]:
    names = connection.execute(
        "SELECT name FROM sqlite_master "
        "WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
    ).fetchall()
    counts = {}
    for (name,) in names:
        quoted_name = '"' + name.replace('"', '""') + '"'
        counts[name] = connection.execute(
            f"SELECT count(*) FROM {quoted_name} NOT INDEXED"
        ).fetchone()[0]
    return counts


if __name__ == "__main__":
    raise SystemExit(main())

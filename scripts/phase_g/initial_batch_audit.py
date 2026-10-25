"""Read-only site preflight. Run as administrator on Arasaka, never print keys."""
import hashlib
import json
from pathlib import Path
import sqlite3
import subprocess

base = Path('C:/ProgramData/coord/g7')
db = sqlite3.connect('file:C:/ProgramData/coord/data/coord.db?mode=ro', uri=True)
db.row_factory = sqlite3.Row
assert db.execute('PRAGMA quick_check').fetchone()[0] == 'ok'
policy = json.loads((base / 'relay-private/identity.json').read_text(encoding='utf-8-sig'))
registry = json.loads((base / 'config/registry-production.json').read_text(encoding='utf-8-sig'))
tables = [r[0] for r in db.execute("SELECT name FROM sqlite_master WHERE type='table'")]
report = {
    'state': 'PASS', 'quick_check': 'ok', 'tables': tables,
    'batches': [dict(r) for r in db.execute('SELECT * FROM batches')],
    'task_counts': [dict(r) for r in db.execute('SELECT batch_id,state,count(*) AS n FROM tasks GROUP BY batch_id,state')],
    'pending_high': [dict(r) for r in db.execute("SELECT id,name,priority FROM tasks WHERE state='pending' AND priority IN ('urgent','high')")],
    'policy': {k: v for k, v in policy.items() if k != 'signing_key_file'},
    'policy_sha256': hashlib.sha256((base / 'relay-private/identity.json').read_bytes()).hexdigest(),
    'registry': registry,
    'registry_sha256': hashlib.sha256((base / 'config/registry-production.json').read_bytes()).hexdigest(),
    'formal_directory_exists': (base / 'initial').exists(),
}
print(json.dumps(report))

"""One-site backup before formal queue preparation. No live DB restore."""
import datetime
import hashlib
import json
from pathlib import Path
import secrets
import shutil
import sqlite3
import subprocess

base = Path('C:/ProgramData/coord/g7')
pointer = base / 'private/initial-operation.json'
assert not pointer.exists(), 'Existing operation must be reconciled; never recreate'
assert not (base / 'initial').exists()
stamp = datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ')
backup = base / 'private' / ('initial-' + stamp + '-' + secrets.token_hex(4))
backup.mkdir()
assert shutil.disk_usage(base).free > 1024**3
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
for src, name in [(base / 'relay-private/identity.json', 'identity-before.json'), (base / 'config/registry-production.json', 'registry-before.json')]:
    shutil.copy2(src, backup / name)
    assert sha(src) == sha(backup / name)
for name in ['coord.exe', 'coord-action-runner.exe', 'coord-emote-action.exe']:
    shutil.copy2(base / 'bin' / name, backup / name)
db = sqlite3.connect('file:C:/ProgramData/coord/data/coord.db?mode=ro', uri=True)
with sqlite3.connect(backup / 'coord.db') as dest:
    db.backup(dest)
    assert dest.execute('PRAGMA quick_check').fetchone()[0] == 'ok'
    assert dest.execute('SELECT count(*) FROM tasks').fetchone()[0] == 22
assert db.execute("SELECT count(*) FROM tasks WHERE state IN ('claimed','submitted')").fetchone()[0] == 0
for name in ['coord', 'CoordActionRunnerG7', 'CoordRelayAgent']:
    subprocess.run(['C:/Program Files/Servy/servy-cli.exe', 'export', '--name', name, '--config', 'json', '--path', str(backup / (name + '.json')), '--quiet'], check=True, capture_output=True)
code = "@{services=@(Get-CimInstance Win32_Service|Select-Object Name,State,StartName,StartMode,ProcessId);health=(Get-Content -Raw C:/ProgramData/coord/health/status.json|ConvertFrom-Json)}|ConvertTo-Json -Depth 8"
r = subprocess.run(['pwsh.exe', '-NoProfile', '-Command', code], check=True, capture_output=True, text=True)
(backup / 'baseline.json').write_text(r.stdout, encoding='utf-8')
state = {'state': 'BACKED_UP', 'backup_root': str(backup), 'business_root': str(base / 'initial/business'), 'inbound_package': str(backup / 'inputs.zip'), 'db_sha256': sha(backup / 'coord.db'), 'prior_task_count': 22, 'rollback_condition': 'Disable/remove only formal binding first. Preserve live DB, accepted rows, receipts and outputs; no automatic DB restore. Restore prior registry only after reconciling formal action states.'}
pointer.write_text(json.dumps(state, indent=2))
(backup / 'backup-manifest.json').write_text(json.dumps(state, indent=2))
print(json.dumps(dict(state, state='PASS')))

"""Read-only final hashes and rollback guard check, after all service reloads."""
import hashlib
import json
from pathlib import Path
import sqlite3

base=Path('C:/ProgramData/coord/g7')
state=json.loads((base/'private/initial-operation.json').read_text())
business=base/'initial/business'
manifest=json.loads((business/'deployment-manifest.json').read_text(encoding='utf-8'))
for item in manifest['files']:
    p=business/item['path']
    assert p.is_file() and hashlib.sha256(p.read_bytes()).hexdigest()==item['sha256']
db=sqlite3.connect('file:C:/ProgramData/coord/data/coord.db?mode=ro',uri=True)
assert db.execute('SELECT count(*) FROM tasks WHERE batch_id=? AND (state!=? OR attempt_no!=0 OR lease_token IS NOT NULL)',(state['batch_id'],'pending')).fetchone()[0]==0
assert db.execute('SELECT count(*) FROM completion_actions WHERE batch_id=?',(state['batch_id'],)).fetchone()[0]==0
assert not list((business/'results').rglob('*.json'))
print(json.dumps({'state':'PASS','batch_id':state['batch_id'],'files_rehashed':len(manifest['files']),'formal_results':0,'formal_action_events':0,'rollback_guards':{'all_tasks_unclaimed_pending':True,'no_action_or_publication_to_reconcile':True,'preserve_live_database':True,'expire_binding_in_place_to_keep_batch_protected':True},'backup_root':state['backup_root']}))

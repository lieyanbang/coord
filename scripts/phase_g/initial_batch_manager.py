"""Exact-site manager create/add/readback. No claim/submit/renew RPC exists here."""
import hashlib
import json
from pathlib import Path
import sqlite3
import urllib.request

base = Path('C:/ProgramData/coord/g7')
pointer = base / 'private/initial-operation.json'
state = json.loads(pointer.read_text(encoding='utf-8-sig'))
request = json.loads((base / 'initial/business/prepared-request.json').read_text(encoding='utf-8'))
key = (base / 'daemon-private/management.key').read_text().strip()
def rpc(method, params):
    assert method in ['batches/create', 'batches/add_tasks', 'batches/status', 'actions/status']
    body = json.dumps({'jsonrpc':'2.0', 'id':1, 'method':method, 'params':params}).encode()
    req = urllib.request.Request('http://127.0.0.1:7777/', data=body, headers={'Content-Type':'application/json', 'Authorization':'Bearer ' + key})
    data = json.load(urllib.request.urlopen(req, timeout=15))
    assert 'error' not in data, 'Manager RPC rejected ' + method
    return data['result']
def connect(p):
    db = sqlite3.connect('file:' + str(p).replace('\\','/') + '?mode=ro', uri=True)
    db.row_factory = sqlite3.Row
    return db
db = connect('C:/ProgramData/coord/data/coord.db')
prior = connect(Path(state['backup_root']) / 'coord.db')
def preserve():
    # All prior task/fence/progress/receipt/action/batch records, not merely counts.
    result = {}
    for table in ['tasks', 'batches', 'task_receipts', 'task_progress', 'task_progress_receipts', 'task_progress_extend_receipts', 'completion_actions']:
        rows = prior.execute('SELECT * FROM ' + table).fetchall()
        pks = [r['name'] for r in prior.execute('PRAGMA table_info(' + table + ')') if r['pk']]
        assert pks
        for row in rows:
            query = 'SELECT * FROM ' + table + ' WHERE ' + ' AND '.join('"' + k + '"=?' for k in pks)
            now = db.execute(query, [row[k] for k in pks]).fetchone()
            assert now is not None and tuple(now) == tuple(row), 'Prior record changed in ' + table
        result[table] = len(rows)
    assert db.execute('PRAGMA quick_check').fetchone()[0] == 'ok'
    return result
preserved = preserve()
stage = globals().get('_INITIAL_STAGE', 'verify')
if stage == 'create':
    assert state['state'] == 'PROFILE_INSTALLED' and not state.get('batch_id')
    assert not db.execute('SELECT id FROM batches WHERE name=?', (request['batch']['name'],)).fetchall(), 'Existing name: reconcile, never repeat create'
    # An actual manager-issued UUID is needed before identity can be bound.
    batch = rpc('batches/create', request['batch'])
    state.update(state='EMPTY_BATCH_CREATED', batch_id=batch['id'])
    pointer.write_text(json.dumps(state, indent=2))
    assert rpc('batches/status', {'batch_id':batch['id']})['total'] == 0
elif stage == 'add':
    assert state['state'] == 'ALL_HOPS_BOUND'
    bid = state['batch_id']
    assert rpc('batches/status', {'batch_id':bid})['total'] == 0
    added = rpc('batches/add_tasks', {'batch_id':bid, 'tasks':request['tasks']})
    # Reconcile exact stored payloads before testing documented add idempotency.
    rows = db.execute('SELECT task_key,payload FROM tasks WHERE batch_id=?', (bid,)).fetchall()
    assert {r['task_key']:json.loads(r['payload']) for r in rows} == {t['key']:t['payload'] for t in request['tasks']}
    replay = rpc('batches/add_tasks', {'batch_id':bid, 'tasks':request['tasks']})
    state.update(state='TASKS_ADDED', add_result=added, idempotent_readd_result=replay)
    pointer.write_text(json.dumps(state, indent=2))
elif stage != 'verify':
    raise ValueError('Unknown stage')
bid = state['batch_id']
progress = rpc('batches/status', {'batch_id':bid})
actions = rpc('actions/status', {'batch_id':bid})
batch_row = db.execute('SELECT * FROM batches WHERE id=?',(bid,)).fetchone()
config = json.loads(batch_row['config'])
assert config['max_attempts'] == 3 and config['agent_lease_budget_seconds'] == 3600 and config['require_progress']
assert config['action_profile'] == 'emote-publisher-v1' and config['action_profile_version'] == 1
assert set(config['action_profile_actions']) == {'on_task_completed','on_batch_completed'}
assert not actions['actions']
tasks = db.execute('SELECT * FROM tasks WHERE batch_id=? ORDER BY task_key',(bid,)).fetchall()
assert all(r['state'] == 'pending' and r['attempt_no'] == 0 and r['attempt_count'] == 0 and r['claimed_by'] is None and r['lease_token'] is None for r in tasks)
if stage != 'create':
    assert len(tasks) == len({r['task_key'] for r in tasks}) == 121
    assert {r['task_key'] for r in tasks} == {t['key'] for t in request['tasks']}
    image_ids = [i for r in tasks for i in json.loads(r['payload'])['source_image_ids']]
    assert len(image_ids) == len(set(image_ids)) == 1206
    assert progress['total'] == progress['states']['pending'] == 121 and progress['states']['claimed'] == progress['states']['submitted'] == 0
    assert progress['active'] == 121  # Deployed API defines active as nonterminal, including pending.
    # active means outstanding tasks in this scheduler; record leases separately below.
    assert all(r['max_attempts'] == 3 for r in tasks)
preserved = preserve()
report = {'state':'PASS', 'stage':stage, 'batch_id':bid, 'progress':progress, 'task_count':len(tasks), 'leased_or_submitted':sum(r['state'] in ['claimed','submitted'] for r in tasks), 'task_unique_keys':len({r['task_key'] for r in tasks}), 'image_coverage':0 if not tasks else len(image_ids), 'prior_records_unchanged':preserved, 'config':config, 'completion_action_count':len(actions['actions']), 'backup_root':state['backup_root'], 'claim_invocations':0, 'tokens_exported':False}
(Path(state['backup_root']) / ('manager-' + stage + '.json')).write_text(json.dumps(report, indent=2))
print(json.dumps(report))

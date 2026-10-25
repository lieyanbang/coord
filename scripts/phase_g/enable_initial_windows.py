"""Append one exact 12-hour binding using the Windows server's current UTC."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

base = Path('C:/ProgramData/coord/g7')
pointer = base / 'private/initial-operation.json'
state = json.loads(pointer.read_text(encoding='utf-8-sig'))
assert state['state'] == 'EMPTY_BATCH_CREATED'
path = base / 'relay-private/identity.json'
old = path.read_bytes()
backup = Path(state['backup_root'])
assert old == (backup / 'identity-before.json').read_bytes()
policy = json.loads(old)
assert policy['version'] == 2 and policy['mode'] == 'logical' and policy['enabled']
assert not any(b['batch_id'] == state['batch_id'] or b['id'] == 'emote-initial-v1' for b in policy['bindings'])
now = time.time_ns() // 1_000_000
binding = {'id':'emote-initial-v1', 'client_sha256':'2e12506a52763d439506792019e34debb54b912d2575c82c3ef275af5cd01ab1', 'batch_id':state['batch_id'], 'worker_ids':[f'{n:04X}' for n in range(6)], 'scopes':['coord:read','coord:claim','coord:progress','coord:extend','coord:submit','coord:failure'], 'not_before':now, 'expires_at':now+43_200_000}
policy['bindings'].append(binding)
candidate = path.with_suffix('.json.initial-new')
with candidate.open('x', encoding='utf-8') as f:
    json.dump(policy, f, indent=2)
node = json.loads((backup / 'CoordRelayAgent.json').read_text(encoding='utf-8-sig'))['ExecutablePath']
check = "const m=require('C:/ProgramData/coord-relay-agent/identity.cjs');(async()=>{const a=await m.loadPolicy('C:/ProgramData/coord/g7/relay-private/identity.json');const b=await m.loadPolicy('C:/ProgramData/coord/g7/relay-private/identity.json.initial-new');if(a.key!==b.key||JSON.stringify(a.bindings)!==JSON.stringify(b.bindings.slice(0,-1))||JSON.stringify(a.legacy_client_sha256)!==JSON.stringify(b.legacy_client_sha256))throw Error('Changed prior policy');})().catch(()=>process.exit(1));"
subprocess.run([node,'-e',check], check=True, capture_output=True)
try:
    os.replace(candidate, path)
    subprocess.run(['pwsh.exe','-NoProfile','-Command','Restart-Service CoordRelayAgent; (Get-Service CoordRelayAgent).WaitForStatus(\'Running\',[timespan]::FromSeconds(25))'], check=True, capture_output=True)
except Exception:
    path.write_bytes(old)
    subprocess.run(['pwsh.exe','-NoProfile','-Command','Restart-Service CoordRelayAgent'], check=True, capture_output=True)
    raise RuntimeError('Windows binding failed; prior policy restored; stop') from None
def utc(ms):return datetime.datetime.fromtimestamp(ms/1000,datetime.timezone.utc).isoformat(timespec='milliseconds').replace('+00:00','Z')
state.update(state='WINDOWS_BOUND', binding=binding, not_before_utc=utc(now), expires_at_utc=utc(now+43_200_000))
pointer.write_text(json.dumps(state,indent=2))
report = {'state':'PASS','batch_id':state['batch_id'],'binding':binding,'not_before_utc':state['not_before_utc'],'expires_at_utc':state['expires_at_utc'],'clock_source':'Arasaka Windows server time.time_ns UTC at actual enable','duration_seconds':43200,'prior_bindings_unchanged':True,'policy_sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'backup_root':state['backup_root']}
(backup/'windows-binding.json').write_text(json.dumps(report,indent=2))
print(json.dumps(report))

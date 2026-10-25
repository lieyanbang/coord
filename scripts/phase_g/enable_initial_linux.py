"""Sync one public binding to the existing policy loaded by both Linux hops."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time
import urllib.request

binding = globals()['_BINDING']
assert binding['id'] == 'emote-initial-v1' and binding['worker_ids'] == [f'{n:04X}' for n in range(6)]
assert binding['expires_at']-binding['not_before'] == 43_200_000
assert binding['not_before'] <= time.time()*1000 < binding['expires_at']
backup = Path('/var/backups/coord-initial-20261025T075307Z-18128120')
path = Path('/etc/coord-relay/identity.json')
old = path.read_bytes()
assert old == (backup / 'identity-before.json').read_bytes()
oldstat = path.stat()
policy = json.loads(old)
assert not any(b['id'] == binding['id'] or b['batch_id'] == binding['batch_id'] for b in policy['bindings'])
policy['bindings'].append(binding)
candidate = path.with_suffix('.json.initial-new')
with candidate.open('x') as f:
    json.dump(policy,f,indent=2);f.flush();os.fsync(f.fileno())
os.chmod(candidate,oldstat.st_mode & 0o777);os.chown(candidate,oldstat.st_uid,oldstat.st_gid)
check = "const m=require('/opt/coord-relay/identity.cjs');(async()=>{const a=await m.loadPolicy('/etc/coord-relay/identity.json');const b=await m.loadPolicy('/etc/coord-relay/identity.json.initial-new');if(a.key!==b.key||JSON.stringify(a.bindings)!==JSON.stringify(b.bindings.slice(0,-1))||JSON.stringify(a.legacy_client_sha256)!==JSON.stringify(b.legacy_client_sha256))throw Error('Prior state changed');})().catch(()=>process.exit(1));"
subprocess.run(['node','-e',check],check=True,capture_output=True)
try:
    os.replace(candidate,path)
    for unit in ['coord-relay.service','feathersjsapp.service']:
        subprocess.run(['systemctl','restart',unit],check=True,capture_output=True)
    health = None
    for _ in range(100):
        try:
            health=json.load(urllib.request.urlopen('http://127.0.0.1:13013/healthz',timeout=2))
            if health.get('state') == 'READY':break
        except Exception:pass
        time.sleep(.2)
    assert health and health['state']=='READY'
except Exception:
    path.write_bytes(old);os.chmod(path,oldstat.st_mode & 0o777);os.chown(path,oldstat.st_uid,oldstat.st_gid)
    for unit in ['coord-relay.service','feathersjsapp.service']:
        subprocess.run(['systemctl','restart',unit],check=True,capture_output=True)
    raise RuntimeError('Linux binding failed; prior policy restored; stop') from None
report={'state':'PASS','batch_id':binding['batch_id'],'binding':binding,'backup_root':str(backup),'both_consumers_policy':str(path),'prior_bindings_unchanged':True,'oauth_store_modified':False,'health':health,'policy_sha256':hashlib.sha256(path.read_bytes()).hexdigest()}
(backup/'binding-result.json').write_text(json.dumps(report,indent=2))
print(json.dumps(report))

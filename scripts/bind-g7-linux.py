"""Apply the public, manager-created canary batch bindings to both Linux hops."""
import json
import os
from pathlib import Path
import subprocess
import time
import urllib.request

source = Path(os.environ['COORD_G7_BINDINGS'])
bindings = json.loads(source.read_text())
assert bindings['state'] == 'EMPTY_BATCHES_BOUND' and bindings['task_count'] == 0
policy_path = Path('/etc/coord-relay/identity.json')
policy = json.loads(policy_path.read_text())
assert policy['version'] == 2 and policy['mode'] == 'logical'
assert policy['bindings'] == [], 'Do not replace existing live bindings'
policy['bindings'] = bindings['bindings']
temp = policy_path.with_suffix('.json.g7-bind')
with temp.open('x') as f:
    json.dump(policy, f, indent=2)
os.chmod(temp, 0o640)
os.chown(temp, 0, policy_path.stat().st_gid)
os.replace(temp, policy_path)
subprocess.run(['node', '-e', "require('/opt/coord-relay/identity.cjs').loadPolicy('/etc/coord-relay/identity.json').then(p=>{if(p.bindings.length!==2)process.exit(1)}).catch(()=>process.exit(1))"], check=True)
for unit in ['coord-relay.service', 'feathersjsapp.service']:
    subprocess.run(['systemctl', 'restart', unit], check=True)
config = json.loads(Path('/etc/coord-relay/config.json').read_text())
health = None
for _ in range(100):
    try:
        health = json.load(urllib.request.urlopen('http://127.0.0.1:' + str(config['clientPort']) + '/healthz', timeout=2))
        if health.get('state') == 'READY':
            break
    except Exception:
        pass
    time.sleep(.2)
assert health and health['state'] == 'READY'
print(json.dumps({'state': 'PASS', 'bindings': len(policy['bindings']), 'health': health, 'task_creation': 'not performed by this helper'}))

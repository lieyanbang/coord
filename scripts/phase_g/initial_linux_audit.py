"""Read-only inspection of both loaded Linux policy consumers."""
import hashlib
import json
from pathlib import Path
import subprocess
import urllib.request

p = Path('/etc/coord-relay/identity.json')
policy = json.loads(p.read_text())
store = json.loads(Path('/var/lib/coord-mcp/oauth.json').read_text())
digest = '2e12506a52763d439506792019e34debb54b912d2575c82c3ef275af5cd01ab1'
clients = [k for k in store['clients'] if hashlib.sha256(k.encode()).hexdigest() == digest]
assert len(clients) == 1
assert any(v.get('clientId') == clients[0] for v in store['accessTokens'].values())
units = {u: subprocess.check_output(['systemctl', 'show', u, '-p', 'ActiveState', '-p', 'MainPID', '-p', 'Environment'], text=True) for u in ['coord-relay.service', 'feathersjsapp.service']}
# Environment values may contain secrets; expose only identity policy path matches.
safe_units = {u: {'active': 'ActiveState=active' in s, 'uses_policy_path': '/etc/coord-relay/identity.json' in s, 'pid': next(x for x in s.splitlines() if x.startswith('MainPID='))} for u, s in units.items()}
print(json.dumps({'state': 'PASS', 'policy': {k:v for k,v in policy.items() if k != 'signing_key_file'}, 'policy_sha256': hashlib.sha256(p.read_bytes()).hexdigest(), 'units': safe_units, 'health': json.load(urllib.request.urlopen('http://127.0.0.1:13013/healthz', timeout=5)), 'approved_client_registered': True, 'signing_key_mode': oct(Path(policy['signing_key_file']).stat().st_mode & 0o777)}))

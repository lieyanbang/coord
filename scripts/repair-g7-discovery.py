"""Exact approved reconnect binding repair; no OAuth grant/store mutation."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import secrets
import subprocess
import time
from urllib.parse import urlsplit
import urllib.request

client = '2e12506a52763d439506792019e34debb54b912d2575c82c3ef275af5cd01ab1'
batch = 'd2313c78-22da-4cb8-bcd7-ece9897e0db1'
path = Path('/etc/coord-relay/identity.json')
old_bytes = path.read_bytes()
old_stat = path.stat()
policy = json.loads(old_bytes)
assert policy['version'] == 2 and policy['mode'] == 'logical' and policy['enabled']
assert not any(b['id'] == 'g7-browser-reconnect' for b in policy['bindings'])
store = json.loads(Path('/var/lib/coord-mcp/oauth.json').read_text())
registered = [(k, v) for k, v in store['clients'].items() if hashlib.sha256(k.encode()).hexdigest() == client]
assert len(registered) == 1 and registered[0][1]['client_name'] == 'ChatGPT'
assert all(urlsplit(u).hostname == 'chatgpt.com' for u in registered[0][1]['redirect_uris'])
assert any(v.get('clientId') == registered[0][0] for v in store['accessTokens'].values())
original = next(b for b in policy['bindings'] if b['id'] == 'g7-browser' and b['batch_id'] == batch)
assert original['expires_at'] > time.time() * 1000
added = dict(original, id='g7-browser-reconnect', client_sha256=client, worker_ids=['0000', '0001'])
policy['bindings'].append(added)
backup = Path('/var/backups/coord-g7-20261025T050727Z-01d4e8f7') / ('reconnect-' + datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ') + '-' + secrets.token_hex(4))
backup.mkdir(mode=0o700)
(backup / 'identity-before.json').write_bytes(old_bytes)
os.chmod(backup / 'identity-before.json', 0o600)
assert (backup / 'identity-before.json').read_bytes() == old_bytes
temp = path.with_name(path.name + '.reconnect-new')
with temp.open('x') as f:
    json.dump(policy, f, indent=2)
    f.flush()
    os.fsync(f.fileno())
os.chmod(temp, old_stat.st_mode & 0o777)
os.chown(temp, old_stat.st_uid, old_stat.st_gid)
check = """const m=require('/opt/coord-relay/identity.cjs');(async()=>{const a=await m.loadPolicy('/etc/coord-relay/identity.json');const b=await m.loadPolicy('/etc/coord-relay/identity.json.reconnect-new');if(a.key!==b.key||JSON.stringify(a.legacy_client_sha256)!==JSON.stringify(b.legacy_client_sha256)||JSON.stringify(a.bindings)!==JSON.stringify(b.bindings.slice(0,-1)))throw Error('Unexpected change');})().catch(()=>process.exit(1));"""
subprocess.run(['node', '-e', check], check=True)
report = {'state': 'FAIL', 'backup_root': str(backup), 'before_sha256': hashlib.sha256(old_bytes).hexdigest(), 'added_binding': added, 'oauth_store_modified': False, 'legacy_allowlist_unchanged': True, 'original_bindings_retained': True}
try:
    os.replace(temp, path)
    for unit in ['coord-relay.service', 'feathersjsapp.service']:
        subprocess.run(['systemctl', 'restart', unit], check=True)
    health = None
    for _ in range(100):
        try:
            health = json.load(urllib.request.urlopen('http://127.0.0.1:13013/healthz', timeout=2))
            if health.get('state') == 'READY':
                break
        except Exception:
            pass
        time.sleep(.2)
    assert health and health['state'] == 'READY'
    report.update(state='PASS', after_sha256=hashlib.sha256(path.read_bytes()).hexdigest(), health=health)
except Exception as exc:
    path.write_bytes(old_bytes)
    os.chmod(path, old_stat.st_mode & 0o777)
    os.chown(path, old_stat.st_uid, old_stat.st_gid)
    for unit in ['coord-relay.service', 'feathersjsapp.service']:
        subprocess.run(['systemctl', 'restart', unit], check=True)
    report.update(failure=str(exc), rollback='prior policy restored; only affected services restarted')
finally:
    (backup / 'result.json').write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2))
if report['state'] != 'PASS':
    raise SystemExit(1)

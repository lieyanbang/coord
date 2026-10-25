"""Run as the operator through direct SSH after the actual Windows site gates.

Set COORD_G7_UPLOAD to the exact owner-only uploaded payload directory. Secrets
are installed directly from it and never returned. OAuth grants stay live.
"""
import datetime
import grp
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import time
import urllib.request

upload = Path(os.environ['COORD_G7_UPLOAD']).resolve()
assert upload.parent == Path('/home/ubuntu') and upload.name.startswith('coord-g7-upload-')
backup = Path('/var/backups/coord-g7-20261025T050727Z-01d4e8f7')
archive = backup / 'configuration.tar.gz'
sha = lambda p: hashlib.sha256(Path(p).read_bytes()).hexdigest()
assert sha(archive) == '6fa0c6af1782f4317ddf77c392a74835d2e3b43a6a8fbe65dbf22cb7274f9bcb'
facade = Path('/opt/app/feathers/realtimetext/lib')
oauth = facade / 'coord-mcp/oauth.cjs'
assert sha(oauth) == '0dcc483bd3d6afd5f4625885aed70f5cdf1b30f6a44ced739549d382ce049c53'
assert sha(facade / 'coord-mcp/index.cjs') == 'e2a4c125f6742f915662707ff123f3eb0a3e3ec6b64ae2818baf1c3ea7cd5db3'
manifest = json.loads((upload / 'sources.json').read_text())
for name, digest in manifest.items():
    assert sha(upload / name) == digest, 'Uploaded source hash mismatch'
    subprocess.run(['node', '--check', str(upload / name)], check=True, capture_output=True)
key = (upload / 'identity.key').read_text().strip()
assert len(key) == 64 and all(c in '0123456789abcdef' for c in key)
private = backup / 'deployment'
private.mkdir(mode=0o700, exist_ok=True)
report = {'state': 'FAIL', 'started_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
          'source_commit': '229d423b003a88bf2d49cca96502dfb803a482f1',
          'backup_root': str(backup), 'oauth_module_unchanged': True,
          'access_policy_changed': False, 'files': manifest}
config_path = Path('/etc/coord-relay/config.json')
dropin = Path('/etc/systemd/system/feathersjsapp.service.d/coord-g7.conf')
assert not dropin.exists(), 'Do not overwrite an existing unit drop-in'
original_config = config_path.read_bytes()
originals = {p: p.read_bytes() if p.exists() else None for p in
             [Path('/opt/coord-relay') / n for n in ['server.mjs', 'protocol.mjs', 'identity.cjs']]
             + [facade / 'coord-mcp/index.cjs', facade / 'identity.cjs']}
gid = grp.getgrnam('coord-relay').gr_gid

def install(data, path, mode=0o644, group=0):
    path = Path(path)
    temp = path.with_name(path.name + '.g7-new')
    with temp.open('xb') as f:
        f.write(data)
        f.flush()
        os.fsync(f.fileno())
    os.chmod(temp, mode)
    os.chown(temp, 0, group)
    os.replace(temp, path)

try:
    for p in originals:
        install((upload / ('index.cjs' if p.name == 'index.cjs' else p.name)).read_bytes(), p)
    install((key + '\n').encode(), '/etc/coord-relay/identity.key', 0o640, gid)
    policy = {'version': 2, 'mode': 'logical', 'enabled': True,
              'signing_key_file': '/etc/coord-relay/identity.key',
              'legacy_client_sha256': ['eeb595ffaeec3cc157243f766c320f4133f1d9a6af05ad21ee8fb373850122ba'],
              'bindings': []}
    install(json.dumps(policy).encode(), '/etc/coord-relay/identity.json', 0o640, gid)
    config = json.loads(original_config)
    config['identityPolicyFile'] = '/etc/coord-relay/identity.json'
    install(json.dumps(config, indent=2).encode(), config_path, 0o640, gid)
    dropin.parent.mkdir(exist_ok=True)
    install(b'[Service]\nEnvironment=COORD_MCP_IDENTITY_POLICY=/etc/coord-relay/identity.json\n', dropin)
    subprocess.run(['systemctl', 'daemon-reload'], check=True)
    for unit in ['coord-relay.service', 'feathersjsapp.service']:
        subprocess.run(['systemctl', 'restart', unit], check=True)
    health = None
    for _ in range(100):
        try:
            health = json.load(urllib.request.urlopen('http://127.0.0.1:' + str(config.get('clientPort', 13013)) + '/healthz', timeout=2))
            if health.get('state') == 'READY' and health.get('agentConnected'):
                break
        except Exception:
            pass
        time.sleep(.3)
    assert health and health.get('state') == 'READY' and health.get('agentConnected'), 'Sidecar not READY'
    assert sha(oauth) == '0dcc483bd3d6afd5f4625885aed70f5cdf1b30f6a44ced739549d382ce049c53'
    report.update(state='PASS', health=health,
                  services={u: subprocess.check_output(['systemctl', 'is-active', u], text=True).strip()
                            for u in ['coord-relay.service', 'feathersjsapp.service', 'openresty.service', 'coord-relay-acl.service']},
                  listeners=subprocess.check_output(['ss', '-lnt', '( sport = :13011 or sport = :13012 )'], text=True).strip())
except Exception as exc:
    report['failure'] = str(exc)
    try:
        for p, data in originals.items():
            if data is None:
                p.unlink(missing_ok=True)
            else:
                install(data, p)
        install(original_config, config_path, 0o640, gid)
        dropin.unlink(missing_ok=True)
        subprocess.run(['systemctl', 'daemon-reload'], check=True)
        for unit in ['coord-relay.service', 'feathersjsapp.service']:
            subprocess.run(['systemctl', 'restart', unit], check=True)
        report['rollback'] = 'affected modules/config restored; live OAuth state preserved'
    except Exception as rollback:
        report['rollback'] = 'FAILED: ' + str(rollback)
finally:
    key = None
    report['finished_at'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    (private / 'linux-deployment.json').write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2))
if report['state'] != 'PASS':
    raise SystemExit(1)

"""Protect current Linux policy/modules/units; preserve active OAuth grants."""
import datetime
import hashlib
import json
from pathlib import Path
import secrets
import subprocess
import tarfile

root = Path('/var/backups') / ('coord-initial-' + datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ') + '-' + secrets.token_hex(4))
root.mkdir(mode=0o700)
items = ['etc/coord-relay', 'opt/coord-relay/identity.cjs', 'opt/app/feathers/realtimetext/lib/coord-mcp', 'etc/systemd/system/coord-relay.service', 'etc/systemd/system/feathersjsapp.service.d']
with tarfile.open(root / 'configuration.tar.gz', 'x:gz') as tar:
    for name in items:
        assert Path('/' + name).exists()
        tar.add('/' + name, arcname=name)
with tarfile.open(root / 'configuration.tar.gz') as tar:
    before = tar.extractfile('etc/coord-relay/identity.json').read()
    assert before == Path('/etc/coord-relay/identity.json').read_bytes()
(root / 'identity-before.json').write_bytes(before)
config = json.loads(Path('/etc/coord-relay/config.json').read_text())
assert config.get('identityPolicyFile') == '/etc/coord-relay/identity.json', 'Sidecar must load exact policy'
units = subprocess.check_output(['systemctl', 'show', 'feathersjsapp.service', '-p', 'Environment'], text=True)
assert 'COORD_MCP_IDENTITY_POLICY=/etc/coord-relay/identity.json' in units
report = {'state': 'PASS', 'backup_root': str(root), 'archive_sha256': hashlib.sha256((root / 'configuration.tar.gz').read_bytes()).hexdigest(), 'both_consumers_policy': '/etc/coord-relay/identity.json', 'oauth_store_modified': False}
(root / 'backup-manifest.json').write_text(json.dumps(report))
print(json.dumps(report))

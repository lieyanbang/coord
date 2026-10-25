"""Explicit component rollback; never restore old OAuth grants over live state."""
from pathlib import Path
import json
import os
import subprocess
import tarfile

backup = Path('/var/backups/coord-g7-20261025T050727Z-01d4e8f7')
targets = ['/opt/coord-relay/server.mjs', '/opt/coord-relay/protocol.mjs',
           '/opt/coord-relay/identity.cjs', '/etc/coord-relay/config.json',
           '/opt/app/feathers/realtimetext/lib/coord-mcp/index.cjs',
           '/opt/app/feathers/realtimetext/lib/identity.cjs']
with tarfile.open(backup / 'configuration.tar.gz') as archive:
    names = set(archive.getnames())
    for target in targets:
        path = Path(target)
        name = target.lstrip('/')
        if name not in names:
            # Only the two exact new identity modules can be absent in baseline.
            assert path.name == 'identity.cjs'
            path.unlink(missing_ok=True)
            continue
        member = archive.getmember(name)
        assert member.isfile()
        temp = path.with_name(path.name + '.g7-rollback')
        with temp.open('xb') as f:
            f.write(archive.extractfile(member).read())
            f.flush()
            os.fsync(f.fileno())
        os.chmod(temp, member.mode)
        os.chown(temp, member.uid, member.gid)
        os.replace(temp, path)
dropin = Path('/etc/systemd/system/feathersjsapp.service.d/coord-g7.conf')
if dropin.exists():
    assert dropin.read_text() == '[Service]\nEnvironment=COORD_MCP_IDENTITY_POLICY=/etc/coord-relay/identity.json\n'
    dropin.unlink()
subprocess.run(['systemctl', 'daemon-reload'], check=True)
for unit in ['coord-relay.service', 'feathersjsapp.service']:
    subprocess.run(['systemctl', 'restart', unit], check=True)
print(json.dumps({'state': 'CODE_RESTORED', 'oauth_state': 'preserved live', 'access_policy': 'unchanged'}))

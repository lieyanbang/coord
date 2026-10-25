"""Expand only the hash-pinned audited package into a fresh restricted root."""
import hashlib
import json
from pathlib import Path, PurePosixPath
import stat
import subprocess
import zipfile

base = Path('C:/ProgramData/coord/g7')
pointer = base / 'private/initial-operation.json'
state = json.loads(pointer.read_text())
assert state['state'] == 'BACKED_UP'
package = Path(state['inbound_package'])
assert hashlib.sha256(package.read_bytes()).hexdigest() == '815bdbe8380776f5397015090e818e6b8b541befe55a5c7c405472d3134d8adb'
target = base / 'initial'
assert not target.exists()
target.mkdir()
sid = 'S-1-5-21-3592388187-2353680085-815129118-1004'
def acl(p, write=False):
    subprocess.run(['icacls.exe', str(p), '/inheritance:r', '/grant:r', '*S-1-5-18:(OI)(CI)F', '*S-1-5-32-544:(OI)(CI)F', '*S-1-5-19:(OI)(CI)RX', '*' + sid + (':(OI)(CI)M' if write else ':(OI)(CI)RX')], check=True, capture_output=True)
acl(target)
business = target / 'business'
business.mkdir()
with zipfile.ZipFile(package) as z:
    names = z.namelist()
    assert len(names) == len(set(names)) == 1675
    for info in z.infolist():
        rel = PurePosixPath(info.filename)
        assert not rel.is_absolute() and all(x not in ('', '.', '..') for x in rel.parts)
        assert '\\' not in info.filename and ':' not in info.filename
        assert not stat.S_ISLNK(info.external_attr >> 16)
        dest = business.joinpath(*rel.parts)
        assert dest.resolve().is_relative_to(business.resolve())
        dest.parent.mkdir(parents=True, exist_ok=True)
        with dest.open('xb') as f:
            f.write(z.read(info))
manifest = json.loads((business / 'deployment-manifest.json').read_text(encoding='utf-8'))
assert len(manifest['files']) == 1673
for item in manifest['files']:
    p = business / item['path']
    assert not (p.lstat().st_file_attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT)
    assert p.stat().st_size == item['bytes'] and hashlib.sha256(p.read_bytes()).hexdigest() == item['sha256']
for name in ['results/.receipts', 'results/review_queue']:
    (business / name).mkdir(parents=True, exist_ok=True)
acl(business / 'results', True)
representatives = {}
for f in manifest['files']:
    representatives.setdefault(str(Path(f['path']).parent), str(business / f['path']))
probe = {
    'read_files': list(representatives.values()),
    'denied_read_files': ['C:/ProgramData/coord/data/coord.db', str(base / 'daemon-private/management.key'), str(base / 'relay-private/identity.key')],
    'denied_write_files': list(representatives.values()),
    'write_directories': [str(business / 'results'), str(business / 'results/.receipts'), str(business / 'results/review_queue')],
    'denied_write_directories': [str(business / 'task'), str(business / 'refs')],
    'denied_rpc_url': 'http://127.0.0.1:7777/',
    'broker_key_file': str(base / 'config/broker.key'),
}
# Probe field names match the native Runner; no executable or key bytes in this file.
(base / 'config/initial-acl-probe.json').write_text(json.dumps(probe))
assert (base / 'config/initial-acl-probe.json').stat().st_size < 65536
state.update(state='INPUTS_DEPLOYED', package_sha256=hashlib.sha256(package.read_bytes()).hexdigest(), deployed_files=1673, refs_files=464)
pointer.write_text(json.dumps(state, indent=2))
print(json.dumps({'state': 'PASS', 'backup_root': state['backup_root'], 'summary': manifest['summary'], 'deployed_files': 1673, 'sha256_verified_files': 1673, 'originals_modified': False}))

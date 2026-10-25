"""Package validated inputs only; preserve originals and pin every deployed file."""
import csv
import hashlib
import json
import stat
from pathlib import Path
import zipfile
from prepare_initial_tasks import validate_inventory

root = Path('D:/ChatGPT_Emote_Work')
out = Path('target/initial-emote')
request, summary = validate_inventory(root.resolve())
rows = list(csv.DictReader((root / 'image_manifest.csv').open(encoding='utf-8-sig')))
assert len({r['sha256'].upper() for r in rows if r['status'] == 'unique'}) == 1206
ids = [i for t in request['tasks'] for i in t['payload']['source_image_ids']]
assert len(ids) == len(set(ids)) == 1206
files = []
for directory in ['task', 'refs']:
    for p in (root / directory).rglob('*'):
        assert not p.is_symlink() and not (p.lstat().st_file_attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT)
        if p.is_file():
            assert p.resolve().is_relative_to(root.resolve())
            if directory == 'task':
                assert p.parent.name not in ['121', '122']
            files.append(p)
for name in ['image_manifest.csv', 'task_inventory.csv', 'exact_duplicates.csv']:
    p = root / name
    assert p.is_file() and not p.is_symlink()
    files.append(p)
manifest = {'version': 1, 'summary': summary, 'files': []}
package = out / 'inputs.zip'
assert not package.exists(), 'Never overwrite an existing package'
with zipfile.ZipFile(package, 'x', zipfile.ZIP_DEFLATED) as z:
    for p in sorted(files):
        data = p.read_bytes()
        name = p.relative_to(root).as_posix()
        manifest['files'].append({'path': name, 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()})
        z.writestr(name, data)
    z.writestr('deployment-manifest.json', json.dumps(manifest, ensure_ascii=False, indent=2))
    # Explicit operational claim duration; core hard cap is 600, not a BatchConfig field.
    for t in request['tasks']:
        t['payload'].update(input_root='C:/ProgramData/coord/g7/initial/business',
                            task_path='task/' + t['payload']['group_id'], refs_path='refs',
                            requested_lease_seconds=600)
    z.writestr('prepared-request.json', json.dumps(request, ensure_ascii=False))
digest = hashlib.sha256(package.read_bytes()).hexdigest()
report = {'state': 'PASS', 'summary': summary, 'file_count': len(files),
          'refs_files': sum(x['path'].startswith('refs/') for x in manifest['files']),
          'bytes': package.stat().st_size, 'sha256': digest, 'originals_modified': False}
(out / 'package.json').write_text(json.dumps(report, indent=2))
(out / 'deployment-manifest.json').write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding='utf-8')
print(json.dumps(report))

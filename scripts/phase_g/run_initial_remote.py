"""Direct SSH runner; Python payloads execute on the named host, no remote MCP."""
import argparse
import base64
import json
from pathlib import Path
import subprocess

p = argparse.ArgumentParser()
p.add_argument('host', choices=['windows', 'linux'])
p.add_argument('script', type=Path)
p.add_argument('output', type=Path)
p.add_argument('--stage')
p.add_argument('--binding-file', type=Path)
a = p.parse_args()
source = a.script.read_text(encoding='utf-8-sig')
if a.binding_file:
    source = '_BINDING=' + repr(json.loads(a.binding_file.read_text(encoding='utf-8-sig'))['binding']) + '\n' + source
if a.stage:
    source = (('$InitialStage=' + repr(a.stage) + '\n') if a.script.suffix == '.ps1' else ('_INITIAL_STAGE=' + repr(a.stage) + '\n')) + source
encoded = base64.b64encode(source.encode()).decode()
if a.host == 'windows':
    if a.script.suffix == '.py':
        source = "$ErrorActionPreference='Stop'; & 'C:/Users/lieyanbang/AppData/Local/Programs/Python/Python314/python.exe' -c \"import base64;exec(base64.b64decode('" + encoded + "'))\"; exit $LASTEXITCODE"
    command = ['ssh.exe', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=12', '-p', '13988', '-i', r'C:\Users\leiya\.ssh\arasaka.key', 'lieyanbang@arasaka.tailc57112.ts.net', 'pwsh.exe', '-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand', base64.b64encode(source.encode('utf-16le')).decode()]
else:
    command = ['ssh.exe', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=12', '-p', '13027', '-i', r'C:\Users\leiya\.ssh\oracle_amd_1g1c_0.key', 'ubuntu@instance-20260424-1840.tailc57112.ts.net', 'sudo -n python3 -c "import base64;exec(base64.b64decode(\'' + encoded + '\'))"']
r = subprocess.run(command, capture_output=True)
a.output.parent.mkdir(parents=True, exist_ok=True)
a.output.write_bytes(r.stdout)
if r.returncode:
    # Remote payloads must use sanitized failures; never echo command arguments.
    print(r.stderr.decode(errors='replace')[-3000:])
else:
    report = __import__('json').loads(r.stdout.decode('utf-8-sig'))
    print(__import__('json').dumps({k:report[k] for k in ['state', 'batch_id', 'backup_root', 'summary', 'not_before_utc', 'expires_at_utc'] if k in report}))
raise SystemExit(r.returncode)

"""Trusted JSON adapter fixture for worker acceptance."""
import json
import os
from pathlib import Path
import sys
import time

task = json.load(sys.stdin)
payload = task['payload']
if payload.get('pid_file'):
    Path(payload['pid_file']).write_text(str(os.getpid()))
time.sleep(payload.get('sleep', 0))
if payload.get('oversize'):
    print('x' * (2 * 1024 * 1024))
else:
    print(json.dumps(payload.get('result', payload)))

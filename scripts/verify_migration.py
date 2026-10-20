"""Real baseline database -> upgraded copy -> restored baseline acceptance."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import socket
import sqlite3
import subprocess
import tempfile
import time
import urllib.request


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


class Daemon:
    def __init__(self, binary, db, vault):
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        self.url = f'http://127.0.0.1:{port}/'
        self.log = (db.parent / (db.stem + '.log')).open('ab')
        self.process = subprocess.Popen([str(binary), 'serve', '--addr', f'127.0.0.1:{port}', '--db', str(db), '--vault', str(vault)], stdout=self.log, stderr=self.log)
        for _ in range(100):
            try:
                self.rpc('agents/list', {})
                break
            except OSError:
                time.sleep(.05)
        else:
            self.close()
            raise RuntimeError('readiness timeout')

    def request(self, method, params):
        request = urllib.request.Request(self.url, json.dumps(dict(jsonrpc='2.0', id=1, method=method, params=params)).encode(), {'Content-Type': 'application/json'})
        with urllib.request.urlopen(request, timeout=5) as response:
            return json.load(response)

    def rpc(self, method, params):
        value = self.request(method, params)
        assert 'error' not in value, value
        return value['result']

    def close(self):
        self.process.terminate()
        self.process.wait(timeout=10)
        self.log.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--old-bin', type=Path, required=True)
    parser.add_argument('--new-bin', type=Path, required=True)
    parser.add_argument('--report', type=Path, default=Path('docs/verification/stage-e-migration.json'))
    args = parser.parse_args()
    old_bin, new_bin = args.old_bin.resolve(), args.new_bin.resolve()
    root = Path(tempfile.mkdtemp(prefix='coord-migration-'))
    vault = root / 'vault'
    db = root / 'original.db'
    daemon = Daemon(old_bin, db, vault)
    try:
        daemon.rpc('agents/heartbeat', {'id': 'legacy-agent', 'name': 'legacy'})
        source = daemon.rpc('tasks/send', {'name': 'completed source', 'payload': {'nested': [1, 2]}})
        daemon.rpc('tasks/claim', {'id': source['id'], 'agentId': 'legacy-agent'})
        daemon.rpc('tasks/complete', {'id': source['id'], 'result': {'kept': True}, 'postAck': True, 'ackName': 'baseline ack'})
        daemon.rpc('tasks/send', {'name': 'pending source', 'payload': ['generic', 3]})
        active = daemon.rpc('tasks/send', {'name': 'legacy active lease'})
        daemon.rpc('tasks/claim', {'id': active['id'], 'agentId': 'legacy-agent'})
        before = {t['id']: t for t in daemon.rpc('tasks/list', {'limit': 100})}
        agents = daemon.rpc('agents/list', {})
        notes = {str(p.relative_to(vault)): digest(p) for p in vault.rglob('*.md')}
    finally:
        daemon.close()
    snapshot = root / 'pre-upgrade.db'
    with sqlite3.connect(db) as original, sqlite3.connect(snapshot) as target:
        original.backup(target)
    snapshot_hash = digest(snapshot)
    upgraded = root / 'upgraded-copy.db'
    shutil.copy2(snapshot, upgraded)
    daemon = Daemon(new_bin, upgraded, vault)
    try:
        after = {t['id']: t for t in daemon.rpc('tasks/list', {'limit': 100})}
        assert before.keys() == after.keys()
        for task_id, old in before.items():
            new = after[task_id]
            for field in ('name', 'payload', 'result', 'created_at'):
                assert old[field] == new[field], (task_id, field)
            assert new['state'] == ('pending' if old['state'] == 'claimed' else old['state'])
        new_agents = daemon.rpc('agents/list', {})
        assert [(a['id'], a['first_seen'], a['name']) for a in agents] == [(a['id'], a['first_seen'], a['name']) for a in new_agents]
        assert all((vault / path).is_file() and digest(vault / path) == value for path, value in notes.items())
        assert daemon.request('tasks/complete', {'id': active['id']})['error']['code'] == -32602
        claim = daemon.rpc('tasks/claim', {'id': active['id'], 'agentId': 'legacy-agent'})
        assert claim['lease_token'] and claim['attempt_no'] == 1
    finally:
        daemon.close()
    daemon = Daemon(new_bin, upgraded, vault)
    try:
        reopened = daemon.rpc('tasks/get', {'id': active['id']})
        assert reopened['state'] == 'claimed' and reopened['lease_token'] == claim['lease_token']
    finally:
        daemon.close()
    rollback = root / 'rollback-restored.db'
    shutil.copy2(snapshot, rollback)
    daemon = Daemon(old_bin, rollback, root / 'rollback-vault')
    try:
        restored = {t['id']: t for t in daemon.rpc('tasks/list', {'limit': 100})}
        assert restored == before, 'restored original database differs'
    finally:
        daemon.close()
    assert digest(snapshot) == snapshot_hash, 'pre-upgrade backup was changed'
    report = dict(status='PASS', tasks=len(before), agents=len(agents), legacy_vault_files_preserved=len(notes), old_binary_sha256=digest(old_bin), new_binary_sha256=digest(new_bin), preupgrade_backup_sha256=snapshot_hash, checks=['baseline binary generated real SQLite sample', 'tasks/results/agents/ack preserved', 'legacy active claim invalidated', 'tokenless writes refused', 'new fenced claim survives repeated migration/restart', 'old binary reads restored backup identically', 'backup unchanged'])
    args.report.write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(json.dumps(report))
    print(f'Retained disposable migration evidence: {root}')


if __name__ == '__main__':
    main()

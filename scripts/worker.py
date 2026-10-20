"""Windows reference worker (Python 3.10+, standard library only).

The default adapter returns the input JSON. Explicit --adapter command reads
one JSON task on stdin and writes one JSON result on stdout. Treat adapters as
trusted code, use isolated workdirs and business idempotency for side effects.
"""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request

STOP = threading.Event()
LIMIT = 1024 * 1024


class LostLease(Exception):
    pass


def contain_adapter(process):
    """Windows Job Object closes the adapter tree if the worker crashes."""
    if os.name != 'nt':
        return lambda: None
    import ctypes as c
    from ctypes import wintypes as w
    class Basic(c.Structure):
        _fields_ = [('process_time', c.c_int64), ('job_time', c.c_int64), ('flags', w.DWORD), ('min_ws', c.c_size_t), ('max_ws', c.c_size_t), ('active', w.DWORD), ('affinity', c.c_size_t), ('priority', w.DWORD), ('scheduling', w.DWORD)]
    class Io(c.Structure):
        _fields_ = [(name, c.c_uint64) for name in ('read_ops', 'write_ops', 'other_ops', 'read_bytes', 'write_bytes', 'other_bytes')]
    class Extended(c.Structure):
        _fields_ = [('basic', Basic), ('io', Io), ('process_memory', c.c_size_t), ('job_memory', c.c_size_t), ('peak_process', c.c_size_t), ('peak_job', c.c_size_t)]
    kernel = c.WinDLL('kernel32', use_last_error=True)
    kernel.CreateJobObjectW.argtypes = [c.c_void_p, w.LPCWSTR]
    kernel.CreateJobObjectW.restype = w.HANDLE
    kernel.SetInformationJobObject.argtypes = [w.HANDLE, c.c_int, c.c_void_p, w.DWORD]
    kernel.AssignProcessToJobObject.argtypes = [w.HANDLE, w.HANDLE]
    kernel.CloseHandle.argtypes = [w.HANDLE]
    job = kernel.CreateJobObjectW(None, None)
    limits = Extended()
    limits.basic.flags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
    if not job or not kernel.SetInformationJobObject(job, 9, c.byref(limits), c.sizeof(limits)) or not kernel.AssignProcessToJobObject(job, w.HANDLE(int(process._handle))):
        if job:
            kernel.CloseHandle(job)
        kill_adapter(process)
        raise OSError('cannot contain adapter in a Windows Job Object')
    return lambda: kernel.CloseHandle(job)


def rpc(url, method, params, timeout=2):
    request = urllib.request.Request(url, json.dumps(dict(jsonrpc='2.0', id=1, method=method, params=params)).encode(), {'Content-Type': 'application/json'})
    with urllib.request.urlopen(request, timeout=timeout) as response:
        value = json.load(response)
    if 'error' in value:
        error = value['error']
        if error['code'] == -32001:
            raise LostLease(error['message'])
        raise RuntimeError(str(error))
    return value['result']


def kill_adapter(process):
    if process.poll() is None:
        if os.name == 'nt':
            subprocess.run(['taskkill', '/PID', str(process.pid), '/T', '/F'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10, check=False)
        else:
            process.kill()  # Optional source-port behavior, not default support.
        if process.poll() is None:
            process.kill()
    process.wait(timeout=10)


def execute(args, task, credentials):
    if not args.adapter:
        return task['payload']
    if not args.workdir:
        raise ValueError('external adapters require an explicit --workdir')
    process = subprocess.Popen(args.adapter, cwd=args.workdir, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    close_job = contain_adapter(process)
    buffers = [bytearray(), bytearray()]
    overflow = threading.Event()

    def read(pipe, buf):
        try:
            while True:
                chunk = pipe.read(4096)
                if not chunk:
                    break
                remaining = LIMIT - len(buf)
                buf.extend(chunk[:remaining])
                if len(chunk) > remaining:
                    overflow.set()
                    break
        finally:
            pipe.close()

    def write():
        try:
            process.stdin.write(json.dumps(task).encode())
            process.stdin.close()
        except (OSError, ValueError):
            pass

    readers = [threading.Thread(target=read, args=(pipe, buf), daemon=True) for pipe, buf in zip((process.stdout, process.stderr), buffers)]
    for thread in readers:
        thread.start()
    writer = threading.Thread(target=write, daemon=True)
    writer.start()
    deadline = time.monotonic() + args.timeout
    renewal = time.monotonic() + max(.2, args.lease / 3)
    try:
        while process.poll() is None:
            if STOP.wait(.02):
                raise LostLease('worker stopping')
            if overflow.is_set():
                raise ValueError('adapter output exceeded 1 MiB')
            if time.monotonic() >= deadline:
                raise TimeoutError('adapter timed out')
            if time.monotonic() >= renewal:
                try:
                    rpc(args.url, 'scheduler/extend', credentials | {'leaseSeconds': args.lease}, timeout=min(2, args.lease / 3))
                    rpc(args.url, 'agents/heartbeat', {'id': args.agent, 'name': 'reference worker'})
                except Exception as error:
                    raise LostLease('lease renewal failed; adapter aborted') from error
                renewal = time.monotonic() + max(.2, args.lease / 3)
        for thread in readers:
            thread.join(timeout=2)
        if overflow.is_set() or any(t.is_alive() for t in readers):
            raise ValueError('adapter output exceeded limit or pipes stayed open')
        if process.returncode:
            raise ValueError(f'adapter exited {process.returncode}')
        return json.loads(buffers[0])
    finally:
        close_job()
        kill_adapter(process)
        for thread in readers:
            thread.join(timeout=2)
        writer.join(timeout=2)


def run(args):
    completed = 0
    delay = .1
    while not STOP.is_set():
        try:
            rpc(args.url, 'agents/heartbeat', {'id': args.agent, 'name': 'reference worker'})
            claim = rpc(args.url, 'scheduler/claim', {'agent_id': args.agent, 'batch_id': args.batch, 'lease_seconds': args.lease})
            if claim['empty']:
                progress = rpc(args.url, 'batches/status', {'batch_id': args.batch})
                if progress['active'] == 0:
                    return
                STOP.wait(min(delay, 1))
                delay = min(delay * 2, 1)
                continue
            delay = .1
            task = claim['task']
            credentials = {'id': task['id'], 'agentId': args.agent, 'leaseToken': task['lease_token']}
            try:
                result = execute(args, task, credentials)
            except LostLease:
                raise
            except Exception as error:
                rpc(args.url, 'scheduler/failure', {'id': task['id'], 'agent_id': args.agent, 'lease_token': task['lease_token'], 'error': {'kind': 'adapter_error', 'message': str(error)}})
                continue
            if STOP.is_set():
                return
            # A dropped response replays the exact fenced request; never re-execute
            # the adapter just to retry submission while this lease is held.
            submitted = False
            submit = credentials | {'result': result}
            for attempt in range(4):
                try:
                    rpc(args.url, 'scheduler/submit', submit)
                    submitted = True
                    break
                except LostLease:
                    raise
                except (OSError, urllib.error.URLError):
                    if STOP.wait(min(.1 * 2 ** attempt, 1)):
                        return
            if not submitted:
                raise LostLease('submission disconnected; stop stale work and let lease recover')
            completed += 1
            print(json.dumps({'event': 'accepted', 'task_id': task['id']}), flush=True)
            if args.max_tasks and completed >= args.max_tasks:
                return
        except LostLease as error:
            print(json.dumps({'event': 'lease_lost', 'message': str(error)}), flush=True)
            if STOP.is_set():
                return
            try:
                if rpc(args.url, 'batches/status', {'batch_id': args.batch})['active'] == 0:
                    return
            except (OSError, RuntimeError):
                pass
            STOP.wait(.1)
        except (OSError, urllib.error.URLError, RuntimeError) as error:
            print(json.dumps({'event': 'reconnect', 'message': str(error)}), flush=True)
            STOP.wait(min(delay, 5))
            delay = min(delay * 2, 5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', default='http://127.0.0.1:7777/')
    parser.add_argument('--batch', required=True)
    parser.add_argument('--agent', required=True)
    parser.add_argument('--lease', type=int, default=30)
    parser.add_argument('--timeout', type=float, default=60)
    parser.add_argument('--workdir', type=Path)
    parser.add_argument('--max-tasks', type=int, default=0)
    parser.add_argument('--adapter', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if not 1 <= args.lease <= 3600 or not 0 < args.timeout <= 3600:
        parser.error('lease and adapter timeout must be within (0,3600]')
    if args.adapter and not args.workdir:
        parser.error('external adapters require --workdir')
    signal.signal(signal.SIGINT, lambda *_: STOP.set())
    signal.signal(signal.SIGTERM, lambda *_: STOP.set())
    run(args)


if __name__ == '__main__':
    main()

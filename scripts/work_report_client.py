"""Durable client for direct work reports; explicit local destination required."""
import argparse
from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
from threading import Event

MAX_BYTES = 128 * 1024


def _write(path, value):
    with tempfile.NamedTemporaryFile(mode='w', dir=path.parent, delete=False) as file:
        temporary = Path(file.name)
        try:
            json.dump(value, file, sort_keys=True)
            file.flush()
            os.fsync(file.fileno())
            os.replace(temporary, path)
        finally:
            temporary.unlink(missing_ok=True)
    fd = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


@contextmanager
def _locked(spool, db_path):
    spool = Path(spool)
    if spool.is_symlink():
        raise ValueError('Queue must be a dedicated directory, not a symlink')
    spool.mkdir(mode=0o700, parents=True, exist_ok=True)
    if spool.stat().st_mode & 0o077:
        raise ValueError('Queue directory must be private (mode 0700)')
    with os.fdopen(os.open(spool/'lock', os.O_CREAT | os.O_RDWR, 0o600), 'w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        destination = str(Path(db_path).resolve())
        route = spool/'destination.json'
        if route.exists():
            if json.loads(route.read_text()) != {'db_path':destination}:
                raise ValueError('Queue belongs to a different database')
        else:
            _write(route, {'db_path':destination})
        yield spool, destination


def _identity(report):
    if not isinstance(report, dict):
        raise ValueError('Report must be an object')
    for field in ('source','run_id'):
        if not isinstance(report.get(field), str) or not report[field].strip() or len(report[field]) > 240:
            raise ValueError(f'Invalid {field}')
    if type(report.get('sequence')) is not int or report['sequence'] < 0:
        raise ValueError('Sequence must be a nonnegative integer')
    return report['source'], report['run_id'], report['sequence']


def _drain(spool, destination, clio):
    jobs = []
    errors = []
    for path in spool.glob('*.job.json'):
        try:
            payload = json.loads(path.read_text())
            jobs.append((_identity(payload), path, payload))
        except (ValueError, OSError) as error:
            errors.append(f'{path.name}: unreadable job retained ({error})')
    blocked = set()
    confirmed = 0
    # ponytail: serial work-only delivery; use per-run workers if bounded CLI latency becomes too slow.
    for identity, path, payload in sorted(jobs, key=lambda job: job[0]):
        run = identity[:2]
        if run in blocked:
            continue
        try:
            result = subprocess.run(
                [str(Path(clio).resolve()),'--local','--db-path',destination,'--json','work','report','-'],
                input=json.dumps(payload), text=True, capture_output=True, timeout=10,
            )
            if result.returncode:
                raise ValueError(f'report command exited {result.returncode}')
            receipt = json.loads(result.stdout)
            if (not isinstance(receipt, dict) or type(receipt.get('id')) is not int
                    or type(receipt.get('received_at')) is not int or receipt.get('report') != payload):
                raise ValueError('receipt did not confirm the submitted report')
            path.unlink()
            confirmed += 1
        except (ValueError, OSError, subprocess.TimeoutExpired) as error:
            blocked.add(run)
            errors.append(f'{path.name}: {error}')
    return {'confirmed':confirmed, 'pending':len(list(spool.glob('*.job.json'))), 'errors':errors}


def publish(spool, db_path, clio, report):
    identity = _identity(report)
    report = {**report, 'supersedes':report.get('supersedes')}
    if len(json.dumps(report).encode()) > MAX_BYTES:
        raise ValueError('Report exceeds 128 KiB')
    with _locked(spool, db_path) as (directory, destination):
        key = hashlib.sha256(json.dumps(identity).encode()).hexdigest()
        path = directory/f'{key}.job.json'
        if path.exists():
            if json.loads(path.read_text()) != report:
                raise ValueError('Queued identity already contains a different report')
        else:
            _write(path, report)
        return _drain(directory, destination, clio)


def drain(spool, db_path, clio):
    with _locked(spool, db_path) as (directory, destination):
        return _drain(directory, destination, clio)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--spool', type=Path, required=True)
    parser.add_argument('--db-path', type=Path, required=True)
    parser.add_argument('--clio', type=Path, required=True)
    parser.add_argument('action', choices=['publish','drain','watch'])
    parser.add_argument('--interval', type=int, default=5)
    args = parser.parse_args()
    if args.interval < 1:
        parser.error('interval must be at least one second')
    if args.action == 'publish':
        data = sys.stdin.buffer.read(MAX_BYTES + 1)
        if len(data) > MAX_BYTES:
            parser.error('report exceeds 128 KiB')
        result = publish(args.spool,args.db_path,args.clio,json.loads(data))
        print(json.dumps(result), flush=True)
        return 2 if result['pending'] else 0
    if args.action == 'drain':
        result = drain(args.spool,args.db_path,args.clio)
        print(json.dumps(result), flush=True)
        return 2 if result['pending'] else 0
    stop = Event()
    for name in (signal.SIGTERM,signal.SIGINT):
        signal.signal(name, lambda *_: stop.set())
    previous = None
    while not stop.is_set():
        result = drain(args.spool,args.db_path,args.clio)
        if result != previous:
            print(json.dumps(result), flush=True)
            previous = result
        stop.wait(args.interval)
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (ValueError,OSError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)

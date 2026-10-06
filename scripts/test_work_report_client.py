"""Delivery tests use disposable queues and a controllable external command."""
import importlib.util
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor

SPEC = importlib.util.spec_from_file_location('client', pathlib.Path(__file__).with_name('work_report_client.py'))
client = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(client)


class DeliveryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name)
        self.spool, self.db = self.root / 'queue', self.root / 'isolated.sqlite'
        self.binary = self.root / 'clio'
        self.binary.write_text('''#!/usr/bin/env python3
import json, pathlib, sys
root=pathlib.Path(__file__).parent
report=json.load(sys.stdin)
if (root/'offline').exists() or ((root/'fail-a').exists() and report['run_id']=='a'):
 sys.exit(1)
with (root/'delivered.jsonl').open('a') as f: f.write(json.dumps(report)+'\\n')
print(json.dumps({'id':1,'received_at':100,'report':report}))
''')
        self.binary.chmod(0o700)

    def update(self, run='a', sequence=0):
        return {'source':'test', 'run_id':run, 'sequence':sequence, 'summary':'update', 'supersedes':None}

    def send(self, report):
        return client.publish(self.spool, self.db, self.binary, report)

    def flush(self):
        return client.drain(self.spool, self.db, self.binary)

    def delivered(self):
        file = self.root / 'delivered.jsonl'
        return [json.loads(line) for line in file.read_text().splitlines()] if file.exists() else []

    def test_outage_keeps_exact_payload_until_successful_receipt(self):
        (self.root/'offline').touch()
        report = self.update()
        self.assertEqual(self.send(report)['pending'], 1)
        self.assertEqual(self.send(report)['pending'], 1)
        with self.assertRaises(ValueError):
            self.send({**report, 'summary':'changed payload'})
        self.assertEqual(len(list(self.spool.glob('*.job.json'))), 1)
        (self.root/'offline').unlink()
        self.assertEqual(self.flush()['confirmed'], 1)
        self.assertEqual(self.delivered(), [report])
        self.assertEqual(list(self.spool.glob('*.job.json')), [])

    def test_failure_blocks_only_its_run_and_keeps_sequence_order(self):
        (self.root/'fail-a').touch()
        self.send(self.update('a', 0))
        self.send(self.update('a', 1))
        self.send(self.update('b', 0))
        self.assertEqual(self.delivered(), [self.update('b', 0)])
        (self.root/'fail-a').unlink()
        self.assertEqual(self.flush()['confirmed'], 2)
        self.assertEqual(self.delivered()[1:], [self.update('a', 0),self.update('a', 1)])

    def test_simultaneous_enqueue_keeps_one_durable_job(self):
        (self.root/'offline').touch()
        with ThreadPoolExecutor(max_workers=3) as pool:
            list(pool.map(lambda _: self.send(self.update()), range(3)))
        self.assertEqual(len(list(self.spool.glob('*.job.json'))), 1)
        self.assertEqual(self.spool.stat().st_mode & 0o777, 0o700)
        self.assertEqual(next(self.spool.glob('*.job.json')).stat().st_mode & 0o777, 0o600)

    def test_pending_reports_cannot_be_redirected_to_another_database(self):
        (self.root/'offline').touch()
        self.send(self.update())
        with self.assertRaises(ValueError):
            client.drain(self.spool,self.root/'different.sqlite',self.binary)
        self.assertEqual(len(list(self.spool.glob('*.job.json'))), 1)

    def test_bad_receipt_does_not_discard_a_report(self):
        self.binary.write_text('#!/usr/bin/env python3\nprint("{}")\n')
        result = self.send(self.update())
        self.assertEqual(result['pending'], 1)
        self.assertEqual(result['confirmed'], 0)

    def test_relative_executable_uses_requested_file_instead_of_path(self):
        alternative = self.root/'path'
        alternative.mkdir()
        (alternative/'clio').write_text('#!/bin/sh\nexit 99\n')
        (alternative/'clio').chmod(0o700)
        result = subprocess.run([
            sys.executable, str(pathlib.Path(client.__file__).resolve()),
            '--spool',str(self.spool),'--db-path',str(self.db),'--clio','./clio','publish',
        ], input=json.dumps(self.update()), text=True, capture_output=True,
            cwd=self.root, env={**os.environ,'PATH':str(alternative)+os.pathsep+os.environ['PATH']})
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.delivered(), [self.update()])

    def test_optional_predecessor_round_trips_like_the_core_report(self):
        (self.root/'offline').touch()
        omitted = self.update()
        omitted.pop('supersedes')
        self.send(omitted)
        self.assertEqual(self.send(self.update())['pending'], 1)
        (self.root/'offline').unlink()
        self.assertEqual(self.flush()['confirmed'], 1)
        self.assertEqual(self.delivered(), [self.update()])


if __name__ == '__main__':
    unittest.main()

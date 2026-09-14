"""Exercise built local binaries and automatic delivery using disposable data only."""
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from concurrent.futures import ThreadPoolExecutor

ROOT = Path(__file__).resolve().parents[1]
CLI = ROOT/'target/debug/clio'
MCP = ROOT/'target/debug/clio-mcp'
CLIENT = ROOT/'scripts/work_report_client.py'


def fixture(run, project='sample-a'):
    return dict(project=project,task=f'task-{run}',task_title=f'Sample {run}',
                source='integration-test',session_id=f'session-{run}',run_id=run,
                sequence=0,observed_at=int(time.time()),worktree=f'fixture-tree-{run}',
                revision='fixture-revision',state='running',summary='Started',
                next_step='Continue scoped work',next_actor='agent',evidence=[],
                evidence_status='current',supersedes=None)


class IntegrationTests(unittest.TestCase):
    def setUp(self):
        self.assertTrue(CLI.is_file() and MCP.is_file(), 'Build clio-cli and clio-mcp first')
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.db = self.root/'test.sqlite'

    def cli(self, *args, report=None):
        output = subprocess.run([str(CLI),'--local','--db-path',str(self.db),'--json','work',*args],
                                input=json.dumps(report) if report else None,
                                capture_output=True,text=True,timeout=10,check=True)
        return json.loads(output.stdout)

    def test_three_cli_processes_report_without_losing_tasks(self):
        self.cli('overview')
        reports = [fixture('a'),fixture('b'),fixture('c','sample-b')]
        with ThreadPoolExecutor(max_workers=3) as pool:
            receipts = list(pool.map(lambda r:self.cli('report','-',report=r),reports))
        self.assertEqual(len(self.cli('overview')['tasks']),3)
        self.assertEqual(len(self.cli('overview','--project','sample-a')['tasks']),2)
        self.assertEqual(self.cli('report','-',report=reports[0]),receipts[0])
        reports[0].update(sequence=2,state='implemented',next_actor='user',summary='Ready for review')
        self.cli('report','-',report=reports[0])
        reports[0].update(sequence=1,state='running')
        self.cli('report','-',report=reports[0])
        a = next(t for t in self.cli('overview')['tasks'] if t['task']=='task-a')
        self.assertEqual(a['state'],'implemented')

    def test_mcp_stdio_and_cli_share_reports(self):
        process = subprocess.Popen([str(MCP)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE,text=True,
                                   env={**os.environ,'CLIO_DB_PATH':str(self.db)})
        messages = queue.Queue()
        reader = threading.Thread(target=lambda:[messages.put(json.loads(line)) for line in process.stdout],daemon=True)
        reader.start()
        def send(value):
            process.stdin.write(json.dumps(value)+'\n')
            process.stdin.flush()
        def request(identity, method, params):
            send(dict(jsonrpc='2.0',id=identity,method=method,params=params))
            deadline=time.monotonic()+8
            while time.monotonic()<deadline:
                response=messages.get(timeout=max(0.01,deadline-time.monotonic()))
                if response.get('id')==identity:
                    self.assertNotIn('error',response)
                    return response['result']
            self.fail('MCP response timed out')
        try:
            request(1,'initialize',dict(protocolVersion='2024-11-05',capabilities={},clientInfo=dict(name='isolated-test',version='1')))
            send(dict(jsonrpc='2.0',method='notifications/initialized'))
            tools=request(2,'tools/list',{})['tools']
            self.assertTrue({'work_report','work_overview','work_history'} <= {t['name'] for t in tools})
            payload=fixture('mcp')
            result=request(3,'tools/call',dict(name='work_report',arguments={'report':payload}))
            self.assertFalse(result.get('isError',False))
            receipt=json.loads(result['content'][0]['text'])
            self.assertEqual(receipt['report'],payload)
            self.assertEqual(self.cli('overview')['tasks'][0]['task'],'task-mcp')
            replay=request(4,'tools/call',dict(name='work_report',arguments={'report':payload}))
            self.assertEqual(json.loads(replay['content'][0]['text']),receipt)
        finally:
            process.terminate()
            process.communicate(timeout=5)
            reader.join(timeout=2)
            self.assertIsNotNone(process.returncode)

    def test_watcher_recovers_queued_report_without_another_publish(self):
        facade=self.root/'report-binary'
        args=[sys.executable,str(CLIENT),'--spool',str(self.root/'spool'),'--db-path',str(self.db),'--clio',str(facade)]
        published=subprocess.run([*args,'publish'],input=json.dumps(fixture('retry')),
                                 text=True,capture_output=True,timeout=10)
        self.assertEqual(published.returncode,2)
        self.assertEqual(json.loads(published.stdout)['pending'],1)
        watcher=subprocess.Popen([*args,'watch','--interval','1'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
        try:
            facade.symlink_to(CLI)
            deadline=time.monotonic()+8
            delivered=False
            while time.monotonic()<deadline:
                if self.cli('overview')['tasks']:
                    delivered=True
                    break
                time.sleep(0.1)
            self.assertTrue(delivered,'watcher did not deliver after the command recovered')
            self.assertEqual(self.cli('overview')['tasks'][0]['task'],'task-retry')
            self.assertEqual(list((self.root/'spool').glob('*.job.json')),[])
        finally:
            watcher.terminate()
            watcher.communicate(timeout=5)
            self.assertEqual(watcher.returncode,0)


if __name__=='__main__':
    unittest.main()

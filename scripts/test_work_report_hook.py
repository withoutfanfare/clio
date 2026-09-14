"""Native hook and reporter checks against the built CLI and disposable local data."""
import json
from pathlib import Path
import shlex
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing

ROOT = Path(__file__).resolve().parents[1]
CLI = ROOT / 'target/debug/clio'
HOOK = ROOT / 'scripts/work_report_hook.py'


class HookTests(unittest.TestCase):
    def setUp(self):
        self.assertTrue(CLI.is_file(), 'Build clio-cli first')
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.cwd = self.root / 'project with spaces'
        self.cwd.mkdir()
        self.config = self.root / 'config.json'
        self.settings = dict(clio=str(CLI), db_path=str(self.root/'work.sqlite'),
                             state_dir=str(self.root/'private'),
                             projects=[dict(id='sample-project', roots=[str(self.cwd)])])
        self.save_config()
        subprocess.run([str(CLI), '--local', '--db-path', self.settings['db_path'],
                        '--json', 'work', 'overview'], capture_output=True, check=True)

    def save_config(self):
        self.config.write_text(json.dumps(self.settings))

    def invoke(self, action, data, session='sample-session', cwd=None, token='turn-1', source='codex'):
        command = [sys.executable, str(HOOK), '--config', str(self.config), '--source', source, action]
        if action == 'report':
            command += ['--session', session, '--cwd', str(cwd or self.cwd), '--checkpoint', token]
        return subprocess.run(command, input=json.dumps(data), text=True, capture_output=True, timeout=20)

    def hook(self, event='UserPromptSubmit', session='sample-session', cwd=None, token='turn-1', **extra):
        result = self.invoke('hook', dict(hook_event_name=event, session_id=session,
                             cwd=str(cwd or self.cwd), turn_id=token, **extra))
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout or '{}')

    def compact(self, task='sample-task', **extra):
        return dict(task=task, task_title='Sample task', state='running', summary='Starting scoped work',
                    next_step='Run the checks', next_actor='agent', evidence=[], evidence_status='current', **extra)

    def rows(self):
        db = Path(self.settings['db_path'])
        if not db.exists():
            return []
        with closing(sqlite3.connect(db)) as conn:
            return [json.loads(row[0]) for row in conn.execute('SELECT payload FROM work_reports ORDER BY id')]

    def report(self, report=None, **kwargs):
        result = self.invoke('report', report or self.compact(), **kwargs)
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        return json.loads(result.stdout)

    def test_prompt_command_is_executable_and_reports_are_idempotent(self):
        prompt = self.hook()['hookSpecificOutput']['additionalContext']
        command = next(line for line in prompt.splitlines() if line.startswith(shlex.quote(sys.executable)))
        result = subprocess.run(shlex.split(command), input=json.dumps(self.compact()), text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        first = self.rows()[0]
        self.report()
        self.assertEqual(self.rows(), [first])
        final = self.compact()
        final.update(state='implemented', next_actor='user')
        self.report(final)
        self.assertEqual([row['sequence'] for row in self.rows()], [0, 1])
        self.assertNotIn('decision', self.hook('Stop'))

    def test_returning_to_earlier_state_is_a_new_observation(self):
        self.hook()
        self.report()
        self.report({**self.compact(), 'state':'stopped'})
        self.report()
        self.assertEqual([row['sequence'] for row in self.rows()], [0, 1, 2])
        self.assertEqual(self.rows()[-1]['state'], 'running')
        self.assertEqual(self.hook('Stop')['decision'], 'block')

    def test_stop_blocks_once_per_turn_and_old_reports_cannot_satisfy_new_turn(self):
        self.hook()
        self.report()
        self.assertEqual(self.hook('Stop', stop_hook_active=True)['decision'], 'block')
        second = self.hook('Stop', stop_hook_active=False)
        self.assertNotIn('decision', second)
        self.assertIn('Work reporting is incomplete', second['systemMessage'])
        self.hook(token='turn-2')
        stale = self.invoke('report', {**self.compact(), 'state':'stopped'})
        self.assertNotEqual(stale.returncode, 0)
        self.assertEqual(self.hook('Stop', token='turn-2')['decision'], 'block')
        self.report({**self.compact(), 'state':'waiting', 'next_actor':'user'}, token='turn-2')
        self.assertNotIn('systemMessage', self.hook('Stop', token='turn-2'))

    def test_session_start_does_not_invent_progress_and_unknown_scope_is_noop(self):
        self.assertIn('additionalContext', self.hook('SessionStart')['hookSpecificOutput'])
        self.assertEqual(self.rows(), [])
        outside = self.root / 'project with spaces-other'
        outside.mkdir()
        self.assertEqual(self.hook(cwd=outside), {})
        self.assertNotEqual(self.invoke('report', self.compact(), cwd=outside).returncode, 0)
        self.settings['projects'].append(dict(id='ambiguous', roots=[str(self.root)]))
        self.save_config()
        result = self.invoke('hook', dict(hook_event_name='UserPromptSubmit', session_id='sample-session', cwd=str(self.cwd)))
        self.assertEqual(result.returncode, 0)
        self.assertIn('overlapping', json.loads(result.stdout)['systemMessage'])

    def test_three_sessions_report_concurrently_without_losing_tasks(self):
        def run(index):
            session = f'session-{index}'
            self.hook(session=session)
            self.report(self.compact(task=f'task-{index}'), session=session)
        with ThreadPoolExecutor(max_workers=3) as pool:
            list(pool.map(run, range(3)))
        self.assertEqual({row['task'] for row in self.rows()}, {'task-0','task-1','task-2'})
        self.assertEqual(len({row['run_id'] for row in self.rows()}), 3)

    def test_parallel_reports_in_one_session_allocate_monotonic_sequences(self):
        self.hook()
        with ThreadPoolExecutor(max_workers=3) as pool:
            list(pool.map(lambda index:self.report({**self.compact(), 'summary':f'Observation {index}'}), range(3)))
        self.assertEqual([row['sequence'] for row in self.rows()], [0, 1, 2])
        self.assertEqual({row['summary'] for row in self.rows()}, {'Observation 0','Observation 1','Observation 2'})

    def test_claude_source_is_separate_and_native_prompt_content_is_not_retained(self):
        self.hook()
        self.report()
        result = self.invoke('hook', dict(hook_event_name='UserPromptSubmit', session_id='sample-session',
                             cwd=str(self.cwd), prompt='PRIVATE_PROMPT_MARKER', transcript_path='/not-a-transcript'), source='claude')
        self.assertEqual(result.returncode, 0, result.stderr)
        context = json.loads(result.stdout)['hookSpecificOutput']['additionalContext']
        command = next(line for line in context.splitlines() if line.startswith(shlex.quote(sys.executable)))
        sent = subprocess.run(shlex.split(command), input=json.dumps(self.compact()), text=True, capture_output=True)
        self.assertEqual(sent.returncode, 0, sent.stderr)
        self.assertEqual({row['source'] for row in self.rows()}, {'codex','claude'})
        self.assertEqual(len({row['run_id'] for row in self.rows()}), 2)
        for path in (self.root/'private').rglob('*.json'):
            self.assertNotIn('PRIVATE_PROMPT_MARKER', path.read_text())
            self.assertNotIn('/not-a-transcript', path.read_text())

    def test_invalid_reports_do_not_poison_sequence(self):
        self.hook()
        invalid = [{**self.compact(), 'state':'done'}, {**self.compact(), 'evidence':['']},
                   {**self.compact(), 'summary':'x'*1001}, {**self.compact(), 'evidence_status':'unknown'},
                   {**self.compact(), 'unexpected':True}, {**self.compact(), 'task':'\0'},
                   {**self.compact(), 'evidence':['x'*2001]}, {**self.compact(), 'summary':'x'*140000}]
        for report in invalid:
            self.assertNotEqual(self.invoke('report', report).returncode, 0)
        self.assertEqual(self.rows(), [])
        self.assertEqual(list((self.root/'private').rglob('*.job.json')), [])
        self.report()
        self.assertEqual(self.rows()[0]['sequence'], 0)

    def test_failure_keeps_exact_payload_and_recovers_before_new_sequence(self):
        self.hook()
        self.settings['clio'] = str(self.root/'missing-binary')
        self.save_config()
        failed = self.invoke('report', self.compact())
        self.assertEqual(failed.returncode, 2, failed.stderr)
        jobs = list((self.root/'private').rglob('*.job.json'))
        self.assertEqual(len(jobs), 1)
        original = json.loads(jobs[0].read_text())
        self.assertEqual(self.hook('Stop')['decision'], 'block')
        self.settings['clio'] = str(CLI)
        self.save_config()
        self.report()
        self.assertEqual(self.rows(), [original])
        self.report({**self.compact(), 'state':'stopped'})
        self.assertEqual([row['sequence'] for row in self.rows()], [0, 1])

    def test_hook_recovers_queued_final_report_without_another_agent_report(self):
        self.hook()
        self.settings['clio'] = str(self.root/'missing-binary')
        self.save_config()
        result = self.invoke('report', {**self.compact(), 'state':'stopped'})
        self.assertEqual(result.returncode, 2)
        self.assertIn('systemMessage', json.loads(result.stdout))
        self.settings['clio'] = str(CLI)
        self.save_config()
        self.assertEqual(self.hook('Stop'), {})
        self.assertEqual(self.rows()[0]['state'], 'stopped')

    def test_symlinked_database_and_sidecars_never_touch_the_target(self):
        self.hook()
        database = Path(self.settings['db_path'])
        original = database.read_bytes()
        target = self.root/'unrelated.sqlite'
        target.write_bytes(b'Unrelated data must remain unchanged')
        for suffix in ('', '-wal', '-shm', '-journal'):
            with self.subTest(suffix=suffix):
                link = Path(str(database)+suffix)
                link.unlink(missing_ok=True)
                link.symlink_to(target)
                result = self.invoke('report', self.compact())
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('symlink', result.stderr)
                self.assertEqual(target.read_bytes(), b'Unrelated data must remain unchanged')
                self.assertEqual(list((self.root/'private').rglob('*.job.json')), [])
                link.unlink()
                if not suffix:
                    database.write_bytes(original)

    def test_invalid_handover_is_rejected_before_queueing(self):
        self.hook()
        result = self.invoke('report', {**self.compact(), 'supersedes':dict(source='codex', run_id='missing')})
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(list((self.root/'private').rglob('*.job.json')), [])
        self.report()
        self.assertEqual(self.rows()[0]['sequence'], 0)

    def test_same_task_in_distinct_worktrees_has_distinct_runs_and_explicit_handover(self):
        self.hook()
        self.report()
        prior = self.rows()[0]
        other = self.cwd/'second-tree'
        other.mkdir()
        self.hook(cwd=other)
        self.report({**self.compact(), 'supersedes':dict(source='codex', run_id=prior['run_id'])}, cwd=other)
        rows = self.rows()
        self.assertNotEqual(rows[0]['run_id'], rows[1]['run_id'])
        self.assertEqual(rows[1]['supersedes']['run_id'], rows[0]['run_id'])


if __name__ == '__main__':
    unittest.main()

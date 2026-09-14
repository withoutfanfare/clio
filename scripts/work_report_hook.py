"""Bounded native agent checkpoints with explicit project scope and durable local delivery."""
import argparse
import hashlib
import json
from pathlib import Path
import shlex
import subprocess
import sys
import time
import uuid

import work_report_client as client


def text(field, value, limit=240, required=True):
    if (not isinstance(value, str) or len(value) > limit or '\0' in value
            or (required and not value.strip())):
        raise ValueError(f'Invalid {field}: expected {1 if required else 0} to {limit} characters without NUL')
    return value


def absolute(field, value):
    text(field, value, 2000)
    if not Path(value).is_absolute():
        raise ValueError(f'{field} must be absolute')
    return Path(value).resolve()


def read_input():
    data = sys.stdin.buffer.read(client.MAX_BYTES + 1)
    if len(data) > client.MAX_BYTES:
        raise ValueError('Input exceeds 128 KiB')
    value = json.loads(data)
    if not isinstance(value, dict):
        raise ValueError('Input must be an object')
    return value


def compact(value):
    fields = {'task','task_title','state','summary','next_step','next_actor','evidence','evidence_status'}
    if not fields <= value.keys() or value.keys() - fields - {'supersedes'}:
        raise ValueError('Compact report requires exactly the documented fields')
    for field in ('task','task_title'):
        text(field, value[field])
    for field in ('summary','next_step'):
        text(field, value[field], 1000, False)
    for field, choices in [('state', ('running','waiting','stopped','implemented')),
                           ('next_actor', ('agent','user','other','none')),
                           ('evidence_status', ('current','unavailable'))]:
        if value[field] not in choices:
            raise ValueError(f'Invalid {field}')
    evidence = value['evidence']
    if not isinstance(evidence, list) or len(evidence) > 32:
        raise ValueError('Evidence must be a list of at most 32 references')
    for reference in evidence:
        text('evidence', reference, 2000)
    previous = value.get('supersedes')
    if previous is not None:
        if not isinstance(previous, dict) or set(previous) != {'source','run_id'}:
            raise ValueError('Supersedes requires source and run_id')
        for field in previous:
            text(f'supersedes {field}', previous[field])
    return {**value, 'supersedes':previous}


def settings(path, cwd):
    config = json.loads(path.read_text())
    if not isinstance(config, dict):
        raise ValueError('Config must be an object')
    raw_database = text('db_path', config.get('db_path'), 2000)
    for suffix in ('', '-wal', '-shm', '-journal'):
        if Path(raw_database + suffix).is_symlink():
            raise ValueError('Dedicated database and sidecars must not be symlinks')
    for field in ('clio','db_path','state_dir'):
        config[field] = absolute(field, config.get(field))
    projects = config.get('projects')
    if not isinstance(projects, list):
        raise ValueError('Config projects must be a list')
    matches, seen = [], set()
    for project in projects:
        if not isinstance(project, dict):
            raise ValueError('Invalid project configuration')
        identity = text('project id', project.get('id'))
        if identity in seen:
            raise ValueError('Project ids must be unique')
        seen.add(identity)
        roots = project.get('roots')
        if not isinstance(roots, list) or not roots:
            raise ValueError('Each project needs absolute roots')
        resolved = [absolute('project root', root) for root in roots]
        if any(cwd.is_relative_to(root) for root in resolved):
            matches.append(identity)
    if len(matches) > 1:
        raise ValueError('Working directory matches overlapping projects')
    return config, matches[0] if matches else None


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True).encode()).hexdigest()


def instructions(args, cwd, token):
    command = shlex.join([sys.executable, str(Path(__file__).resolve()), '--config', str(args.config.resolve()),
                          '--source', args.source, 'report', '--session', args.session,
                          '--cwd', str(cwd), '--checkpoint', token])
    return ('Work reporting is enabled for this project. Before working, at meaningful checkpoints (and about every three minutes during active work where practical), and before your final '
            'response, run this command with one compact JSON object on stdin:\n' + command + '\n'
            'Required fields: task, task_title, state (running|waiting|stopped|implemented), summary, next_step, '
            'next_actor (agent|user|other|none), evidence (reference strings), evidence_status (current|unavailable). '
            'Use one stable task id for this work or the exact known issue id; do not infer an issue from a branch. '
            'Keep the same task id on follow-ups. Optional supersedes is {"source":"previous-source","run_id":"previous-run"} '
            'only for an explicit handover. Report actual observations, never invent success. Use implemented only when '
            'ready for human review; acceptance remains human-owned. When stopping, report waiting, stopped or implemented '
            'honestly. Include no secrets, full transcript or user prompt. Queued delivery (exit 2) needs an unchanged retry; '
            'do not claim reporting succeeded until confirmed.')


def hook(args, data, cwd, state):
    event = data.get('hook_event_name')
    if event not in ('SessionStart','UserPromptSubmit','Stop'):
        return {}
    if event == 'UserPromptSubmit':
        token = data.get('turn_id') or str(uuid.uuid4())
        text('turn_id', token)
        if token != state.get('checkpoint'):
            state.update(checkpoint=token, stop_blocked=False, latest=None)
    if not state.get('checkpoint'):
        state.update(checkpoint=str(uuid.uuid4()), stop_blocked=False, latest=None)
    context = instructions(args, cwd, state['checkpoint'])
    if event != 'Stop':
        return {'hookSpecificOutput':{'hookEventName':event, 'additionalContext':context}}
    latest = state.get('latest')
    if (latest and latest['checkpoint'] == state['checkpoint'] and latest['delivered']
            and latest['state'] != 'running'):
        return {}
    if not state.get('stop_blocked'):
        state['stop_blocked'] = True
        return {'decision':'block', 'reason':'Before finishing, submit one honest current work checkpoint. ' + context}
    return {'systemMessage':'Work reporting is incomplete: no confirmed final checkpoint for this turn. '
            'The task status may be outdated. Work may stop; reporting must not be described as complete.'}


def revision(cwd):
    try:
        result = subprocess.run(['git','-C',str(cwd),'rev-parse','HEAD'], capture_output=True, text=True, timeout=3)
        if result.returncode == 0:
            return text('revision', result.stdout.strip())
    except (OSError, subprocess.TimeoutExpired):
        pass
    return 'unavailable'


def validate_handover(config, report, project):
    previous = report['supersedes']
    if previous is None:
        return
    result = subprocess.run([str(config['clio']), '--local', '--db-path', str(config['db_path']),
                             '--json','work','overview','--project',project], capture_output=True, text=True, timeout=10)
    if result.returncode:
        raise ValueError('Cannot verify handover; retry when the reporting destination is available')
    runs = [run for task in json.loads(result.stdout)['tasks'] if task['task'] == report['task'] for run in task['runs']]
    if not any(not run['superseded'] and run['receipt']['report']['source'] == previous['source']
               and run['receipt']['report']['run_id'] == previous['run_id'] for run in runs):
        raise ValueError('Superseded run must be an active run in the same project and task')


def report(args, value, cwd, config, project, directory, state):
    value = compact(value)
    text('checkpoint', args.checkpoint)
    if args.checkpoint != state.get('checkpoint'):
        raise ValueError('Checkpoint is no longer current; use the latest prompt reporting command')
    run_id = digest([args.source, args.session, value['task'], str(cwd)])
    runs = state.setdefault('runs', {})
    run = runs.get(run_id, {'sequence':-1, 'reports':{}})
    fingerprint = digest([args.checkpoint, value])
    key = str(run['sequence'])
    entry = run['reports'].get(key) if run.get('latest_fingerprint') == fingerprint else None
    if entry is None:
        if run['sequence'] < 0:
            validate_handover(config, value, project)
        elif value['supersedes'] is not None and value['supersedes'] != run['supersedes']:
            raise ValueError('A run cannot change its predecessor')
        payload = dict(value, project=project, source=args.source, session_id=args.session, run_id=run_id,
                       sequence=run['sequence']+1, observed_at=int(time.time()), worktree=str(cwd), revision=revision(cwd))
        if len(json.dumps(payload).encode()) > client.MAX_BYTES:
            raise ValueError('Expanded report exceeds 128 KiB')
        # Save the exact pending payload before delivery or allocating a later sequence.
        entry = {'payload':payload, 'delivered':False}
        key = str(payload['sequence'])
        run['reports'][key] = entry
        run['latest_fingerprint'] = fingerprint
        run.update(sequence=payload['sequence'], supersedes=run.get('supersedes',value['supersedes']))
        runs[run_id] = run
        state['latest'] = dict(checkpoint=args.checkpoint, state=value['state'], delivered=False, run_id=run_id, key=key)
        client._write(directory/'state.json', state)
    result = deliver_pending(config, directory, state)
    client._write(directory/'state.json', state)
    return {**result, 'report':entry['payload'], 'delivered':entry['delivered']}, 2 if result['pending'] else 0


def deliver_pending(config, directory, state):
    # ponytail: serial delivery within a session; split queues if checkpoint volume warrants it.
    result = {'confirmed':0, 'pending':0, 'errors':[]}
    for run in state.get('runs', {}).values():
        for pending in sorted(run['reports'].values(), key=lambda item:item['payload']['sequence']):
            if not pending['delivered']:
                result = client.publish(directory/'queue', config['db_path'], config['clio'], pending['payload'])
                if result['pending']:
                    result['systemMessage'] = 'Work reporting is incomplete: delivery is queued. Retry the same report or let the next hook retry it.'
                    return result
                pending['delivered'] = True
    latest = state.get('latest')
    if latest:
        latest['delivered'] = state['runs'][latest['run_id']]['reports'][latest['key']]['delivered']
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', type=Path, required=True)
    parser.add_argument('--source', choices=['codex','claude'], required=True)
    parser.add_argument('action', choices=['hook','report'])
    parser.add_argument('--session')
    parser.add_argument('--cwd')
    parser.add_argument('--checkpoint')
    args = parser.parse_args()
    data = read_input()
    if args.action == 'hook':
        args.session, args.cwd = data.get('session_id'), data.get('cwd')
    cwd = absolute('cwd', args.cwd)
    config, project = settings(args.config, cwd)
    if project is None:
        if args.action == 'hook':
            print('{}')
            return 0
        raise ValueError('Working directory is outside the configured projects')
    text('session', args.session)
    # The short root lock pins the destination; each session has its own delivery lock.
    with client._locked(config['state_dir'], config['db_path']):
        pass
    identity = digest([args.source, args.session, str(cwd)])
    with client._locked(config['state_dir']/identity, config['db_path']) as (directory, _):
        path = directory/'state.json'
        state = json.loads(path.read_text()) if path.exists() else {}
        if args.action == 'hook':
            delivery = deliver_pending(config, directory, state)
            result = hook(args, data, cwd, state)
            if delivery.get('systemMessage'):
                result.setdefault('systemMessage', delivery['systemMessage'])
            client._write(path, state)
            status = 0
        else:
            result, status = report(args, data, cwd, config, project, directory, state)
    print(json.dumps(result))
    return status


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (ValueError, OSError, KeyError, TypeError, subprocess.TimeoutExpired) as error:
        if 'hook' in sys.argv[1:]:
            print(json.dumps({'systemMessage':f'Work reporting is incomplete: {error}. The task can continue; reporting is unverified.'}))
            sys.exit(0)
        print(f'Work reporting: {error}', file=sys.stderr)
        sys.exit(1)

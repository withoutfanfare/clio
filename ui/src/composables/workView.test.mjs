import '../../test/setup.mjs';
import assert from 'node:assert/strict';
import test from 'node:test';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { mountSetup } from '../../test/setup.mjs';
const { default: WorkView } = await import('../views/WorkView.vue');

function run(id, nextActor = 'agent', superseded = false) {
  return {
    superseded, stale: false,
    receipt: { id, received_at: 1000, report: {
      source: 'test', run_id: `run-${id}`, session_id: `session-${id}`,
      state: 'running', next_actor: nextActor, next_step: 'Check the result',
      evidence: [], evidence_status: 'current',
    } },
  };
}

test('projects count unique tasks, retain concurrent runs, and include user requests within conflicts', async t => {
  const tasks = [
    { project: 'Clio', task: 'one', task_title: 'Concurrent work', state: 'conflict', next_actor: 'other', stale: false, source_unavailable: false, runs: [run(1), run(2, 'user')] },
    { project: 'Clio', task: 'two', task_title: 'Implemented work', state: 'implemented', next_actor: 'agent', stale: false, source_unavailable: false, runs: [run(3, 'user', true), run(4)] },
    { project: 'Other', task: 'one', task_title: 'Stale work', state: 'reporting_missing', next_actor: 'agent', stale: true, source_unavailable: true, runs: [run(5)] },
  ];
  mockIPC(command => { assert.equal(command, 'cmd_work_overview'); return { tasks }; });
  const mounted = mountSetup(() => WorkView.setup({}, { expose() {} }));
  t.after(() => { mounted.unmount(); clearMocks(); });
  await new Promise(resolve => setImmediate(resolve));
  const view = mounted.state;
  assert.equal(view.projects.value.length, 2);
  assert.equal(view.orderedTasks.value.filter(task => task.project === 'Clio').length, 2);
  assert.equal(view.needsCount.value, 1, 'a superseded request cannot increase the count');
  assert.equal(view.uncertainCount.value, 2, 'stale and missing evidence count as one uncertain task');
  assert.equal(view.activeRuns(tasks[0]).length, 2);
  assert.match(view.nextStep(tasks[0]), /Resolve the differing reports/);
  assert.equal(view.stateLabels.implemented, 'Ready for review');
  assert.equal(view.stateLabels.conflict, "Conflicting reports");
});

test('evidence keeps current runs before collapsed history with historical action labels', async t => {
  const task = { project: 'Clio', task: 'handover', task_title: 'Handover', state: 'running', next_actor: 'agent', runs: [run(1, 'user', true), run(2), run(3), run(4, 'agent', true)] };
  mockIPC(() => ({ tasks: [task] }));
  const mounted = mountSetup(() => WorkView.setup({}, { expose() {} }));
  t.after(() => { mounted.unmount(); clearMocks(); });
  await new Promise(resolve => setImmediate(resolve));
  const html = await renderView(mounted.state);
  const history = html.match(/<details\b([^>]*)>\s*<summary>Previous runs \(2\)<\/summary>([\s\S]*?)<\/details>/);
  assert.ok(history, 'superseded runs belong in a separate Previous runs disclosure');
  assert.doesNotMatch(history[1], /\bopen\b/, 'history starts collapsed');
  const current = html.slice(0, html.indexOf(history[0]));
  assert.match(current, /session-2/);
  assert.match(current, /session-3/, 'all current runs precede history');
  assert.doesNotMatch(current, /session-[14]/);
  assert.match(history[2], /session-1/);
  assert.match(history[2], /session-4/);
  assert.doesNotMatch(history[2], /session-[23]/);
  assert.equal((html.match(/Previous next step ·/g) || []).length, 2);
  task.runs = task.runs.filter(item => !item.superseded);
  assert.doesNotMatch(await renderView(mounted.state), /Previous runs|Previous next step/);
});

async function renderView(state) {
  const { readFileSync } = await import('node:fs');
  const { parse } = await import('@vue/compiler-sfc');
  const { compile } = await import('@vue/compiler-dom');
  const Vue = await import('vue');
  const { renderToString } = await import('@vue/server-renderer');
  const { descriptor } = parse(readFileSync(new URL('../views/WorkView.vue', import.meta.url), 'utf8'));
  const render = new Function('Vue', compile(descriptor.template.content, { mode: 'function' }).code)(Vue);
    const app = Vue.createSSRApp({ render: () => render(Vue.proxyRefs(state), []) });
    for (const name of ['SHeading', 'SButton', 'SBadge']) app.component(name, { inheritAttrs: false, render() { return this.$slots.default?.(); } });
    return renderToString(app);
}

test('task report age follows current observations rather than receipt or polling time', async t => {
  t.mock.timers.enable({ apis: ['Date', 'setTimeout'], now: 1_000_000 });
  const current = run(1), older = run(2), historical = run(3, 'agent', true);
  current.receipt.report.observed_at = 880;
  older.receipt.report.observed_at = 700;
  historical.receipt.report.observed_at = 1000;
  const task = { project: 'Clio', task: 'age', task_title: 'Report age', state: 'running', next_actor: 'agent', runs: [historical, older, current] };
  let fail = false;
  mockIPC(() => { if (fail) throw Error('Unavailable'); return { tasks: [task] }; });
  const mounted = mountSetup(() => WorkView.setup({}, { expose() {} }));
  t.after(() => { mounted.unmount(); clearMocks(); });
  await new Promise(resolve => setImmediate(resolve));
  const card = async () => (await renderView(mounted.state)).split('<details class="provenance">')[0];
  assert.match(await card(), /Latest report 2 minutes ago/);
  assert.match(await card(), /Dashboard checked/);
  t.mock.timers.tick(60_000);
  await new Promise(resolve => setImmediate(resolve));
  assert.match(await card(), /Latest report 3 minutes ago/, 'unchanged reports age through polling');
  fail = true;
  t.mock.timers.tick(60_000);
  await new Promise(resolve => setImmediate(resolve));
  assert.match(await card(), /Latest report 4 minutes ago/, 'failed refresh cannot freeze report age');
  t.mock.timers.setTime(1_200_000);
  for (const [observed, expected] of [[1190, /Latest report just now/], [0, /Latest report 20 minutes ago/], [-2400, /Latest report 1 hour ago/], [-85200, /Latest report 1 day ago/], [1201, /Report time is in the future/]]) {
    task.runs = [current];
    current.receipt.report.observed_at = observed;
    assert.match(await card(), expected);
  }
  task.runs = [historical];
  assert.match(await card(), /Report time unavailable/);
});

test('accepted work drops review attention while guidance stays a copyable recommendation', async t => {
  const acceptance = { receipt_id: 1, scope: 'Item 3 only', accepted_by: 'Danny', accepted_at: 1000, evidence: ['checks.md#human-acceptance'] };
  const recommendation = { project: 'Clio', parent_task: 'one', task: 'freshness', task_title: 'Show report age', reason: 'Polling time is not report time', next_actor: 'user', prompt: 'Implement report age from the existing freshness finding.', evidence: ['checks.md#item-2'], checked_at: 1100 };
  const accepted = { ...run(1, 'user'), accepted: true };
  const task = { project: 'Clio', task: 'one', task_title: 'Reporting', state: 'accepted', next_actor: 'none', runs: [accepted, run(2, 'user', true)], acceptances: [acceptance], recommendation };
  let copied, failCopy = false;
  mockIPC((command, args) => {
    if (command === 'cmd_copy_to_clipboard') { if (failCopy) throw new Error('Clipboard unavailable'); copied = args.text; return; }
    assert.equal(command, 'cmd_work_overview'); return { tasks: [task] };
  });
  const mounted = mountSetup(() => WorkView.setup({}, { expose() {} }));
  t.after(() => { mounted.unmount(); clearMocks(); });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(mounted.state.needsCount.value, 0);
  assert.doesNotMatch(mounted.state.nextStep(task), /Check the result/);
  const html = await renderView(mounted.state);
  assert.match(html, /Accepted.*Item 3 only.*Danny/s);
  assert.match(html, /Recommendation only.*not authorised or started/s);
  assert.match(html, /Danny acts next/);
  assert.match(html, /Show report age/);
  assert.match(html, /textarea[^>]*readonly[^>]*>[\s\S]*Implement report age/);
  assert.match(html, /Recommendation evidence/);
  await mounted.state.copyPrompt(recommendation);
  assert.equal(copied, recommendation.prompt);
  assert.match(mounted.state.copyStatus.value.message, /Copied/);
  failCopy = true;
  await mounted.state.copyPrompt(recommendation);
  assert.match(mounted.state.copyStatus.value.message, /Could not copy/);
  await mounted.state.refresh();
  assert.equal(mounted.state.needsCount.value, 0);
  assert.equal(mounted.state.overview.value.tasks.length, 1);
});

test('selected details show the current summary and honest next-action ownership', async t => {
  const current = run(1), historical = run(2, 'user', true), accepted = { ...run(3, 'user'), accepted: true };
  current.receipt.report.summary = 'Current scoped progress';
  historical.receipt.report.summary = 'Obsolete progress';
  accepted.receipt.report.summary = 'Already accepted progress';
  const task = { project: 'Clio', task: 'clarity', task_title: 'Clarity', state: 'running', next_actor: 'agent', runs: [historical, accepted, current] };
  mockIPC(() => ({ tasks: [task] }));
  const mounted = mountSetup(() => WorkView.setup({}, { expose() {} }));
  t.after(() => { mounted.unmount(); clearMocks(); });
  await new Promise(resolve => setImmediate(resolve));
  const summary = async () => (await renderView(mounted.state)).split('<section class="task-inspector"')[1].split('<details class="provenance">')[0];
  for (const [actor, label] of [['agent', 'Agent next'], ['user', 'Danny next'], ['other', 'Other next'], ['none', 'No action required'], [undefined, 'Next owner unresolved']]) {
    task.next_actor = actor;
    current.receipt.report.next_actor = actor;
    const html = await summary();
    assert.match(html, /Current scoped progress/);
    assert.ok(html.includes(label), label);
    assert.doesNotMatch(html, /Obsolete progress|Already accepted progress|Suggested next/);
  }
  task.state = 'conflict'; task.next_actor = 'other';
  current.receipt.report.next_actor = 'agent'; task.runs.push(run(4, 'user'));
  const conflict = await summary();
  assert.match(conflict, /Conflicting reports/);
  assert.match(conflict, /Next owner unresolved/);
  assert.match(conflict, /Resolve the differing reports/);
  assert.doesNotMatch(conflict, /Current scoped progress|Agent next|Danny next|Other next|No action required/);
  task.state = 'accepted'; task.next_actor = 'none'; task.runs = [accepted, historical];
  assert.match(await summary(), /human acceptance/);
  assert.match(await summary(), /No action required/);
  task.state = 'running'; task.next_actor = undefined; task.runs = [];
  assert.match(await summary(), /No summary reported/);
  assert.match(await summary(), /Next owner unresolved/);
});

test('only native-validated evidence offers Open and failures remain visible', async t => {
  const current = run(7);
  current.receipt.report.evidence = ['checks.md', 'https://example.test', '../outside.txt'];
  current.openable_evidence = ['checks.md'];
  const task = { project: 'Clio', task: 'open', task_title: 'Open evidence', state: 'running', next_actor: 'agent', runs: [current] };
  let opened, fail = false;
  mockIPC((command, args) => {
    if (command === 'cmd_open_work_evidence') { if (fail) throw Error('File removed'); opened = args; return; }
    assert.equal(command, 'cmd_work_overview'); return { tasks: [task] };
  });
  const mounted = mountSetup(() => WorkView.setup({}, { expose() {} }));
  t.after(() => { mounted.unmount(); clearMocks(); });
  await new Promise(resolve => setImmediate(resolve));
  const html = await renderView(mounted.state);
  assert.equal((html.match(/>Open</g) || []).length, 1);
  assert.match(html.replace(/<!--[\s\S]*?-->/g, ''), /<li>https:\/\/example.test<\/li>/);
  assert.match(html.replace(/<!--[\s\S]*?-->/g, ''), /<li>..\/outside.txt<\/li>/);
  await mounted.state.openEvidence(7, 'checks.md');
  assert.deepEqual(opened, { receiptId: 7, reference: 'checks.md' });
  fail = true;
  await mounted.state.openEvidence(7, 'checks.md');
  assert.match(await renderView(mounted.state), /Could not open/);
});

test('orientation exposes started work and review saves only after explicit confirmation', async t => {
  const current = run(20, 'user');
  Object.assign(current.receipt.report, { state: 'implemented', task_title: 'Evidence opening', summary: 'Open a local file', observed_at: 1000, evidence: ['checks.md'] });
  current.openable_evidence = ['checks.md'];
  const task = { project: 'Clio', task: 'review', task_title: 'Evidence opening', state: 'implemented', next_actor: 'user', runs: [current] };
  const active = { project: 'Clio', task: 'active', task_title: 'Active change', state: 'running', next_actor: 'agent', runs: [run(21)] };
  let writes = 0, fail = false;
  mockIPC((command, args) => {
    if (command === 'cmd_accept_work_change') {
      writes++; assert.deepEqual(args, { receiptId: 20 });
      if (fail) throw Error('Report changed');
      current.accepted = true; task.state = 'accepted'; task.next_actor = 'none';
      return { receipt_id: 20, scope: 'Evidence opening', accepted_by: 'Danny' };
    }
    assert.equal(command, 'cmd_work_overview'); return { tasks: [task, active], can_accept: true };
  });
  const mounted = mountSetup(() => WorkView.setup({}, { expose() {} }));
  t.after(() => { mounted.unmount(); clearMocks(); });
  await new Promise(resolve => setImmediate(resolve));
  const view = mounted.state;
  const initial = await renderView(view);
  assert.match(initial, /Work is in progress/);
  assert.match(initial, /1<\/strong> ready for review/);
  assert.match(initial, /Newest first · report time/);
  assert.match(initial, /Review change/);
  assert.equal(view.nextStep(task), 'Check the result, then accept this change here.');
  view.startReview(task, current);
  assert.equal(writes, 0, 'opening a review never accepts');
  assert.match(await renderView(view), /Accept this change/);
  view.cancelReview(); assert.equal(view.review.value, null); assert.equal(writes, 0);
  view.startReview(task, current);
  const reviewedId = current.receipt.id;
  current.receipt.id = 22;
  assert.equal(view.canConfirmReview.value, false, 'refresh cannot silently retarget a review');
  await view.acceptReview(); assert.equal(writes, 0);
  current.receipt.id = reviewedId;
  await view.refresh();
  fail = true; await view.acceptReview();
  assert.match(await renderView(view), /Could not save acceptance/);
  assert.equal(current.accepted, undefined);
  fail = false; await view.acceptReview();
  assert.equal(writes, 2);
  assert.equal(view.review.value, null);
  assert.equal(view.needsCount.value, 0);
  assert.match(await renderView(view), /Acceptance saved/);
  assert.doesNotMatch(await renderView(view), /Review change/);
});

test('review stays unavailable remotely and double confirmation cannot duplicate a save', async t => {
  const current = run(30);
  current.receipt.report.state = 'implemented';
  const task = { project: 'Clio', task: 'guard', task_title: 'Guarded review', state: 'implemented', next_actor: 'agent', stale: true, runs: [current] };
  let local = false, writes = 0, release;
  mockIPC((command) => {
    if (command === 'cmd_accept_work_change') { writes++; return new Promise(resolve => { release = resolve; }); }
    return { tasks: [task], can_accept: local };
  });
  const mounted = mountSetup(() => WorkView.setup({}, { expose() {} }));
  t.after(() => { mounted.unmount(); clearMocks(); });
  await new Promise(resolve => setImmediate(resolve));
  const view = mounted.state;
  view.startReview(task, current);
  assert.equal(view.review.value, null);
  assert.equal(view.uncertainCount.value, 0, 'an old implemented observation is not stalled work');
  assert.equal(view.needsCount.value, 1, 'reviewable changes need Danny even if an agent reported another next owner');
  assert.doesNotMatch(await renderView(view), /Choose.*Review change|Needs a status check/);
  local = true; await view.refresh();
  view.startReview(task, current);
  const first = view.acceptReview();
  await new Promise(resolve => setImmediate(resolve));
  await view.acceptReview();
  view.cancelReview();
  assert.ok(view.review.value, 'cannot cancel a write in flight');
  assert.equal(writes, 1);
  release({}); await first;
});


test('chronology uses current observation time across projects and selection survives refresh', async t => {
  const make = (project, task, time) => {
    const current = run(task); current.receipt.report.observed_at = time;
    current.receipt.report.summary = `Summary for ${task}`;
    return { project, task, task_title: task, state: 'running', next_actor: 'agent', runs: [current] };
  };
  const old = make('Z', 'old', 100), newest = make('A', 'newest', 300), missing = make('A', 'missing', undefined);
  const history = run('historical', 'user', true); history.receipt.report.observed_at = 900; old.runs.push(history);
  let tasks = [old, missing, newest];
  mockIPC(() => ({ tasks }));
  const mounted = mountSetup(() => WorkView.setup({}, { expose() {} }));
  t.after(() => { mounted.unmount(); clearMocks(); });
  await new Promise(resolve => setImmediate(resolve));
  const view = mounted.state;
  assert.deepEqual(view.orderedTasks.value.map(task => task.task), ['newest', 'old', 'missing']);
  assert.equal(view.selectedTask.value.task, 'newest');
  view.selectTask(old);
  tasks = [make('B', 'just arrived', 400), ...tasks.map(task => ({ ...task }))];
  await view.refresh();
  assert.equal(view.orderedTasks.value[0].task, 'just arrived');
  assert.equal(view.selectedTask.value.task, 'old', 'polling never jumps away from the selected task');
  const html = await renderView(view);
  const list = html.split('<ul class="task-list"')[1].split('</ul>')[0];
  assert.doesNotMatch(list, /Summary for/);
  assert.match(list, /datetime=/);
  assert.match(html, /Summary for old/);
  assert.doesNotMatch(html, /Summary for newest/);
  view.changeFilter('needs-you'); await new Promise(resolve => setImmediate(resolve));
  assert.equal(view.visibleTasks.value.length, 0, 'historical requests do not need action');
  assert.equal(view.selectedTask.value, null);
  view.changeFilter('all'); await new Promise(resolve => setImmediate(resolve));
  view.projectFilter.value = 'Z'; await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(view.visibleTasks.value.map(task => task.task), ['old']);
  assert.equal(view.selectedTask.value.task, 'old');
  assert.equal(view.taskSummary({ ...old, runs: [old.runs[0], newest.runs[0]] }), 'Summary for newest');
});


test('accepting the last filtered task returns narrow layouts to navigation', async t => {
  const current = run(50, 'user'); current.receipt.report.state = 'implemented';
  const task = { project: 'Clio', task: 'last', task_title: 'Last review', state: 'implemented', next_actor: 'user', runs: [current] };
  mockIPC(command => {
    if (command === 'cmd_accept_work_change') { task.state = 'accepted'; current.accepted = true; return {}; }
    return { tasks: [task], can_accept: true };
  });
  const mounted = mountSetup(() => WorkView.setup({}, { expose() {} }));
  t.after(() => { mounted.unmount(); clearMocks(); });
  await new Promise(resolve => setImmediate(resolve));
  const view = mounted.state;
  view.changeFilter('needs-you'); view.selectTask(task); view.startReview(task, current);
  await view.acceptReview(); await new Promise(resolve => setImmediate(resolve));
  assert.equal(view.selectedTask.value, null);
  assert.equal(view.mobileDetail.value, false);
  view.changeFilter('all'); task.state = 'running'; task.stale = true;
  await view.refresh();
  const html = await renderView(view);
  assert.equal((html.match(/Needs a status check/g) || []).length, 2, 'both row and details identify stale work');
});

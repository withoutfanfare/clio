import '../../test/setup.mjs';
import assert from 'node:assert/strict';
import test, { beforeEach } from 'node:test';
import { createPinia, setActivePinia } from 'pinia';
import { nextTick } from 'vue';
import { routerKey } from 'vue-router';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { mountSetup, memory } from '../../test/setup.mjs';
const { default: AttentionView } = await import('../views/AttentionView.vue');
const { useMemoryStore } = await import('../stores/memories.ts');
beforeEach(() => { localStorage.clear(); setActivePinia(createPinia()); });
const item = { id: 'attention-one', memory_id: 'memory-full-id', namespace: 'clio', status: 'open', reason: 'overdue', created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z' };
function overview(titles = { 'memory-full-id': 'Decide the next release' }) { return { eligible: [item], open: [item], memory_titles: titles, review_pending: 12, consolidation_stale: false, generated_at: '2026-09-04T12:00:00Z' }; }
async function setup(t, handler = command => command === 'cmd_attention_overview' ? overview() : null) {
  mockIPC(handler);
  const routes = [];
  const mounted = mountSetup(() => AttentionView.setup({}, { expose() {} }), [[routerKey, { push: route => routes.push(route) }]]);
  let active = true;
  const unmount = () => { if (active) { active = false; mounted.unmount(); } };
  t.after(() => { unmount(); clearMocks(); });
  await new Promise(resolve => setImmediate(resolve));
  return { view: mounted.state, store: useMemoryStore(), routes, unmount };
}

test('attention names use the eligible full-ID map and evidence opens without changing route', async t => {
  const { view, store, routes } = await setup(t);
  const opened = [];
  store.openDrawer = async (...args) => { opened.push(args); return true; };
  assert.equal(typeof view.memoryTitle, 'function');
  assert.equal(view.memoryTitle(item), 'Decide the next release');
  await view.viewEvidence(item);
  assert.deepEqual(routes, []);
  assert.equal(opened.length, 1);
  assert.equal(opened[0][0], 'memory-full-id');
  assert.equal(opened[0][1].eligibleOnly, true);
  assert.equal(opened[0][1].isCurrent(), true);
});

test('unavailable or unsupported evidence never opens an archived or unknown memory', async t => {
  const { view, store } = await setup(t, command => command === 'cmd_attention_overview' ? overview({}) : null);
  let opened = false;
  store.openDrawer = async () => { opened = true; return true; };
  await view.viewEvidence(item);
  assert.equal(opened, false);
  assert.equal(view.canViewEvidence(item), false);
  view.overview.value = { ...overview(), memory_titles: undefined };
  assert.equal(view.titlesSupported.value, false);
  assert.match(view.memoryTitle(item), /unavailable|unsupported/i);
});

test('local queue unknown counts remain unknown instead of becoming zero', async t => {
  const { view } = await setup(t, command => command === 'cmd_attention_overview' ? overview() : { pending: null, processing: 0, dead: null, oldest_pending_age_secs: null, scope: 'local', checked_at: '2026-09-04T12:00:00Z', unavailable_buckets: ['pending', 'dead'] });
  assert.equal(typeof view.countLabel, 'function');
  assert.equal(view.countLabel(null), 'Unknown');
  assert.equal(view.countLabel(0), '0');
  assert.equal(view.queueWaiting.value, null);
  assert.match(view.diagnostics.value, /"scope": "local"/);
});

test('namespace changes reload attention and a failed fresh scope clears old results', async t => {
  let fail = false;
  const { view, store } = await setup(t, command => {
    if (command === 'cmd_capture_queue_health') return null;
    if (fail) throw new Error('new scope unavailable');
    return overview();
  });
  assert.ok(view.overview.value);
  fail = true;
  store.selectedNamespace = 'another';
  await nextTick();
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(view.overview.value, null);
  assert.match(view.error.value, /new scope unavailable/);
});

test('a late attention response cannot replace the current namespace', async t => {
  const pending = new Map();
  const { view, store } = await setup(t, (command, args) => {
    if (command === 'cmd_capture_queue_health') return null;
    if (!args.namespace) return overview();
    return new Promise(resolve => pending.set(args.namespace, resolve));
  });
  store.selectedNamespace = 'first';
  await nextTick();
  store.selectedNamespace = 'second';
  await nextTick();
  pending.get('second')(overview({ 'memory-full-id': 'Current project' }));
  await new Promise(resolve => setImmediate(resolve));
  pending.get('first')(overview({ 'memory-full-id': 'Previous project' }));
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(view.memoryTitle(item), 'Current project');
  assert.equal(view.loading.value, false);
});

test('evidence which becomes unavailable shows a safe failure without navigation', async t => {
  const { view, store, routes } = await setup(t);
  store.openDrawer = async () => false;
  await view.viewEvidence(item);
  assert.match(view.actionError.value, /could not be opened/);
  assert.deepEqual(routes, []);
});

test('repeated complete requests submit only once while confirmation is pending', async t => {
  let finish;
  let calls = 0;
  const { view } = await setup(t, command => {
    if (command === 'cmd_action_complete') { calls++; return new Promise(resolve => { finish = () => resolve({ ...item, status: 'resolved' }); }); }
    return command === 'cmd_attention_overview' ? overview() : null;
  });
  const first = view.complete(item);
  const second = view.complete(item);
  assert.equal(calls, 1);
  finish();
  await Promise.all([first, second]);
});

for (const leave of ['namespace', 'unmount']) {
  test(`late evidence cannot open a drawer after attention ${leave}`, async t => {
    let finish;
    const { view, store, unmount } = await setup(t, command => {
      if (command === 'cmd_get') return new Promise(resolve => { finish = resolve; });
      return command === 'cmd_attention_overview' ? overview() : null;
    });
    const opening = view.viewEvidence(item);
    if (leave === 'namespace') store.setNamespace('another');
    else unmount();
    finish(memory({ id: item.memory_id, namespace: 'clio' }));
    await opening;
    assert.equal(store.drawerOpen, false);
    assert.equal(store.drawerMemory, null);
  });
}

test('late failed evidence does not overwrite the current screen error', async t => {
  let fail;
  const { view, store, unmount } = await setup(t, command => {
    if (command === 'cmd_get') return new Promise((_resolve, reject) => { fail = reject; });
    return command === 'cmd_attention_overview' ? overview() : null;
  });
  const opening = view.viewEvidence(item);
  unmount();
  store.error = 'Current screen error';
  fail(new Error('Old evidence failure'));
  await opening;
  assert.equal(store.error, 'Current screen error');
});

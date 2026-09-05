import '../../test/setup.mjs';
import assert from 'node:assert/strict';
import test, { beforeEach } from 'node:test';
import { createPinia, setActivePinia } from 'pinia';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { mountSetup, memory } from '../../test/setup.mjs';
const { default: InboxView } = await import('../views/InboxView.vue');
beforeEach(() => { localStorage.clear(); setActivePinia(createPinia()); });
function review(overrides = {}) { return { id: 'review-one', content: 'Original captured text', suggested_namespace: 'clio', suggested_kind: 'custom-kind', suggested_title: 'Review this', suggested_summary: null, suggested_tags: ['original'], suggested_importance: 3, suggested_confidence: 0.4, status: 'pending', created_at: '2026-09-04T12:00:00Z', source_route: 'desktop', source_ref: null, metadata: {}, reviewed_at: null, ...overrides }; }
async function setup(t, handler, withMount = false) {
  assert.ok(InboxView, 'An actionable review inbox is required');
  mockIPC(handler);
  const mounted = mountSetup(() => InboxView.setup({}, { expose() {} }));
  let active = true;
  const unmount = () => { if (active) { mounted.unmount(); active = false; } };
  t.after(() => { unmount(); clearMocks(); });
  await new Promise(resolve => setImmediate(resolve));
  mounted.state.selectItem(mounted.state.items.value[0]);
  return withMount ? { view: mounted.state, unmount } : mounted.state;
}

test('failed inbox approval retains the selected item and error', async t => {
  const view = await setup(t, command => { if (command === 'cmd_inbox_list') return [review()]; throw new Error('offline'); });
  await view.approve();
  assert.equal(view.items.value.length, 1);
  assert.equal(view.selected.value.id, 'review-one');
  assert.match(view.error.value, /offline/);
});

test('suggestions are saved before approval and the confirmed server destination is shown', async t => {
  let saved = review();
  let resolved = false;
  const calls = [];
  const view = await setup(t, (command, args) => {
    calls.push(command);
    if (command === 'cmd_inbox_list') return resolved ? [] : [saved];
    if (command === 'cmd_inbox_edit') {
      assert.equal(args.reviewId, 'review-one');
      assert.equal(args.namespace, 'explicit');
      saved = review({ suggested_namespace: 'server-confirmed', status: 'edited' });
      return saved;
    }
    if (command === 'cmd_inbox_approve') { assert.equal(saved.suggested_namespace, 'server-confirmed'); resolved = true; return memory({ namespace: saved.suggested_namespace }); }
    throw new Error(command);
  });
  view.draft.value.namespace = 'explicit';
  assert.equal(view.dirty.value, true);
  await view.approve();
  assert.equal(calls.includes('cmd_inbox_approve'), false);
  assert.equal(await view.saveSuggestions(), true);
  assert.equal(view.draft.value.namespace, 'server-confirmed');
  assert.equal(view.dirty.value, false);
  await view.approve();
  assert.equal(view.items.value.length, 0);
  assert.ok(calls.indexOf('cmd_inbox_edit') < calls.indexOf('cmd_inbox_approve'));
});

test('failed suggestion edits preserve the draft and block approval', async t => {
  const view = await setup(t, command => { if (command === 'cmd_inbox_list') return [review()]; throw new Error('save failed'); });
  view.draft.value.title = 'Keep this draft';
  assert.equal(await view.saveSuggestions(), false);
  assert.equal(view.draft.value.title, 'Keep this draft');
  assert.equal(view.dirty.value, true);
  await view.approve();
  assert.equal(view.selected.value.id, 'review-one');
});

test('rejection requires deliberate confirmation and duplicate submissions are ignored', async t => {
  let rejected = false;
  let finish;
  let calls = 0;
  const view = await setup(t, command => {
    if (command === 'cmd_inbox_list') return rejected ? [] : [review()];
    if (command === 'cmd_inbox_reject') { calls++; return new Promise(resolve => { finish = () => { rejected = true; resolve(review({ status: 'rejected' })); }; }); }
    throw new Error(command);
  });
  await view.reject();
  assert.equal(calls, 0);
  assert.equal(view.confirmingReject.value, true);
  const first = view.reject();
  const duplicate = view.reject();
  assert.equal(calls, 1);
  finish();
  await Promise.all([first, duplicate]);
  assert.equal(view.items.value.length, 0);
});

test('confirmed approval refills the bounded inbox from the server', async t => {
  let resolved = false;
  let lists = 0;
  const view = await setup(t, command => {
    if (command === 'cmd_inbox_list') { lists++; return [review(resolved ? { id: 'next-oldest' } : {})]; }
    if (command === 'cmd_inbox_approve') { resolved = true; return memory(); }
    throw new Error(command);
  });
  await view.approve();
  assert.equal(lists, 2);
  assert.deepEqual(view.items.value.map(item => item.id), ['next-oldest']);
  assert.equal(view.selected.value, null);
});

test('clearing an existing title sends an empty string without clearing unchanged nullable fields', async t => {
  const view = await setup(t, (command, args) => {
    if (command === 'cmd_inbox_list') return [review()];
    if (command === 'cmd_inbox_edit') {
      assert.deepEqual(args, { reviewId: 'review-one', title: '' });
      return review({ suggested_title: '', status: 'edited' });
    }
    throw new Error(command);
  });
  view.draft.value.title = '';
  assert.equal(await view.saveSuggestions(), true);
  assert.equal(view.draft.value.title, '');
  assert.equal(view.dirty.value, false);
});

test('failed suggestion drafts recover after leaving the inbox or restarting it', async t => {
  const handler = command => { if (command === 'cmd_inbox_list') return [review()]; throw new Error('offline'); };
  const first = await setup(t, handler, true);
  first.view.draft.value.title = 'Recover this unsaved title';
  await first.view.saveSuggestions();
  first.unmount();
  setActivePinia(createPinia());
  const recovered = await setup(t, handler);
  assert.equal(recovered.draft.value.title, 'Recover this unsaved title');
  assert.equal(recovered.selected.value.id, 'review-one');
  assert.equal(recovered.dirty.value, true);
  assert.match(recovered.recoveryStatus.value, /recover/i);
});

test('a recovery missing from the bounded queue cannot be overwritten by another capture', async t => {
  const first = await setup(t, () => [review()], true);
  first.view.draft.value.namespace = 'unsaved-destination';
  first.unmount();
  const recovered = await setup(t, () => [review({ id: 'another' })]);
  assert.equal(recovered.selectItem(recovered.items.value[0]), false);
  assert.equal(recovered.selected.value, null);
  assert.match(recovered.recoveryStatus.value, /not.*loaded|unavailable/i);
  const stored = localStorage.getItem('clio-inbox-draft');
  assert.match(stored, /unsaved-destination/);
  assert.equal(recovered.discardRecovery(), true);
  assert.equal(localStorage.getItem('clio-inbox-draft'), null);
  assert.equal(recovered.selectItem(recovered.items.value[0]), true);
});

test('unreadable recovery storage is surfaced and blocks overwriting a possible draft', async t => {
  const getItem = localStorage.getItem;
  localStorage.getItem = key => { if (key === 'clio-inbox-draft') throw new Error('denied'); return getItem(key); };
  t.after(() => { localStorage.getItem = getItem; });
  const view = await setup(t, () => [review()]);
  assert.equal(view.selected.value, null);
  assert.match(view.recoveryError.value, /recover|read/i);
  assert.equal(view.selectItem(view.items.value[0]), false);
});

test('confirmed suggestion save clears its local recovery', async t => {
  const view = await setup(t, (command, args) => command === 'cmd_inbox_list' ? [review()] : review({ suggested_title: args.title, status: 'edited' }));
  view.draft.value.title = 'Saved title';
  assert.ok(localStorage.getItem('clio-inbox-draft'));
  assert.equal(await view.saveSuggestions(), true);
  assert.equal(localStorage.getItem('clio-inbox-draft'), null);
});

test('a save completing after navigation cannot erase a newer recovered draft', async t => {
  let finish;
  const first = await setup(t, command => {
    if (command === 'cmd_inbox_list') return [review()];
    return new Promise(resolve => { finish = resolve; });
  }, true);
  first.view.draft.value.title = 'First draft';
  const saving = first.view.saveSuggestions();
  first.unmount();
  const reopened = await setup(t, () => [review()]);
  reopened.draft.value.title = 'Newer draft';
  finish(review({ suggested_title: 'First draft', status: 'edited' }));
  await saving;
  assert.match(localStorage.getItem('clio-inbox-draft'), /Newer draft/);
  assert.equal(reopened.draft.value.title, 'Newer draft');
});

test('unsupported recovery records remain intact until explicitly discarded', async t => {
  const stored = JSON.stringify({ version: 99, draft: { reviewId: 'old-review', edits: { title: 'Keep for recovery' } } });
  localStorage.setItem('clio-inbox-draft', stored);
  const view = await setup(t, () => [review()]);
  assert.equal(view.selected.value, null);
  assert.equal(localStorage.getItem('clio-inbox-draft'), stored);
  assert.equal(view.discardRecovery(), true);
  assert.equal(view.selectItem(view.items.value[0]), true);
});

test('oversized recovery writes retain the in-view draft and show a storage warning', async t => {
  const view = await setup(t, () => [review()]);
  view.draft.value.summary = 'x'.repeat(1_000_001);
  assert.equal(view.draft.value.summary.length, 1_000_001);
  assert.match(view.recoveryError.value, /unavailable.*keep.*open/i);
  assert.equal(localStorage.getItem('clio-inbox-draft'), null);
});

for (const action of ['approve', 'reject']) {
  test(`confirmed ${action} clears matching recovery and allows the next capture`, async t => {
    localStorage.setItem('clio-inbox-draft', JSON.stringify({ version: 1, draft: { reviewId: 'review-one', edits: { title: 'Saved title' } } }));
    let resolved = false;
    const view = await setup(t, command => {
      if (command === 'cmd_inbox_list') return [review(resolved ? { id: 'next-oldest' } : { suggested_title: 'Saved title' })];
      resolved = true;
      return action === 'approve' ? memory() : review({ status: 'rejected' });
    });
    assert.equal(view.dirty.value, false);
    if (action === 'reject') await view.reject();
    await view[action]();
    assert.equal(localStorage.getItem('clio-inbox-draft'), null);
    assert.equal(view.selectItem(view.items.value[0]), true);
    assert.equal(view.selected.value.id, 'next-oldest');
  });
}

test('failed approval keeps recovery even when its suggestions already match the server', async t => {
  const stored = JSON.stringify({ version: 1, draft: { reviewId: 'review-one', edits: { title: 'Saved title' } } });
  localStorage.setItem('clio-inbox-draft', stored);
  const view = await setup(t, command => {
    if (command === 'cmd_inbox_list') return [review({ suggested_title: 'Saved title' })];
    throw new Error('offline');
  });
  await view.approve();
  assert.equal(localStorage.getItem('clio-inbox-draft'), stored);
  assert.equal(view.selected.value.id, 'review-one');
  assert.match(view.error.value, /offline/);
});

test('a decision completing after navigation cannot clear a newer recovery', async t => {
  localStorage.setItem('clio-inbox-draft', JSON.stringify({ version: 1, draft: { reviewId: 'review-one', edits: { title: 'Saved title' } } }));
  let finish;
  const first = await setup(t, command => {
    if (command === 'cmd_inbox_list') return [review({ suggested_title: 'Saved title' })];
    return new Promise(resolve => { finish = resolve; });
  }, true);
  const approving = first.view.approve();
  first.unmount();
  const reopened = await setup(t, () => [review({ suggested_title: 'Saved title' })]);
  reopened.draft.value.title = 'Newer recovery';
  finish(memory());
  await approving;
  assert.match(localStorage.getItem('clio-inbox-draft'), /Newer recovery/);
  assert.equal(reopened.draft.value.title, 'Newer recovery');
});

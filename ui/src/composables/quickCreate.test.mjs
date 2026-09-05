import '../../test/setup.mjs';
import assert from 'node:assert/strict';
import test, { beforeEach } from 'node:test';
beforeEach(() => localStorage.clear());
import { createPinia, setActivePinia } from 'pinia';
import { nextTick } from 'vue';
import { mountSetup } from '../../test/setup.mjs';
const { default: QuickCreate } = await import('../components/QuickCreate.vue');
const { useMemoryStore } = await import('../stores/memories.ts');

function setup(t) {
  setActivePinia(createPinia());
  const store = useMemoryStore();
  const mounted = mountSetup(() => QuickCreate.setup({}, { expose() {} }));
  t.after(mounted.unmount);
  return { store, draft: mounted.state };
}

test('fresh creation prefers selected namespace and retains an explicit open-draft override', async t => {
  const { store, draft } = setup(t);
  store.selectedNamespace = 'workspace';
  store.quickCreateLastNamespace = 'last-used';
  store.composeOpen = true;
  await nextTick();
  assert.equal(draft.namespace.value, 'workspace');
  draft.namespace.value = 'explicit';
  store.selectedNamespace = 'different';
  await nextTick();
  assert.equal(draft.namespace.value, 'explicit');
});

test('fresh creation falls back to last-used namespace then global', async t => {
  const { store, draft } = setup(t);
  store.selectedNamespace = null;
  store.quickCreateLastNamespace = 'last-used';
  store.composeOpen = true;
  await nextTick();
  assert.equal(draft.namespace.value, 'last-used');
  await draft.close();
  store.quickCreateLastNamespace = '';
  store.composeOpen = true;
  await nextTick();
  assert.equal(draft.namespace.value, 'global');
});

test('non-empty creation is retained on close and empty creation dismisses', async t => {
  const { store, draft } = setup(t);
  store.composeOpen = true;
  await nextTick();
  draft.content.value = 'Do not lose this';
  await draft.close();
  await nextTick();
  assert.equal(store.composeOpen, true);
  assert.equal(draft.content.value, 'Do not lose this');
  draft.content.value = '';
  await draft.close();
  assert.equal(store.composeOpen, false);
});

test('creation recovers an unfinished draft after component restart without replacing its destination', async t => {
  const first = setup(t);
  first.store.selectedNamespace = 'workspace';
  first.store.composeOpen = true;
  await nextTick();
  first.draft.namespace.value = 'explicit';
  first.draft.content.value = 'Survives quit';
  await nextTick();
  const reopened = setup(t);
  reopened.store.selectedNamespace = 'different';
  reopened.store.composeOpen = true;
  await nextTick();
  assert.equal(reopened.draft.content.value, 'Survives quit');
  assert.equal(reopened.draft.namespace.value, 'explicit');
});

test('a confirmed queued capture stays successful when default preference storage fails', async t => {
  const {mockIPC,clearMocks}=await import('@tauri-apps/api/mocks');
  const {store,draft}=setup(t);
  let captures=0;
  mockIPC(command=>{if(command==='cmd_capture'){captures++;return{outcome:'Queued'};}return[];});
  t.after(clearMocks);
  store.composeOpen=true;await nextTick();
  draft.mode.value='automatic';draft.content.value='Capture exactly once';
  const original=localStorage.setItem;
  localStorage.setItem=()=>{throw Error('quota');};
  try {await draft.submit();assert.equal(draft.submitError.value,null);assert.equal(store.composeOpen,false);assert.equal(captures,1);}
  finally{localStorage.setItem=original;}
});

for (const [reason, raw] of [
  ['corrupt', '{broken'], ['empty', ''], ['unsupported', JSON.stringify({ version: 2, draft: {} })],
  ['invalid fields', JSON.stringify({ version: 1, draft: { content: 42 } })],
  ['oversized', 'x'.repeat(1_000_001)],
]) {
  test(`${reason} creator recovery cannot be replaced or submitted`, async t => {
    localStorage.setItem('clio-create-draft', raw);
    const { store, draft } = setup(t);
    let creates = 0;
    store.quickCreate = async () => { creates++; };
    store.composeOpen = true;
    await nextTick();
    draft.content.value = 'Replacement';
    await draft.submit();
    assert.equal(creates, 0);
    assert.ok(localStorage.getItem('clio-create-draft') === raw);
    assert.match(draft.recoveryError.value, /recover|read/i);
    await draft.discardAndClose();
    assert.equal(localStorage.getItem('clio-create-draft'), null);
    assert.equal(store.composeOpen, false);
  });
}

test('creator recovery can retry a failed read without changing its retained destination', async t => {
  const stored = { content: 'Protected creation', title: 'Title', namespace: 'original', kind: 'custom', tags: ['tag'], tagInput: '', importance: 4, mode: 'manual' };
  localStorage.setItem('clio-create-draft', JSON.stringify({ version: 1, draft: stored }));
  const getItem = localStorage.getItem;
  let denied = true;
  localStorage.getItem = key => { if (key === 'clio-create-draft' && denied) throw Error('denied'); return getItem(key); };
  t.after(() => { localStorage.getItem = getItem; });
  const { store, draft } = setup(t);
  store.selectedNamespace = 'different';
  await nextTick();
  assert.match(draft.recoveryError.value, /recover|read/i);
  denied = false;
  assert.equal(draft.retryRecovery(), true);
  assert.equal(draft.content.value, stored.content);
  assert.equal(draft.namespace.value, stored.namespace);
  assert.deepEqual(JSON.parse(localStorage.getItem('clio-create-draft')).draft, stored);
});

test('failed creator discard keeps recovery reserved and visible', async t => {
  localStorage.setItem('clio-create-draft', '{broken');
  const { store, draft } = setup(t);
  const removeItem = localStorage.removeItem;
  localStorage.removeItem = () => { throw Error('denied'); };
  t.after(() => { localStorage.removeItem = removeItem; });
  await draft.discardAndClose();
  assert.equal(store.composeOpen, true);
  draft.namespace.value = 'replacement';
  assert.equal(localStorage.getItem('clio-create-draft'), '{broken');
  assert.match(draft.recoveryError.value, /discard|clear/i);
});

test('a confirmed creation whose recovery copy cannot be cleared stays open and blocked', async t => {
  const { store, draft } = setup(t);
  let creates = 0;
  store.quickCreate = async () => { creates++; };
  store.composeOpen = true;
  await nextTick();
  draft.content.value = 'Create exactly once';
  const removeItem = localStorage.removeItem;
  localStorage.removeItem = () => { throw Error('denied'); };
  t.after(() => { localStorage.removeItem = removeItem; });
  await draft.submit();
  assert.equal(creates, 1);
  assert.equal(store.composeOpen, true);
  assert.equal(draft.recoveryBlocked.value, true);
  assert.match(draft.recoveryError.value, /saved.*could not be cleared/i);
  await draft.submit();
  assert.equal(creates, 1);
  assert.equal(draft.canClose(), false);
  localStorage.removeItem = removeItem;
  await draft.discardAndClose();
  assert.equal(localStorage.getItem('clio-create-draft'), null);
  assert.equal(store.composeOpen, false);
});

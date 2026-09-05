import '../../test/setup.mjs';
import assert from 'node:assert/strict';
import test, { beforeEach } from 'node:test';
beforeEach(() => localStorage.clear());
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { mountSetup, memory } from '../../test/setup.mjs';
const { useAutoSave } = await import('./useAutoSave.ts');

function setup(t, update) {
  mockIPC((command, args) => { assert.equal(command, 'cmd_update'); return update(args.patch, args.memoryId); });
  const mounted = mountSetup(() => useAutoSave(10_000));
  t.after(() => { mounted.unmount(); clearMocks(); });
  return mounted.state;
}

test('closing before debounce flushes edits before allowing close', async t => {
  const original = memory();
  const save = setup(t, patch => ({ ...original, ...patch, updated_at: 'version-2' }));
  save.scheduleAutoSave(original, { content: 'Latest draft' });
  assert.equal(typeof save.flush, 'function', 'close needs an awaitable flush');
  assert.equal(await save.flush(), true);
  assert.equal(original.content, 'Latest draft');
  assert.equal(save.dirty.value, false);
});

test('failure keeps the draft dirty and retry retains the original conflict token', async t => {
  const original = memory();
  let failed = true;
  const patches = [];
  const save = setup(t, patch => {
    patches.push(patch);
    if (failed) throw new Error('conflict: modified elsewhere');
    return { ...original, ...patch, updated_at: 'version-2' };
  });
  save.scheduleAutoSave(original, { title: 'Retain me', content: 'Draft' });
  assert.equal(typeof save.flush, 'function');
  assert.equal(await save.flush(), false);
  assert.equal(save.dirty.value, true);
  assert.match(save.error.value, /conflict/);
  original.updated_at = 'external-version';
  failed = false;
  assert.equal(await save.flush(), true);
  assert.equal(patches[1].expected_updated_at, 'version-1');
  assert.equal(patches[1].title, 'Retain me');
});

test('flush waits for edits entered during the first save and advances only its own version', async t => {
  const original = memory();
  let finish;
  let calls = 0;
  const save = setup(t, patch => {
    calls++;
    if (calls === 1) return new Promise(resolve => { finish = () => resolve({ ...original, ...patch, updated_at: 'version-2' }); });
    assert.equal(patch.expected_updated_at, 'version-2');
    return { ...original, ...patch, updated_at: 'version-3' };
  });
  save.scheduleAutoSave(original, { content: 'First edit' });
  assert.equal(typeof save.flush, 'function');
  const closing = save.flush();
  save.scheduleAutoSave(original, { content: 'Second edit', title: 'New title' });
  finish();
  assert.equal(await closing, true);
  assert.equal(calls, 2);
  assert.equal(original.content, 'Second edit');
  assert.equal(original.title, 'New title');
  assert.equal(save.dirty.value, false);
});

test('explicit discard is refused during an in-flight save', async t => {
  const original = memory();
  let finish;
  const save = setup(t, patch => new Promise(resolve => { finish = () => resolve({ ...original, ...patch }); }));
  save.scheduleAutoSave(original, { content: 'Draft' });
  assert.equal(typeof save.flush, 'function');
  const closing = save.flush();
  assert.equal(save.discard(), false);
  finish();
  await closing;
});

test('reopening restores a pending draft with its original conflict token', async t => {
  const original = memory();
  const save = setup(t, patch => {
    assert.equal(patch.expected_updated_at, 'version-1');
    return { ...original, ...patch, updated_at: 'version-3' };
  });
  save.scheduleAutoSave(original, { content: 'Survives quit', title: 'Draft title' });
  const reopened = mountSetup(() => useAutoSave(10_000));
  t.after(reopened.unmount);
  assert.equal(typeof reopened.state.restoreDraft, 'function');
  const restored = reopened.state.restoreDraft(memory({ updated_at: 'version-2' }));
  assert.equal(restored.content, 'Survives quit');
  assert.equal(restored.title, 'Draft title');
  assert.equal(reopened.state.dirty.value, true);
  assert.match(reopened.state.error.value, /recovered/i);
  assert.equal(await reopened.state.flush(), true);
});

test('an unresolved recovered draft cannot be overwritten by a different memory', t => {
  const original={version:1,draft:{memoryId:'recover-me',expectedUpdatedAt:'old',updates:{content:'Original unsaved draft'}}};
  localStorage.setItem('clio-editor-draft',JSON.stringify(original));
  const save=setup(t,()=>{});
  assert.equal(save.scheduleAutoSave(memory({id:'another'}),{content:'Other content'}),false);
  assert.deepEqual(JSON.parse(localStorage.getItem('clio-editor-draft')),original);
  assert.equal(save.discard(),true);
  assert.equal(save.scheduleAutoSave(memory({id:'another'}),{content:'Other content'}),true);
});

for (const [reason, raw] of [
  ['corrupt', '{broken'], ['empty', ''], ['unsupported', JSON.stringify({ version: 2, draft: {} })],
  ['invalid fields', JSON.stringify({ version: 1, draft: { memoryId: 42, updates: {} } })],
  ['empty memory ID', JSON.stringify({ version: 1, draft: { memoryId: '', expectedUpdatedAt: 'old', updates: { content: 'Original draft' } } })],
  ['oversized', 'x'.repeat(1_000_001)],
]) {
  test(`${reason} editor recovery is reserved until explicit discard`, async t => {
    localStorage.setItem('clio-editor-draft', raw);
    let updates = 0;
    const save = setup(t, () => { updates++; return memory(); });
    assert.equal(save.scheduleAutoSave(memory(), { content: 'Replacement' }), false);
    assert.equal(await save.flush(), false);
    assert.equal(updates, 0);
    assert.ok(localStorage.getItem('clio-editor-draft') === raw);
    assert.match(save.recoveryError.value, /recover|read/i);
    assert.equal(save.discard(), true);
    assert.equal(localStorage.getItem('clio-editor-draft'), null);
    assert.equal(save.scheduleAutoSave(memory(), { content: 'Permitted new draft' }), true);
  });
}

test('an unreadable editor draft can be retried without losing its original conflict token', async t => {
  const stored = JSON.stringify({ version: 1, draft: { memoryId: 'one', expectedUpdatedAt: 'original-version', updates: { content: 'Protected content' } } });
  localStorage.setItem('clio-editor-draft', stored);
  const getItem = localStorage.getItem;
  let denied = true;
  localStorage.getItem = key => { if (key === 'clio-editor-draft' && denied) throw Error('denied'); return getItem(key); };
  t.after(() => { localStorage.getItem = getItem; });
  const save = setup(t, patch => { assert.equal(patch.expected_updated_at, 'original-version'); return memory(patch); });
  assert.equal(save.scheduleAutoSave(memory(), { content: 'Replacement' }), false);
  denied = false;
  assert.equal(save.retryRecovery(), true);
  assert.equal(save.restoreDraft(memory()).content, 'Protected content');
  assert.equal(await save.flush(), true);
});

test('failed removal cannot release the editor recovery reservation', t => {
  localStorage.setItem('clio-editor-draft', '{broken');
  const save = setup(t, () => memory());
  const removeItem = localStorage.removeItem;
  localStorage.removeItem = () => { throw Error('denied'); };
  t.after(() => { localStorage.removeItem = removeItem; });
  assert.equal(save.discard(), false);
  assert.equal(save.scheduleAutoSave(memory(), { content: 'Replacement' }), false);
  assert.equal(localStorage.getItem('clio-editor-draft'), '{broken');
});

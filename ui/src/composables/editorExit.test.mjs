import '../../test/setup.mjs';
import assert from 'node:assert/strict';
import test, { beforeEach } from 'node:test';
beforeEach(() => localStorage.clear());
import { setActivePinia, createPinia } from 'pinia';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { nextTick } from 'vue';
import { mountSetup, memory } from '../../test/setup.mjs';
const { useMemoryStore } = await import('../stores/memories.ts');
const { default: MemoryDrawer } = await import('../components/MemoryDrawer.vue');
const { default: TagInput } = await import('../components/TagInput.vue');

function setup(t, update) {
  setActivePinia(createPinia());
  const store = useMemoryStore();
  mockIPC((command, args) => {
    if (command === 'cmd_update') return update(args.patch);
    if (command === 'cmd_get') return memory({ id: args.memoryId, content: 'Other memory' });
    throw new Error(`Unexpected command ${command}`);
  });
  const mounted = mountSetup(() => MemoryDrawer.setup({}, { expose() {} }));
  t.after(() => { mounted.unmount(); clearMocks(); });
  return { store, editor: mounted.state };
}

test('opening another memory retains the original editor if its save fails', async t => {
  const { store, editor } = setup(t, () => { throw new Error('offline'); });
  store.drawerMemory = memory();
  store.drawerOpen = true;
  await nextTick();
  editor.editContent.value = 'Keep this draft';
  editor.onContentChange();
  assert.equal(await store.openDrawer('two'), false);
  assert.equal(store.drawerMemory.id, 'one');
  assert.equal(editor.editContent.value, 'Keep this draft');
  assert.equal(editor.dirty.value, true);
});

test('archive from the editor stops when pending changes cannot save', async t => {
  const { store, editor } = setup(t, () => { throw new Error('conflict'); });
  store.drawerMemory = memory();
  store.drawerOpen = true;
  await nextTick();
  editor.editTitle.value = 'Keep this title';
  editor.onTitleChange();
  await editor.archiveMemory();
  assert.equal(store.drawerOpen, true);
  assert.equal(editor.editTitle.value, 'Keep this title');
});

test('tag input exposes a synchronous commit before the editor closes', t => {
  const updates = [];
  let exposed;
  const mounted = mountSetup(() => TagInput.setup({ modelValue: [], suggestions: [], placeholder: 'Add tag' }, { emit: (_name, tags) => updates.push(tags), expose: value => { exposed = value; } }));
  t.after(mounted.unmount);
  mounted.state.input.value = 'Pending tag';
  assert.equal(typeof exposed?.commit, 'function');
  exposed.commit();
  assert.deepEqual(updates, [['pending-tag']]);
});

test('unavailable revision storage cannot prevent edits being queued for saving', async t => {
  const { store, editor } = setup(t, patch => ({ ...memory(), ...patch, updated_at: 'version-2' }));
  store.drawerMemory = memory();
  store.drawerOpen = true;
  await nextTick();
  const original = localStorage.setItem;
  localStorage.setItem = () => { throw new Error('storage unavailable'); };
  t.after(() => { localStorage.setItem = original; });
  editor.editContent.value = 'Still save this';
  assert.doesNotThrow(() => editor.onContentChange());
  assert.equal(editor.dirty.value, true);
  assert.equal(await store.closeDrawer(), true);
});

test('a pending delete cannot close a different editor after it finishes', async t => {
  const { store, editor } = setup(t, patch => ({ ...memory(), ...patch }));
  let finishDelete;
  store.deleteMemory = () => new Promise(resolve => { finishDelete = () => resolve(true); });
  store.drawerMemory = memory();
  store.drawerOpen = true;
  await nextTick();
  const deleting = editor.confirmDelete();
  await nextTick();
  assert.equal(await store.openDrawer('two'), false);
  assert.equal(store.drawerMemory.id, 'one');
  finishDelete();
  await deleting;
  assert.equal(store.drawerOpen, false);
});

test('discarding an editor clears pending tag text without re-saving discarded content', async t => {
  const calls=[];
  const {store,editor}=setup(t, patch=>{calls.push(patch);return {...memory(),...patch};});
  store.drawerMemory=memory();store.drawerOpen=true;await nextTick();
  editor.editContent.value='Explicitly discarded';editor.onContentChange();
  let exposed;
  const tag=mountSetup(()=>TagInput.setup({modelValue:[],suggestions:[]},{emit:(_name,tags)=>{editor.editTags.value=tags;editor.onMetaChange();},expose:value=>{exposed=value;}}));
  t.after(tag.unmount);
  tag.state.input.value='Uncommitted tag';editor.tagsRef.value=exposed;
  await editor.discardAndClose();
  assert.equal(store.drawerOpen,false);
  assert.deepEqual(calls,[]);
});

test('opening a creator invalidates an earlier pending drawer request', async t => {
  const {store}=setup(t,()=>{});
  let finish;
  mockIPC(()=>new Promise(resolve=>{finish=()=>resolve(memory());}));
  const opening=store.openDrawer('one');
  await new Promise(resolve => setImmediate(resolve));
  await store.toggleCompose();
  finish();await opening;
  assert.equal(store.composeOpen,true);
  assert.equal(store.drawerOpen,false);
});

test('failed recovery remains visible and blocks opening a different memory', async t => {
  const draft={version:1,draft:{memoryId:'recover-me',expectedUpdatedAt:'old',updates:{content:'Protected draft'}}};
  localStorage.setItem('clio-editor-draft',JSON.stringify(draft));
  const {store,editor}=setup(t,()=>{});
  mockIPC(()=>{throw Error('Offline while recovering');});
  await nextTick();await nextTick();
  assert.equal(editor.unresolvedRecoveryId.value,'recover-me');
  assert.equal(await store.openDrawer('another'),false);
  assert.deepEqual(JSON.parse(localStorage.getItem('clio-editor-draft')),draft);
});

test('attention evidence is rechecked for archive and expiry before the drawer opens', async t => {
  const {store}=setup(t,()=>{});
  for(const overrides of [{archived_at:'2026-09-04T00:00:00Z'},{valid_until:'2000-01-01T00:00:00Z'}]){
    mockIPC(()=>memory(overrides));
    assert.equal(await store.openDrawer('one',{eligibleOnly:true}),false);
    assert.equal(store.drawerOpen,false);
  }
});

test('attention evidence that moved to another workspace does not open under the old one', async t => {
  const {store}=setup(t,()=>{});
  mockIPC(()=>memory({namespace:'project:moved'}));
  assert.equal(await store.openDrawer('one',{eligibleOnly:true,namespace:'project:original'}),false);
  assert.equal(store.drawerOpen,false);
  assert.equal(await store.openDrawer('one',{eligibleOnly:true,namespace:'project:moved'}),true);
});

test('corrupt editor recovery blocks opening another memory until explicit discard', async t => {
  localStorage.setItem('clio-editor-draft', '{broken');
  const { store, editor } = setup(t, () => memory());
  assert.equal(editor.recoveryBlocked.value, true);
  assert.equal(await store.openDrawer('another'), false);
  assert.equal(localStorage.getItem('clio-editor-draft'), '{broken');
  assert.equal(editor.discard(), true);
  assert.equal(await store.openDrawer('another'), true);
});

test('retrying unreadable editor recovery opens its original memory and restores its edits', async t => {
  localStorage.setItem('clio-editor-draft', JSON.stringify({ version: 1, draft: { memoryId: 'recover-me', expectedUpdatedAt: 'original-version', updates: { content: 'Protected draft' } } }));
  const getItem = localStorage.getItem;
  let denied = true;
  localStorage.getItem = key => { if (key === 'clio-editor-draft' && denied) throw Error('denied'); return getItem(key); };
  t.after(() => { localStorage.getItem = getItem; });
  const { store, editor } = setup(t, () => memory());
  assert.equal(await store.openDrawer('another'), false);
  denied = false;
  await editor.retryRecovery();
  await nextTick();
  assert.equal(store.drawerMemory.id, 'recover-me');
  assert.equal(editor.editContent.value, 'Protected draft');
  assert.equal(editor.dirty.value, true);
  assert.equal(editor.recoveryBlocked.value, false);
});

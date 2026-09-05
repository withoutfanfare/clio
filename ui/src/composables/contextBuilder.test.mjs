import '../../test/setup.mjs';
import assert from 'node:assert/strict';
import test from 'node:test';
import { createPinia, setActivePinia } from 'pinia';
import { nextTick } from 'vue';
import { mountSetup } from '../../test/setup.mjs';
const values = new Map();
globalThis.sessionStorage = { getItem: key => values.get(key) ?? null, setItem: (key, value) => values.set(key, value), removeItem: key => values.delete(key) };
const { default: Builder } = await import('../views/ContextBuilderView.vue');
function mount(t) {
  setActivePinia(createPinia());
  const mounted = mountSetup(() => Builder.setup({}, { expose() {} }));
  t.after(mounted.unmount);
  return mounted;
}
test('brief survives a fresh session, retains source IDs and exports them', async t => {
  localStorage.removeItem('clio-context-builder');
  const first = mount(t);
  first.state.addMemoryBlock({id:'018f675c-379a-70cd-813d-ea659479c610', title:'Synthetic evidence', content:'Disposable content', namespace:'project:test', kind:'fact', tags:['test']});
  first.state.addHeading();
  first.state.blocks.value[1].text = 'Next steps';
  await nextTick();
  first.unmount(); values.clear();
  const recovered = mount(t).state;
  assert.equal(recovered.blocks.value.length, 2);
  assert.equal(recovered.blocks.value[0].memoryId, '018f675c-379a-70cd-813d-ea659479c610');
  assert.match(recovered.buildMarkdown(), /018f675c-379a-70cd-813d-ea659479c610/);
  assert.match(recovered.buildMarkdown(), /## Next steps/);
  recovered.clearAll(); await nextTick();
  assert.equal(mount(t).state.blocks.value.length, 0);
});
test('storage failure is visible and does not discard the current brief', async t => {
  const state = mount(t).state;
  const original = localStorage.setItem;
  localStorage.setItem = () => { throw new Error('Storage full'); };
  try {
    state.addNarrative(); await nextTick();
    assert.equal(state.blocks.value.length, 1);
    assert.match(state.saveMessage.value, /couldn't save/i);
  } finally { localStorage.setItem = original; }
});

test('failed durable migration still restores the valid legacy draft', t => {
  localStorage.removeItem('clio-context-builder');
  values.set('clio-context-builder',JSON.stringify([{id:'legacy',type:'narrative',text:'Keep this existing brief'}]));
  const original=localStorage.setItem;localStorage.setItem=()=>{throw Error('quota');};
  try {
    const state=mount(t).state;
    assert.equal(state.blocks.value[0]?.text,'Keep this existing brief');
    assert.match(state.saveMessage.value,/couldn't save/i);
    assert.ok(values.has('clio-context-builder'));
  } finally {localStorage.setItem=original;values.clear();}
});

test('clearing a pending context search clears its loading state', async t => {
  const {mockIPC,clearMocks}=await import('@tauri-apps/api/mocks');
  t.after(clearMocks);t.mock.timers.enable({apis:['setTimeout']});
  let finish;
  mockIPC(()=>new Promise(resolve=>{finish=()=>resolve({items:[]});}));
  const state=mount(t).state;
  state.onSearchInput('pending');t.mock.timers.tick(200);
  assert.equal(state.searchLoading.value,true);
  state.onSearchInput('');
  assert.equal(state.searchLoading.value,false);
  finish();await nextTick();
  assert.deepEqual(state.searchResults.value,[]);
});

test('malformed stored brief fields are rejected without losing the original record', t => {
  for (const invalid of [
    {memoryTags:'not-an-array'}, {memoryTags:[42]}, {memoryContent:42},
    {memoryTitle:{}}, {memoryId:42}, {memoryNamespace:[]}, {memoryKind:false}, {text:42},
  ]) {
    const raw = JSON.stringify({version:1,blocks:[{id:'invalid',type:'memory',...invalid}]});
    localStorage.setItem('clio-context-builder', raw);
    const state = mount(t).state;
    assert.deepEqual(state.blocks.value, []);
    assert.match(state.saveMessage.value, /couldn't restore/i);
    assert.equal(localStorage.getItem('clio-context-builder'), raw);
    assert.doesNotThrow(() => state.buildMarkdown());
  }
  localStorage.removeItem('clio-context-builder');
});

test('a brief that failed to restore is never overwritten until explicitly discarded', async t => {
  localStorage.setItem('clio-context-builder', '{broken');
  const state = mount(t).state;
  assert.equal(state.recoveryBlocked.value, true);
  state.addNarrative(); await nextTick();
  assert.equal(localStorage.getItem('clio-context-builder'), '{broken');
  state.discardStoredBrief();
  assert.equal(state.recoveryBlocked.value, false);
  assert.equal(localStorage.getItem('clio-context-builder'), null);
  state.addHeading(); await nextTick();
  assert.match(localStorage.getItem('clio-context-builder'), /heading/);
  localStorage.removeItem('clio-context-builder');
});

test('retrying a failed brief restore recovers it once the stored record is readable', t => {
  const good = JSON.stringify({version:1,blocks:[{id:'kept',type:'narrative',text:'Kept brief'}]});
  localStorage.setItem('clio-context-builder', good);
  const getItem = localStorage.getItem;
  let denied = true;
  localStorage.getItem = key => { if (key === 'clio-context-builder' && denied) throw Error('denied'); return getItem(key); };
  t.after(() => { localStorage.getItem = getItem; localStorage.removeItem('clio-context-builder'); });
  const state = mount(t).state;
  assert.equal(state.recoveryBlocked.value, true);
  denied = false;
  state.retryRecovery();
  assert.equal(state.recoveryBlocked.value, false);
  assert.equal(state.blocks.value[0]?.text, 'Kept brief');
});

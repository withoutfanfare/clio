import '../../test/setup.mjs';
import assert from 'node:assert/strict';
import test from 'node:test';
import {createPinia,setActivePinia} from 'pinia';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import {mountSetup} from '../../test/setup.mjs';
const {default:SidePanel}=await import('../components/SidePanel.vue');
const {useMemoryStore}=await import('../stores/memories.ts');
test('obsolete hidden workspace pins do not consume every shortcut slot',t=>{
  localStorage.clear();setActivePinia(createPinia());
  localStorage.setItem('clio-workspace-pins',JSON.stringify(Array.from({length:8},(_,i)=>`deleted-${i}`)));
  const store=useMemoryStore();store.allNamespaces=['current'];
  const mounted=mountSetup(()=>SidePanel.setup({}, {expose(){}}));t.after(mounted.unmount);
  mounted.state.toggleWorkspacePin('current');
  assert.deepEqual(mounted.state.pinnedWorkspaces.value,['current']);
  assert.deepEqual(JSON.parse(localStorage.getItem('clio-workspace-pins')),['current']);
});

test('a stale confirmed global workspace menu cannot invoke purge', async t => {
  localStorage.clear(); setActivePinia(createPinia());
  const calls = [];
  mockIPC(command => { calls.push(command); throw new Error('unexpected backend call'); });
  const mounted = mountSetup(() => SidePanel.setup({}, { expose() {} }));
  t.after(() => { mounted.unmount(); clearMocks(); });
  mounted.state.ctxMenu.value = { x: 0, y: 0, ns: 'global', memoryCount: 10 };
  mounted.state.ctxConfirming.value = true;
  await mounted.state.ctxDelete();
  assert.deepEqual(calls, []);
});

test('a non-global workspace still requires confirmation and preserves shortcuts on failed purge', async t => {
  localStorage.clear(); setActivePinia(createPinia());
  localStorage.setItem('clio-workspace-pins', JSON.stringify(['project:old']));
  const calls = [];
  mockIPC((command, args) => {
    calls.push([command, args]);
    throw new Error('backup failed');
  });
  const mounted = mountSetup(() => SidePanel.setup({}, { expose() {} }));
  t.after(() => { mounted.unmount(); clearMocks(); });
  mounted.state.ctxMenu.value = { x: 0, y: 0, ns: 'project:old', memoryCount: 10 };
  await mounted.state.ctxDelete();
  assert.deepEqual(calls, []);
  assert.equal(mounted.state.ctxConfirming.value, true);
  await mounted.state.ctxDelete();
  assert.deepEqual(calls, [['cmd_purge_namespace', { namespace: 'project:old' }]]);
  assert.deepEqual(mounted.state.pinnedWorkspaces.value, ['project:old']);
});

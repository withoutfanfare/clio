import '../../test/setup.mjs';
import assert from 'node:assert/strict';
import test, { beforeEach } from 'node:test';
import { createPinia, setActivePinia } from 'pinia';
import { unref } from 'vue';
import { mountSetup, memory } from '../../test/setup.mjs';
const { default: KindSelector } = await import('../components/KindSelector.vue');
const { default: QuickCreate } = await import('../components/QuickCreate.vue');
const { default: MemoryPage } = await import('../components/MemoryPage.vue');
const { default: CommandPalette } = await import('../components/CommandPalette.vue');
const { useMemoryStore } = await import('../stores/memories.ts');
beforeEach(() => { localStorage.clear(); setActivePinia(createPinia()); });

test('kind controls preserve custom stored values and include all core kinds', t => {
  const editor = mountSetup(() => KindSelector.setup({ modelValue: 'custom-kind' }, { emit() {}, expose() {} }));
  const creator = mountSetup(() => QuickCreate.setup({}, { expose() {} }));
  t.after(() => { editor.unmount(); creator.unmount(); });
  for (const kind of ['note', 'fact', 'decision', 'summary', 'task', 'observation', 'constraint', 'snippet', 'knowledgebase', 'receipt']) {
    assert.ok(unref(editor.state.kinds).includes(kind), `Editor missing ${kind}`);
    assert.ok(unref(creator.state.kinds).includes(kind), `Creator missing ${kind}`);
  }
  assert.ok(unref(editor.state.kinds).includes('custom-kind'));
  const store = useMemoryStore();
  store.currentStats = { by_kind: [['tenant-specific-kind', 15]], top_tags: [] };
  assert.ok(unref(creator.state.kinds).includes('tenant-specific-kind'));
});

test('card dates distinguish older years', t => {
  const mounted = mountSetup(() => MemoryPage.setup({ memory: memory() }, { expose() {} }));
  t.after(mounted.unmount);
  assert.match(mounted.state.formatTime('2024-02-03T10:15:00Z'), /2024/);
  assert.match(mounted.state.formatTime('2024-02-03T10:15:00Z'), /3.*Feb/);
});

test('search excerpt reveals matching text beyond the opening sentence', t => {
  const mounted = mountSetup(() => CommandPalette.setup({}, { expose() {} }));
  t.after(mounted.unmount);
  mounted.state.query.value = 'needle';
  assert.equal(typeof mounted.state.excerpt, 'function');
  const text = mounted.state.excerpt({ content: `${'Opening detail. '.repeat(40)}The needle explains the actual match. More text.` });
  assert.match(text, /needle/);
  assert.ok(text.length <= 190);
});

import '../../test/setup.mjs';
import assert from 'node:assert/strict';
import test, { beforeEach } from 'node:test';
import { createPinia, setActivePinia } from 'pinia';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { memory } from '../../test/setup.mjs';
const { useMemoryStore } = await import('../stores/memories.ts');
beforeEach(() => { localStorage.clear(); setActivePinia(createPinia()); });
function row(index, overrides = {}) { return { ...memory({ id: String(index), updated_at: `2026-09-${String((index % 20) + 1).padStart(2, '0')}T10:00:00Z`, archived_at: null, valid_until: null, ...overrides }), rank: null, linked_from: null }; }
function page(items, offset = 0, limit = 50) { return { total: items.length, count: items.slice(offset, offset + limit).length, offset, limit, items: items.slice(offset, offset + limit) }; }
function setup(t, handler) {
  const store = useMemoryStore();
  mockIPC(handler);
  t.after(() => { store.stopPolling(); clearMocks(); });
  return store;
}

test('reopening the same memory retains a save completed while its fetch was pending', async t => {
  let finishFetch;
  const store = setup(t, () => new Promise(resolve => { finishFetch = resolve; }));
  const original = memory({ content: 'Before editing', updated_at: 'version-1' });
  store.drawerMemory = original;
  store.drawerOpen = true;
  let guards = 0;
  store.setDrawerCloseGuard(async () => {
    if (++guards === 2) Object.assign(store.drawerMemory, { content: 'Saved during fetch', updated_at: 'version-2' });
    return true;
  });
  const opening = store.openDrawer(original.id);
  await new Promise(resolve => setImmediate(resolve));
  finishFetch({ ...original });
  assert.equal(await opening, true);
  assert.equal(store.drawerMemory.content, 'Saved during fetch');
  assert.equal(store.drawerMemory.updated_at, 'version-2');
});

test('reopening an unchanged editor still loads a newer external version', async t => {
  const store = setup(t, () => memory({ content: 'Changed elsewhere', updated_at: 'version-2' }));
  store.drawerMemory = memory({ content: 'Original', updated_at: 'version-1' });
  store.drawerOpen = true;
  store.setDrawerCloseGuard(async () => true);
  assert.equal(await store.openDrawer(store.drawerMemory.id), true);
  assert.equal(store.drawerMemory.content, 'Changed elsewhere');
});

test('all 125 records are reachable in capped pages and refresh retains the loaded depth', async t => {
  const records = Array.from({ length: 125 }, (_, i) => row(i));
  const calls = [];
  const store = setup(t, (command, args) => {
    assert.equal(command, 'cmd_recent');
    calls.push(args);
    assert.ok(args.limit <= 50);
    assert.equal(args.sortBy, 'importance_desc');
    return page(records, args.offset, args.limit);
  });
  await store.loadRecent();
  assert.equal(store.items.length, 50);
  assert.equal(typeof store.loadMore, 'function');
  await store.loadMore();
  await store.loadMore();
  assert.equal(store.items.length, 125);
  assert.equal(new Set(store.items.map(item => item.id)).size, 125);
  assert.equal(store.canLoadMore, false);
  await store.loadRecent(true);
  assert.equal(store.items.length, 125);
  assert.ok(calls.some(args => args.offset === 100));
});

test('a late response from the previous namespace cannot replace the selected scope', async t => {
  const waiting = [];
  const store = setup(t, (_command, args) => new Promise(resolve => waiting.push({ args, resolve })));
  store.selectedNamespace = 'old';
  const old = store.loadRecent();
  store.selectedNamespace = 'new';
  const current = store.loadRecent();
  waiting[1].resolve(page([row(2, { namespace: 'new' })]));
  await current;
  waiting[0].resolve(page([row(1, { namespace: 'old' })]));
  await old;
  assert.deepEqual(store.items.map(item => item.namespace), ['new']);
});

test('poll and load-more failure retain loaded data and show an error', async t => {
  const records = Array.from({ length: 110 }, (_, i) => row(i));
  let fail = false;
  const store = setup(t, (_command, args) => { if (fail) throw new Error('offline'); return page(records, args.offset, args.limit); });
  await store.loadRecent();
  fail = true;
  await store.loadRecent(true);
  assert.equal(store.items.length, 50);
  assert.match(store.error, /offline/i);
  assert.equal(typeof store.loadMore, 'function');
  await store.loadMore();
  assert.equal(store.items.length, 50);
  assert.match(store.error, /offline/i);
});

test('pins beyond the first page resolve by full ID, respect scope and expiry and do not refetch every poll', async t => {
  const pins = [row('older'), row('elsewhere', { namespace: 'other' }), row('expired', { valid_until: '2020-01-01T00:00:00Z' }), row('archived', { archived_at: '2025-01-01T00:00:00Z' })];
  localStorage.setItem('clio-pinned-ids', JSON.stringify(pins.map(item => item.id)));
  let gets = 0;
  const store = setup(t, (command, args) => {
    if (command === 'cmd_get') { gets++; return pins.find(item => item.id === args.memoryId); }
    if (command === 'cmd_recent') return page([row('first')]);
    throw new Error(command);
  });
  store.selectedNamespace = 'clio';
  await store.loadRecent();
  assert.equal(typeof store.refreshPins, 'function');
  await store.refreshPins();
  assert.deepEqual(store.pinnedItems.map(item => item.id), ['older']);
  assert.deepEqual(store.navigableItems.map(item => item.id), ['older', 'first']);
  await store.loadRecent(true);
  await store.refreshPins();
  assert.equal(gets, 4);
  assert.equal(store.pinnedIds.length, 4);
});

test('Archive rejects active entries from an older backend instead of mislabelling them', async t => {
  const store = setup(t, (_command, args) => { assert.equal(args.archivedOnly, true); return { ...page([row('active')]), archived_only: true }; });
  assert.equal(typeof store.setCollection, 'function');
  await store.setCollection('archive');
  assert.equal(store.items.length, 0);
  assert.match(store.error, /archive.*support|active.*archive|backend/i);
  store.clearFilters();
  assert.equal(store.collection, 'archive');
});

test('namespace counts are fetched independently of scoped statistics', async t => {
  const store = setup(t, command => {
    assert.equal(command, 'cmd_namespace_details');
    return [{ name: 'clio', memory_count: 100 }, { name: 'other', memory_count: 45 }];
  });
  store.currentStats = { by_namespace: [['clio', 100]], by_kind: [] };
  assert.equal(typeof store.loadNamespaceDetails, 'function');
  await store.loadNamespaceDetails();
  assert.equal(store.namespaceDetails.find(item => item.name === 'other').memory_count, 45);
});

test('late palette results cannot replace a newer query', async t => {
  const waiting = [];
  const store = setup(t, (_command, args) => new Promise(resolve => waiting.push({ args, resolve })));
  const old = store.paletteSearch('old');
  const current = store.paletteSearch('new');
  waiting[1].resolve(page([row('new')]));
  await current;
  waiting[0].resolve(page([row('old')]));
  await old;
  assert.deepEqual(store.paletteResults.map(item => item.id), ['new']);
});

test('overlapping pages never render a duplicate memory', async t => {
  const records = Array.from({ length: 101 }, (_, i) => row(i));
  const store = setup(t, (_command, args) => {
    const items = args.offset === 0 ? records.slice(0, 50) : args.offset === 50 ? records.slice(49, 99) : records.slice(99);
    return { total: 101, count: items.length, offset: args.offset, limit: 50, items };
  });
  await store.loadRecent();
  await store.loadMore();
  assert.equal(new Set(store.items.map(item => item.id)).size, store.items.length);
  assert.equal(store.items.length, 99);
  await store.loadMore();
  assert.equal(store.items.length, 101);
});

test('changing kind and sort resets loaded depth and rejects an older pending page', async t => {
  const records = Array.from({ length: 120 }, (_, i) => row(i));
  let late;
  const calls = [];
  const store = setup(t, (_command, args) => {
    calls.push(args);
    if (args.offset === 50 && !args.kind) return new Promise(resolve => { late = () => resolve(page(records, args.offset)); });
    return page(args.kind ? records.map(item => ({ ...item, kind: args.kind })) : records, args.offset);
  });
  await store.loadRecent();
  const loadingMore = store.loadMore();
  await new Promise(resolve => setImmediate(resolve));
  store.setFilterKind('custom');
  await new Promise(resolve => setImmediate(resolve));
  late();
  await loadingMore;
  assert.equal(store.items.length, 50);
  assert.ok(store.items.every(item => item.kind === 'custom'));
  store.setSortBy('updated_desc');
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(store.items.length, 50);
  assert.equal(calls.at(-1).sortBy, 'updated_desc');
  assert.equal(calls.at(-1).offset, 0);
});

test('late scoped statistics cannot replace the current scope', async t => {
  const waiting = [];
  const store = setup(t, (_command, args) => new Promise(resolve => waiting.push({ args, resolve })));
  store.selectedNamespace = 'old';
  const old = store.loadStats();
  store.selectedNamespace = 'new';
  const current = store.loadStats();
  waiting[1].resolve({ namespace: 'new', by_namespace: [['new', 5]], by_kind: [] });
  await current;
  waiting[0].resolve({ namespace: 'old', by_namespace: [['old', 100]], by_kind: [] });
  await old;
  assert.deepEqual(store.currentStats.by_namespace, [['new', 5]]);
});

test('missing pins preserve preference and show a bounded error', async t => {
  localStorage.setItem('clio-pinned-ids', JSON.stringify(['missing', 'found']));
  const store = setup(t, (_command, args) => {
    if (args.memoryId === 'missing') throw new Error('not found');
    return row('found');
  });
  await store.refreshPins();
  assert.deepEqual(store.pinnedItems.map(item => item.id), ['found']);
  assert.deepEqual(store.pinnedIds, ['missing', 'found']);
  assert.match(store.pinError, /1 pinned memory/);
});

test('stored pin IDs are unique and capped at 25 before resolving them', async t => {
  const ids = Array.from({ length: 30 }, (_, i) => String(i));
  localStorage.setItem('clio-pinned-ids', JSON.stringify(['0', ...ids]));
  const store = setup(t, (_command, args) => row(args.memoryId));
  assert.equal(store.pinnedIds.length, 25);
  assert.equal(new Set(store.pinnedIds).size, 25);
});

test('load more is honoured while a background refresh is in flight', async t => {
  const records = Array.from({ length: 110 }, (_, i) => row(i));
  let holdRefresh = false;
  let finishRefresh;
  const store = setup(t, (_command, args) => {
    if (holdRefresh) {
      holdRefresh = false;
      return new Promise(resolve => { finishRefresh = () => resolve(page(records)); });
    }
    return page(records, args.offset);
  });
  await store.loadRecent();
  holdRefresh = true;
  const refresh = store.loadRecent(true);
  await store.loadMore();
  assert.equal(store.items.length, 100);
  finishRefresh();
  await refresh;
  assert.equal(store.items.length, 100);
});

test('pinned memories must match every selected tag, like the main list', async t => {
  localStorage.setItem('clio-pinned-ids', JSON.stringify(['partial', 'complete']));
  const store = setup(t, (_command, args) => row(args.memoryId, { tags: args.memoryId === 'partial' ? ['one'] : ['one', 'two'] }));
  store.filterTags = ['one', 'two'];
  await store.refreshPins();
  assert.deepEqual(store.pinnedItems.map(item => item.id), ['complete']);
});

test('clearing filters retains the chosen Recent or Important browse order', async t => {
  const store = setup(t, () => page([]));
  store.setSortBy('updated_desc');
  store.setGroupBy('date');
  store.clearFilters();
  assert.equal(store.sortBy, 'updated_desc');
  assert.equal(store.groupBy, 'date');
  assert.equal(localStorage.getItem('clio-sort-by'), 'updated_desc');
});

test('Archive fails visibly even when an older backend returns an empty unmarked response', async t => {
  const store = setup(t, () => page([]));
  await store.setCollection('archive');
  assert.match(store.error, /backend.*archive|archive.*backend/i);
  assert.equal(store.items.length, 0);
});

test('Archive accepts an empty response when the backend confirms archived-only filtering', async t => {
  const store = setup(t, () => ({ ...page([]), archived_only: true }));
  await store.setCollection('archive');
  assert.equal(store.error, null);
  assert.equal(store.total, 0);
});

test('project statistics from a backend without scope confirmation are not presented as scoped', async t => {
  const store=setup(t,()=>({by_namespace:[['other',50]],by_kind:[],total_memories:2}));
  store.selectedNamespace='project:test';
  await store.loadStats();
  assert.equal(store.currentStats,null);
  assert.match(store.statsError,/backend.*updat/i);
});

test('late activity cannot replace the current project activity', async t => {
  const requests=[];
  const store=setup(t,()=>new Promise(resolve=>requests.push(resolve)));
  store.selectedNamespace='old';const old=store.loadActivity();
  store.selectedNamespace='new';const current=store.loadActivity();
  requests[1]([{namespace:'new'}]);await current;
  requests[0]([{namespace:'old'}]);await old;
  assert.deepEqual(store.recentActivity,[{namespace:'new'}]);
});

import '../../test/setup.mjs';
import assert from 'node:assert/strict';
import test from 'node:test';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { mountSetup } from '../../test/setup.mjs';
const { useWorkOverview } = await import('./useWorkOverview.ts');
const settle = () => new Promise(resolve => setImmediate(resolve));

function setup(t, handler) {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  mockIPC((command, args) => {
    assert.equal(command, 'cmd_work_overview');
    assert.deepEqual(args, { project: null, staleAfterSecs: null });
    return handler();
  });
  const mounted = mountSetup(useWorkOverview);
  t.after(() => { mounted.unmount(); clearMocks(); });
  return mounted;
}

test('starts loading, polls after completion, and never overlaps a pending request', async t => {
  let calls = 0;
  let finish;
  const { state } = setup(t, () => { calls++; return new Promise(resolve => { finish = resolve; }); });
  assert.equal(state.loading.value, true);
  assert.equal(state.overview.value, null);
  t.mock.timers.tick(30_000);
  await state.refresh();
  assert.equal(calls, 1);
  finish({ tasks: [] });
  await settle();
  assert.equal(state.loading.value, false);
  assert.deepEqual(state.overview.value, { tasks: [] });
  t.mock.timers.tick(10_000);
  assert.equal(calls, 2);
  finish({ tasks: [] });
  await settle();
});

test('failed refresh preserves prior results and error until a successful retry', async t => {
  let fail = false;
  const { state } = setup(t, () => {
    if (fail) throw Error('Remote unavailable');
    return { tasks: [] };
  });
  await settle();
  const checkedAt = state.checkedAt.value;
  fail = true;
  t.mock.timers.tick(10_000);
  await settle();
  assert.deepEqual(state.overview.value, { tasks: [] });
  assert.match(state.error.value, /Remote unavailable/);
  assert.equal(state.checkedAt.value, checkedAt);
  fail = false;
  await state.refresh();
  assert.equal(state.error.value, null);
});

test('initial failure stays unknown rather than becoming an empty overview', async t => {
  const { state } = setup(t, () => { throw Error('Unsupported tool'); });
  await settle();
  assert.equal(state.overview.value, null);
  assert.equal(state.loading.value, false);
  assert.match(state.error.value, /Unsupported tool/);
});

test('unmount clears the next refresh timer', async t => {
  let calls = 0;
  const mounted = setup(t, () => { calls++; return { tasks: [] }; });
  await settle();
  mounted.unmount();
  t.mock.timers.tick(30_000);
  await mounted.state.refresh();
  assert.equal(calls, 1);
});

for (const reject of [false, true]) {
  test(`unmount discards a late ${reject ? 'failure' : 'response'} without scheduling another poll`, async t => {
    let finish;
    let calls = 0;
    const mounted = setup(t, () => {
      calls++;
      return new Promise((resolve, fail) => { finish = () => reject ? fail(Error('Late')) : resolve({ tasks: [] }); });
    });
    mounted.unmount();
    finish();
    await settle();
    assert.equal(mounted.state.overview.value, null);
    assert.equal(mounted.state.error.value, null);
    t.mock.timers.tick(30_000);
    assert.equal(calls, 1);
  });
}

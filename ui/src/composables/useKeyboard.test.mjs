import '../../test/setup.mjs';
import assert from 'node:assert/strict';
import test from 'node:test';
import { mountSetup } from '../../test/setup.mjs';
const { useKeyboard } = await import('./useKeyboard.ts');

function run(t, key, tagName, modal = false, metaKey = false) {
  let handler;
  const activated = [];
  window.addEventListener = (_name, callback) => { handler = callback; };
  const mounted = mountSetup(() => useKeyboard({ isModalOpen: () => modal, onCompose: () => activated.push('compose'), onArchiveFocused: () => activated.push('archive'), onOpenFocused: () => activated.push('open'), onNavigateDown: () => activated.push('down') }));
  t.after(mounted.unmount);
  const target = { tagName, isContentEditable: false, closest: selector => selector.includes('form') && tagName === 'INPUT' ? {} : null };
  handler({ key, target, metaKey, ctrlKey: false, defaultPrevented: false, isComposing: false, preventDefault() { activated.push('prevented'); } });
  return activated;
}

test('Enter on a button preserves native activation', t => assert.deepEqual(run(t, 'Enter', 'BUTTON'), []));
test('navigation does not act on the list behind a modal', t => assert.deepEqual(run(t, 'j', 'DIV', true), []));
test('archive shortcut is ignored in a form field', t => assert.deepEqual(run(t, 'd', 'INPUT', false, true), []));
test('compose shortcut is ignored while a modal is active', t => assert.deepEqual(run(t, 'n', 'TEXTAREA', true, true), []));
test('list navigation remains available outside forms and modals', t => assert.deepEqual(run(t, 'j', 'DIV'), ['prevented', 'down']));

test('Escape can close shortcut help when its opener still has focus', t => {
  let handler;
  let closed = false;
  window.addEventListener = (_name, callback) => { handler = callback; };
  const mounted = mountSetup(() => useKeyboard({ isModalOpen: () => true, onEscape: () => { closed = true; } }));
  t.after(mounted.unmount);
  handler({ key: 'Escape', target: { tagName: 'BUTTON', closest: () => null }, preventDefault() {} });
  assert.equal(closed, true);
});

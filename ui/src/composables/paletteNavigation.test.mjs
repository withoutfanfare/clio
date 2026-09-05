import '../../test/setup.mjs';
import assert from 'node:assert/strict';
import test from 'node:test';
import {createPinia,setActivePinia} from 'pinia';
import {nextTick} from 'vue';
import {mountSetup,memory} from '../../test/setup.mjs';
const {default:Palette}=await import('../components/CommandPalette.vue');
const {useMemoryStore}=await import('../stores/memories.ts');
test('arrow navigation exposes and scrolls the active result after the visible list overflows',async t=>{
  localStorage.clear();setActivePinia(createPinia());
  const store=useMemoryStore();store.paletteOpen=true;
  store.paletteResults=Array.from({length:20},(_,i)=>({...memory({id:String(i)}),rank:null,linked_from:null}));
  const mounted=mountSetup(()=>Palette.setup({}, {expose(){}}));t.after(mounted.unmount);
  const state=mounted.state;const scrolled=[];
  assert.ok(state.resultsContainer,'overflow list needs a scroll target');
  state.resultsContainer.value={querySelectorAll:()=>Array.from({length:20},(_,i)=>({scrollIntoView:options=>scrolled.push([i,options.block])}))};
  for(let i=0;i<15;i++)state.handleKeydown({key:'ArrowDown',preventDefault(){}});
  await nextTick();await nextTick();
  assert.equal(state.selectedIndex.value,15);
  assert.deepEqual(scrolled.at(-1),[15,'nearest']);
  assert.equal(state.activeResultId.value,'palette-result-15');
});

test('composition confirmation and candidate keys leave search and selection unchanged', t => {
  localStorage.clear(); setActivePinia(createPinia());
  const store = useMemoryStore(); store.paletteOpen = true;
  store.paletteResults = [memory({id:'first'}), memory({id:'second'})];
  const mounted = mountSetup(() => Palette.setup({}, {expose(){}})); t.after(mounted.unmount);
  const state = mounted.state;
  for (const composition of [{isComposing:true}, {isComposing:false,keyCode:229}]) {
    for (const key of ['Enter', 'ArrowDown', 'ArrowUp']) {
      let prevented = false;
      state.handleKeydown({key, ...composition, preventDefault(){prevented=true;}});
      assert.equal(store.paletteOpen, true, `${key} must not close an active composition`);
      assert.equal(state.selectedIndex.value, 0);
      assert.equal(prevented, false, 'the IME must receive its own keys');
    }
  }
});

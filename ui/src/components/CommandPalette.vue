<script setup lang="ts">
import { ref, watch, computed, nextTick } from "vue";
import { SBadge, SSpinner, SButton } from "@stuntrocket/ui";
import { useMemoryStore } from "@/stores/memories";
import NativeDialog from "./NativeDialog.vue";
import { memoryDate, memoryTimestamp, memoryExcerpt } from "@/utils/memoryPresentation";

const store = useMemoryStore();
const query = ref("");
const input = ref<HTMLInputElement | null>(null);
const resultsContainer = ref<HTMLElement | null>(null);
const selectedIndex = ref(0);
const results = computed(() => [
  ...store.paletteResults,
  ...store.paletteSemanticResults.filter(item => !store.paletteResults.some(result => result.id === item.id)),
]);
const activeResultId = computed(() => {
  const result = results.value[selectedIndex.value];
  return result ? `palette-result-${result.id}` : undefined;
});

watch([selectedIndex, results], async () => {
  selectedIndex.value = Math.max(0, Math.min(selectedIndex.value, results.value.length - 1));
  await nextTick();
  resultsContainer.value?.querySelectorAll<HTMLElement>(".palette-result")[selectedIndex.value]?.scrollIntoView({ block: "nearest" });
});

watch([query, () => store.selectedNamespace], ([value], _previous, onCleanup) => {
  selectedIndex.value = 0;
  // Invalidate old results immediately, before starting the debounce period.
  void store.paletteSearch("");
  const fts = setTimeout(() => void store.paletteSearch(value), 150);
  const semantic = setTimeout(() => void store.paletteSemanticSearch(value), 500);
  onCleanup(() => { clearTimeout(fts); clearTimeout(semantic); });
});
watch(() => store.paletteOpen, (open) => {
  if (open) nextTick(() => input.value?.focus());
  else query.value = "";
});

async function handleSelect(index: number) {
  const result = results.value[index];
  if (!result) return;
  store.closePalette();
  await store.openDrawer(result.id);
}

function handleClose() {
  store.closePalette();
  query.value = "";
}

function handleKeydown(event: KeyboardEvent) {
  // WebKit may clear isComposing on the key that confirms an IME candidate.
  if (event.isComposing || event.keyCode === 229) return;
  if (event.key === "ArrowDown" || event.key === "ArrowUp") {
    event.preventDefault();
    const direction = event.key === "ArrowDown" ? 1 : -1;
    selectedIndex.value = Math.max(0, Math.min(results.value.length - 1, selectedIndex.value + direction));
  } else if (event.key === "Enter") {
    event.preventDefault();
    void handleSelect(selectedIndex.value);
  }
}

function excerpt(result: { content: string }) {
  return memoryExcerpt(result.content, query.value);
}

function isSemanticOnly(id: string): boolean {
  return store.paletteSemanticResults.some(item => item.id === id) && !store.paletteResults.some(item => item.id === id);
}
</script>

<template>
  <NativeDialog :open="store.paletteOpen" label="Search memories" @close="handleClose">
    <section class="palette-panel">
      <header class="palette-header">
        <h2>Search memories</h2>
        <SButton variant="icon" size="sm" aria-label="Close search" @click="handleClose">×</SButton>
      </header>
      <p class="palette-scope">Active memories · {{ store.selectedNamespace || "All namespaces" }}</p>
      <input ref="input" v-model="query" autofocus type="search" class="palette-input" role="combobox" aria-autocomplete="list" aria-controls="palette-results" :aria-expanded="store.paletteOpen" :aria-activedescendant="activeResultId" aria-label="Search memories" placeholder="Search memories…" @keydown="handleKeydown" />
      <div id="palette-results" ref="resultsContainer" class="palette-results" role="listbox" aria-label="Matching memories">
        <div v-if="store.paletteLoading && !results.length" class="palette-loading" role="status">
          <SSpinner size="sm" /><span>Searching…</span>
        </div>
        <p v-if="store.paletteError" class="palette-empty" role="alert">Search could not load: {{ store.paletteError }}</p>
        <button v-for="(result, index) in results" :id="`palette-result-${result.id}`" :key="result.id" class="palette-result" role="option" :aria-selected="index === selectedIndex" tabindex="-1" :class="{ selected: index === selectedIndex }" @click="handleSelect(index)" @focus="selectedIndex = index">
          <span class="result-main">
            <span class="result-title">{{ result.title || result.content.slice(0, 80) }}</span>
            <SBadge v-if="isSemanticOnly(result.id)" variant="accent">Related</SBadge>
          </span>
          <span class="result-meta">
            <span>{{ result.kind }}</span><span>· {{ result.namespace }}</span>
            <time :datetime="result.updated_at" :title="memoryTimestamp(result.updated_at)">· {{ memoryDate(result.updated_at) }}</time>
            <span v-if="result.source">· {{ result.source }}</span>
          </span>
          <span class="result-excerpt">{{ excerpt(result) }}</span>
        </button>
        <div v-if="query.trim() && !results.length && !store.paletteLoading && !store.paletteError" class="palette-empty">No matching active memories.</div>
      </div>
    </section>
  </NativeDialog>
</template>

<style scoped>
.palette-panel { position: fixed; top: 12vh; left: 50%; transform: translateX(-50%); width: min(640px, 90vw); max-height: 76vh; background: var(--colour-surface-dropdown); border: 1px solid var(--colour-border); border-radius: var(--radius-lg); box-shadow: var(--shadow-overlay); overflow: hidden; display: flex; flex-direction: column; }
.palette-header { display: flex; align-items: center; justify-content: space-between; padding: var(--space-4); }
.palette-header h2 { font-size: 15px; font-weight: 600; }
.palette-scope { padding: 0 var(--space-4) var(--space-3); font-size: 12px; color: var(--color-text-secondary); }
.palette-input { margin: 0 var(--space-4) var(--space-3); padding: var(--space-3); background: var(--colour-surface-input); border: 1px solid var(--colour-border); border-radius: var(--radius-sm); color: var(--color-text-primary); font: inherit; }
.palette-results { overflow-y: auto; padding: var(--space-2); }
.result-excerpt { display: block; margin-top: var(--space-2); color: var(--color-text-secondary); font-size: 12px; line-height: 1.5; }

.palette-loading {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-4);
  font-size: 13px;
  color: var(--color-text-tertiary);
  justify-content: center;
}

.palette-result {
  width: 100%;
  padding: var(--space-2) var(--space-3);
  background: transparent;
  border: none;
  border-radius: var(--radius-md);
  cursor: pointer;
  text-align: left;
  font-family: inherit;
  transition: background 100ms ease;
}

.palette-result:hover,
.palette-result.selected {
  background: var(--color-surface-hover);
}

.result-main {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin-bottom: 2px;
}

.result-title {
  font-size: 13px;
  font-weight: 500;
  color: var(--color-text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  line-height: 1.5;
}

.result-meta {
  display: flex;
  gap: var(--space-1);
  font-size: 11px;
  color: var(--color-text-tertiary);
}

.palette-empty {
  padding: var(--space-6) var(--space-4);
  text-align: center;
  font-size: 13px;
  color: var(--color-text-tertiary);
  line-height: 1.5;
}

.palette-empty kbd {
  font-size: 11px;
  padding: 1px var(--space-1);
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border);
  color: var(--color-text-tertiary);
  font-family: inherit;
}
</style>

<script setup lang="ts">
import { ref, watch } from "vue";
import { SCommandPalette, SBadge, SSpinner } from "@stuntrocket/ui";
import { useMemoryStore } from "@/stores/memories";
import { useDebounceFn } from "@/composables/useDebounce";

const store = useMemoryStore();
const query = ref("");

const debouncedFts = useDebounceFn((q: string) => {
  store.paletteSearch(q);
}, 150);

const debouncedSemantic = useDebounceFn((q: string) => {
  store.paletteSemanticSearch(q);
}, 500);

watch(query, (q) => {
  debouncedFts(q);
  debouncedSemantic(q);
});

function allResults() {
  return [
    ...store.paletteResults,
    ...store.paletteSemanticResults.filter(
      (s) => !store.paletteResults.some((r) => r.id === s.id),
    ),
  ];
}

function handleSelect(index: number) {
  const results = allResults();
  if (results[index]) {
    store.closePalette();
    store.openDrawer(results[index].id);
  }
}

function handleClose() {
  store.closePalette();
  query.value = "";
}

function handleQueryUpdate(q: string) {
  query.value = q;
}

function isSemanticOnly(id: string): boolean {
  return (
    !store.paletteResults.some((r) => r.id === id) &&
    store.paletteSemanticResults.some((s) => s.id === id)
  );
}
</script>

<template>
  <SCommandPalette
    :open="store.paletteOpen"
    placeholder="Search memories..."
    :result-count="allResults().length"
    @close="handleClose"
    @select="handleSelect"
    @update:query="handleQueryUpdate"
  >
    <template #default="{ selectedIndex }">
      <div v-if="store.paletteLoading && !allResults().length" class="palette-loading">
        <SSpinner size="sm" />
        <span>Searching&hellip;</span>
      </div>
      <button
        v-for="(result, i) in allResults()"
        :key="result.id"
        class="palette-result"
        :class="{ selected: i === selectedIndex }"
        @click="handleSelect(i)"
        @mouseenter="() => {}"
      >
        <div class="result-main">
          <span class="result-title">{{ result.title || result.content.slice(0, 80) }}</span>
          <SBadge v-if="isSemanticOnly(result.id)" variant="accent">Related</SBadge>
        </div>
        <div class="result-meta">
          <span class="result-kind">{{ result.kind }}</span>
          <span v-if="result.namespace !== 'global'" class="result-ns">&middot; {{ result.namespace }}</span>
        </div>
      </button>

      <div v-if="query.trim() && !allResults().length && !store.paletteLoading" class="palette-empty">
        No results found. Press <kbd>&#8984;&#9166;</kbd> to create a new memory.
      </div>
    </template>
  </SCommandPalette>
</template>

<style scoped>
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

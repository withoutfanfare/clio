<script setup lang="ts">
import { ref, watch, nextTick } from "vue";
import { useMemoryStore } from "@/stores/memories";
import { useDebounceFn } from "@/composables/useDebounce";

const store = useMemoryStore();
const inputRef = ref<HTMLInputElement | null>(null);
const query = ref("");
const selectedIndex = ref(0);

const debouncedFts = useDebounceFn((q: string) => {
  store.paletteSearch(q);
}, 150);

const debouncedSemantic = useDebounceFn((q: string) => {
  store.paletteSemanticSearch(q);
}, 500);

watch(
  () => store.paletteOpen,
  (open) => {
    if (open) {
      query.value = "";
      selectedIndex.value = 0;
      nextTick(() => inputRef.value?.focus());
    }
  },
);

watch(query, (q) => {
  selectedIndex.value = 0;
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

function handleKeydown(e: KeyboardEvent) {
  const results = allResults();

  if (e.key === "ArrowDown") {
    e.preventDefault();
    selectedIndex.value = Math.min(selectedIndex.value + 1, results.length - 1);
  } else if (e.key === "ArrowUp") {
    e.preventDefault();
    selectedIndex.value = Math.max(selectedIndex.value - 1, 0);
  } else if (e.key === "Enter") {
    e.preventDefault();
    if ((e.metaKey || e.ctrlKey) && query.value.trim()) {
      store.closePalette();
      store.captureMemory(query.value.trim());
    } else if (results[selectedIndex.value]) {
      store.closePalette();
      store.openDrawer(results[selectedIndex.value].id);
    }
  }
}

function selectResult(id: string) {
  store.closePalette();
  store.openDrawer(id);
}

function isSemanticOnly(id: string): boolean {
  return (
    !store.paletteResults.some((r) => r.id === id) &&
    store.paletteSemanticResults.some((s) => s.id === id)
  );
}
</script>

<template>
  <Teleport to="body">
    <Transition name="fade">
      <div
        v-if="store.paletteOpen"
        class="palette-backdrop"
        @click="store.closePalette()"
      />
    </Transition>
    <Transition name="scale">
      <div v-if="store.paletteOpen" class="palette">
        <div class="palette-input-row">
          <svg class="palette-icon" width="16" height="16" viewBox="0 0 16 16" fill="none">
            <circle cx="7" cy="7" r="4.5" stroke="currentColor" stroke-width="1.5"/>
            <path d="M10.5 10.5L14 14" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
          <input
            ref="inputRef"
            v-model="query"
            class="palette-input"
            placeholder="Search memories..."
            @keydown="handleKeydown"
          />
          <kbd class="palette-kbd">esc</kbd>
        </div>

        <div
          v-if="allResults().length || store.paletteLoading"
          class="palette-results"
        >
          <div v-if="store.paletteLoading && !allResults().length" class="palette-loading">
            Searching&hellip;
          </div>
          <button
            v-for="(result, i) in allResults()"
            :key="result.id"
            class="palette-result"
            :class="{ selected: i === selectedIndex }"
            @click="selectResult(result.id)"
            @mouseenter="selectedIndex = i"
          >
            <div class="result-main">
              <span class="result-title">{{ result.title || result.content.slice(0, 80) }}</span>
              <span v-if="isSemanticOnly(result.id)" class="result-badge">Related</span>
            </div>
            <div class="result-meta">
              <span class="result-kind">{{ result.kind }}</span>
              <span v-if="result.namespace !== 'global'" class="result-ns">&middot; {{ result.namespace }}</span>
            </div>
          </button>
        </div>

        <div v-if="query.trim() && !allResults().length && !store.paletteLoading" class="palette-empty">
          No results found. Press <kbd>&#8984;&#9166;</kbd> to create a new memory.
        </div>

        <div class="palette-footer">
          <span class="palette-hint">
            <kbd>&uarr;&darr;</kbd> navigate
            <kbd>&#9166;</kbd> open
            <kbd>&#8984;&#9166;</kbd> create
          </span>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.palette-backdrop {
  position: fixed;
  inset: 0;
  background: color-mix(in srgb, var(--grey-950) 60%, transparent);
  backdrop-filter: blur(4px);
  -webkit-backdrop-filter: blur(4px);
  z-index: 400;
}

.palette {
  position: fixed;
  top: 15vh;
  left: 50%;
  transform: translateX(-50%);
  width: 540px;
  max-width: 90vw;
  background: var(--colour-surface-panel);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-overlay), var(--glass-glow-strong);
  z-index: 401;
  overflow: hidden;
}

/* ── Search Input ── */
.palette-input-row {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-4);
  border-bottom: 1px solid var(--colour-border);
}

.palette-icon {
  color: var(--colour-text-muted);
  flex-shrink: 0;
}

.palette-input {
  flex: 1;
  background: none;
  border: none;
  outline: none;
  font-size: var(--text-lg);
  font-weight: var(--font-normal);
  color: var(--colour-text);
  font-family: inherit;
  line-height: var(--leading-normal);
}

.palette-input::placeholder {
  color: var(--colour-text-disabled);
}

.palette-kbd {
  font-size: var(--text-xs);
  padding: 2px var(--space-2);
  border-radius: var(--radius-sm);
  border: 1px solid var(--colour-border);
  color: var(--colour-text-disabled);
  font-family: inherit;
  line-height: var(--leading-tight);
}

/* ── Results List ── */
.palette-results {
  max-height: 340px;
  overflow-y: auto;
  padding: var(--space-1);
}

.palette-loading {
  padding: var(--space-4);
  font-size: var(--text-sm);
  color: var(--colour-text-muted);
  text-align: center;
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
  background: var(--colour-surface-overlay);
}

.result-main {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin-bottom: 2px;
}

.result-title {
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  color: var(--colour-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  line-height: var(--leading-normal);
}

.result-badge {
  font-size: var(--text-xs);
  padding: 1px var(--space-2);
  border-radius: var(--radius-sm);
  background: var(--colour-accent-muted);
  color: var(--colour-accent);
  font-weight: var(--font-medium);
  flex-shrink: 0;
  line-height: var(--leading-normal);
}

.result-meta {
  display: flex;
  gap: var(--space-1);
  font-size: var(--text-xs);
  color: var(--colour-text-disabled);
}

.result-kind {
  color: var(--colour-text-disabled);
}

.result-ns {
  color: var(--colour-text-disabled);
}

/* ── Empty State ── */
.palette-empty {
  padding: var(--space-6) var(--space-4);
  text-align: center;
  font-size: var(--text-sm);
  color: var(--colour-text-muted);
  line-height: var(--leading-normal);
}

.palette-empty kbd {
  font-size: var(--text-xs);
  padding: 1px var(--space-1);
  border-radius: var(--radius-sm);
  border: 1px solid var(--colour-border);
  color: var(--colour-text-disabled);
  font-family: inherit;
}

/* ── Footer Hints ── */
.palette-footer {
  padding: var(--space-2) var(--space-4);
  border-top: 1px solid var(--colour-border);
}

.palette-hint {
  font-size: var(--text-xs);
  color: var(--colour-text-disabled);
  display: flex;
  gap: var(--space-4);
}

.palette-hint kbd {
  font-size: var(--text-xs);
  padding: 1px var(--space-1);
  border-radius: var(--radius-sm);
  border: 1px solid var(--colour-border);
  color: var(--colour-text-disabled);
  font-family: inherit;
  margin-right: 3px;
}
</style>

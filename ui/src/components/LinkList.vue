<script setup lang="ts">
import { ref, watch } from "vue";
import * as api from "@/api/memory";
import { useMemoryStore } from "@/stores/memories";
import type { MemoryLink, SuggestionResult } from "@/api/types";

const props = defineProps<{
  memoryId: string;
}>();

const store = useMemoryStore();
const links = ref<MemoryLink[]>([]);
const suggestions = ref<SuggestionResult[]>([]);
const loadingSuggestions = ref(false);
const loadingLinks = ref(false);
const suggestError = ref<string | null>(null);
const suggestAttempted = ref(false);
const expanded = ref(false);
const loaded = ref(false);

async function loadLinks() {
  if (loaded.value || loadingLinks.value) return;
  loadingLinks.value = true;
  try {
    links.value = await api.getLinks(props.memoryId);
    loaded.value = true;
  } catch {
    // May not have links
  } finally {
    loadingLinks.value = false;
  }
}

function toggle() {
  expanded.value = !expanded.value;
  if (expanded.value && !loaded.value) {
    loadLinks();
  }
}

// Reset state when memory changes
watch(() => props.memoryId, () => {
  links.value = [];
  suggestions.value = [];
  suggestError.value = null;
  suggestAttempted.value = false;
  loaded.value = false;
  if (expanded.value) {
    loadLinks();
  }
});

async function suggestLinks() {
  loadingSuggestions.value = true;
  suggestError.value = null;
  suggestAttempted.value = true;
  try {
    suggestions.value = await api.suggestLinks({
      memory_id: props.memoryId,
      limit: 5,
    });
  } catch (e) {
    console.error("suggestLinks failed:", e);
    suggestError.value = String(e);
  } finally {
    loadingSuggestions.value = false;
  }
}

async function createLink(toId: string) {
  try {
    await api.link({
      from_memory_id: props.memoryId,
      to_memory_id: toId,
    });
    links.value = await api.getLinks(props.memoryId);
    suggestions.value = suggestions.value.filter((s) => s.memory.id !== toId);
  } catch {
    // Link creation failed
  }
}

function openLinked(id: string) {
  store.openDrawer(id);
}
</script>

<template>
  <div class="link-list">
    <div class="links-header">
      <button class="links-toggle" @click="toggle">
        <svg
          width="10" height="10" viewBox="0 0 12 12" fill="none"
          class="links-chevron"
          :class="{ open: expanded }"
        >
          <path d="M4 2l4 4-4 4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
        <span class="links-title">Links</span>
        <span v-if="links.length && !expanded" class="links-count">{{ links.length }}</span>
      </button>
      <button
        v-if="expanded"
        class="suggest-btn"
        @click="suggestLinks"
        :disabled="loadingSuggestions"
      >
        <svg
          v-if="loadingSuggestions"
          class="spinner"
          width="14"
          height="14"
          viewBox="0 0 14 14"
          fill="none"
        >
          <circle
            cx="7" cy="7" r="5.5"
            stroke="currentColor"
            stroke-width="1.5"
            stroke-dasharray="24 10"
            stroke-linecap="round"
          />
        </svg>
        {{ loadingSuggestions ? "Finding links\u2026" : "Suggest links" }}
      </button>
    </div>

    <Transition name="fade">
      <div v-if="expanded" class="links-body">
        <div v-if="loadingLinks" class="loading-bar">
          <div class="loading-bar-track" />
        </div>

        <div v-if="loadingSuggestions" class="loading-bar">
          <div class="loading-bar-track" />
        </div>

        <div v-if="links.length" class="links-items">
          <button
            v-for="link in links"
            :key="link.to_memory_id"
            class="link-item"
            @click="openLinked(link.to_memory_id)"
          >
            <span class="link-rel">{{ link.relationship || "relates_to" }}</span>
            <span class="link-id">{{ link.to_memory_id.slice(0, 8) }}</span>
          </button>
        </div>

        <p v-if="suggestError" class="suggest-error">
          {{ suggestError }}
        </p>

        <div v-if="suggestions.length" class="suggestions">
          <span class="suggestions-label">Suggested</span>
          <button
            v-for="s in suggestions"
            :key="s.memory.id"
            class="suggestion-item"
            @click="createLink(s.memory.id)"
          >
            <span class="suggestion-title">{{ s.memory.title || s.memory.content.slice(0, 60) }}</span>
            <span class="suggestion-score">{{ (s.similarity * 100).toFixed(0) }}%</span>
          </button>
        </div>

        <p v-if="suggestAttempted && !loadingSuggestions && !suggestError && !suggestions.length && !links.length" class="links-empty">
          No similar memories found
        </p>

        <p v-if="!suggestAttempted && !links.length && !suggestions.length && !loadingLinks" class="links-empty">
          No linked memories
        </p>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.link-list {
  padding-top: var(--space-4);
  border-top: 1px solid var(--colour-border);
  margin-top: var(--space-4);
}

.links-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.links-toggle {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  background: none;
  border: none;
  color: var(--colour-text-muted);
  cursor: pointer;
  transition: color 150ms;
  padding: var(--space-1) 0;
}

.links-toggle:hover {
  color: var(--colour-text);
}

.links-chevron {
  transition: transform 200ms cubic-bezier(0.4, 0, 0.2, 1);
}

.links-chevron.open {
  transform: rotate(90deg);
}

.links-title {
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  text-transform: uppercase;
  letter-spacing: var(--tracking-caps);
}

.links-count {
  font-size: var(--text-xs);
  color: var(--colour-text-disabled);
  font-variant-numeric: tabular-nums;
}

.links-body {
  margin-top: var(--space-3);
}

.suggest-btn {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
  padding: 0;
  background: none;
  border: none;
  color: var(--colour-accent);
  font-size: var(--text-sm);
  cursor: pointer;
  transition: color 150ms;
}

.suggest-btn:hover:not(:disabled) {
  color: var(--colour-accent-hover);
}

.suggest-btn:disabled {
  opacity: 0.7;
  cursor: default;
  color: var(--colour-text-secondary);
}

/* ── Spinner ── */
.spinner {
  animation: spin 0.8s linear infinite;
  flex-shrink: 0;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

/* ── Loading Bar ── */
.loading-bar {
  height: 2px;
  border-radius: 1px;
  background: var(--colour-surface-overlay);
  overflow: hidden;
  margin-bottom: var(--space-3);
}

.loading-bar-track {
  height: 100%;
  width: 40%;
  background: var(--colour-accent);
  border-radius: 1px;
  animation: slide 1.2s ease-in-out infinite;
}

@keyframes slide {
  0% { transform: translateX(-100%); }
  100% { transform: translateX(350%); }
}

/* ── Error ── */
.suggest-error {
  font-size: var(--text-sm);
  color: var(--colour-danger);
  margin-bottom: var(--space-3);
}

.links-items {
  display: flex;
  flex-direction: column;
  margin-bottom: var(--space-3);
}

.link-item {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-1);
  background: none;
  border: none;
  border-bottom: 1px solid var(--colour-border);
  cursor: pointer;
  transition: background 150ms;
  width: 100%;
  text-align: left;
}

.link-item:last-child {
  border-bottom: none;
}

.link-item:hover {
  background: var(--colour-surface-overlay);
}

.link-rel {
  font-size: var(--text-xs);
  color: var(--colour-text-disabled);
  font-weight: var(--font-medium);
}

.link-id {
  font-size: var(--text-sm);
  color: var(--colour-text);
  font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace;
}

.suggestions {
  display: flex;
  flex-direction: column;
}

.suggestions-label {
  font-size: var(--text-xs);
  color: var(--colour-text-disabled);
  text-transform: uppercase;
  letter-spacing: var(--tracking-caps);
  font-weight: var(--font-semibold);
  margin-bottom: var(--space-1);
}

.suggestion-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-2) var(--space-1);
  background: none;
  border: none;
  border-bottom: 1px dashed var(--colour-border);
  cursor: pointer;
  transition: background 150ms;
  width: 100%;
  text-align: left;
}

.suggestion-item:last-child {
  border-bottom: none;
}

.suggestion-item:hover {
  background: var(--colour-accent-muted);
}

.suggestion-title {
  font-size: var(--text-sm);
  color: var(--colour-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 80%;
}

.suggestion-score {
  font-size: var(--text-xs);
  color: var(--colour-text-disabled);
  font-weight: var(--font-medium);
  font-variant-numeric: tabular-nums;
}

.links-empty {
  font-size: var(--text-sm);
  color: var(--colour-text-disabled);
}
</style>

<script setup lang="ts">
import { ref, watch } from "vue";
import { SButton, SSpinner, SProgressBar } from "@stuntrocket/ui";
import * as api from "@/api/memory";
import { useMemoryStore } from "@/stores/memories";
import type { LinkContext, SuggestionResult } from "@/api/types";

const props = defineProps<{
  memoryId: string;
}>();

// Relationships that describe how truth changed — rendered as history.
const HISTORY_RELATIONSHIPS = ["supersedes", "reverses", "contradicts", "resolved_by"];

const store = useMemoryStore();
const links = ref<LinkContext[]>([]);
const titles = ref<Record<string, string>>({});
const suggestions = ref<SuggestionResult[]>([]);
const loadingSuggestions = ref(false);
const loadingLinks = ref(false);
const suggestError = ref<string | null>(null);
const suggestAttempted = ref(false);
const expanded = ref(false);
const loaded = ref(false);

function otherId(ctx: LinkContext): string {
  return ctx.direction === "outgoing" ? ctx.to_memory_id : ctx.from_memory_id;
}

function linkTitle(ctx: LinkContext): string {
  const id = otherId(ctx);
  return titles.value[id] ?? id.slice(-8);
}

function rationale(ctx: LinkContext): string | null {
  const value = ctx.metadata?.["rationale"];
  return typeof value === "string" && value ? value : null;
}

function isHistory(ctx: LinkContext): boolean {
  return HISTORY_RELATIONSHIPS.includes(ctx.relationship);
}

async function loadLinks() {
  if (loaded.value || loadingLinks.value) return;
  loadingLinks.value = true;
  try {
    links.value = await api.linkContexts(props.memoryId);
    loaded.value = true;
    // Resolve linked titles best-effort so rows read as titles, not IDs.
    const ids = [...new Set(links.value.map(otherId))];
    for (const id of ids) {
      if (titles.value[id]) continue;
      try {
        const memory = await api.getMemory(id);
        titles.value[id] = memory.title ?? memory.content.slice(0, 60);
      } catch {
        // Leave the fallback id snippet.
      }
    }
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
    links.value = await api.linkContexts(props.memoryId);
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
      <SButton
        v-if="expanded"
        variant="ghost"
        size="sm"
        @click="suggestLinks"
        :disabled="loadingSuggestions"
        :loading="loadingSuggestions"
      >
        {{ loadingSuggestions ? "Finding links\u2026" : "Suggest links" }}
      </SButton>
    </div>

    <Transition name="fade">
      <div v-if="expanded" class="links-body">
        <SProgressBar v-if="loadingLinks" :value="0" indeterminate size="sm" class="loading-bar" />
        <SProgressBar v-if="loadingSuggestions" :value="0" indeterminate size="sm" class="loading-bar" />

        <div v-if="links.some(isHistory)" class="history-items">
          <span class="suggestions-label">Decision history</span>
          <button
            v-for="link in links.filter(isHistory)"
            :key="link.from_memory_id + link.relationship + link.to_memory_id"
            class="link-item"
            @click="openLinked(otherId(link))"
          >
            <span class="link-dir" aria-hidden="true">{{ link.direction === "outgoing" ? "→" : "←" }}</span>
            <span class="link-rel">{{ link.relationship }}</span>
            <span class="link-title">{{ linkTitle(link) }}</span>
          </button>
        </div>

        <div v-if="links.some((l) => !isHistory(l))" class="links-items">
          <button
            v-for="link in links.filter((l) => !isHistory(l))"
            :key="link.from_memory_id + link.relationship + link.to_memory_id"
            class="link-item"
            :title="rationale(link) ?? undefined"
            @click="openLinked(otherId(link))"
          >
            <span class="link-dir" aria-hidden="true">{{ link.direction === "outgoing" ? "→" : "←" }}</span>
            <span class="link-rel">{{ link.relationship || "relates_to" }}</span>
            <span class="link-title">{{ linkTitle(link) }}</span>
            <span v-if="rationale(link)" class="link-rationale">{{ rationale(link) }}</span>
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
  border-top: 1px solid var(--color-border-subtle);
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
  color: var(--color-text-tertiary);
  cursor: pointer;
  transition: color 150ms;
  padding: var(--space-1) 0;
}

.links-toggle:hover {
  color: var(--color-text-primary);
}

.links-chevron {
  transition: transform 200ms cubic-bezier(0.4, 0, 0.2, 1);
}

.links-chevron.open {
  transform: rotate(90deg);
}

.links-title {
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
}

.links-count {
  font-size: 11px;
  color: var(--color-text-tertiary);
  font-variant-numeric: tabular-nums;
}

.links-body {
  margin-top: var(--space-3);
}

.loading-bar {
  margin-bottom: var(--space-3);
}

/* ── Error ── */
.suggest-error {
  font-size: 13px;
  color: var(--color-danger);
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
  border-bottom: 1px solid var(--color-border-subtle);
  cursor: pointer;
  transition: background 150ms;
  width: 100%;
  text-align: left;
}

.link-item:last-child {
  border-bottom: none;
}

.link-item:hover {
  background: var(--color-surface-hover);
}

.link-rel {
  font-size: 11px;
  color: var(--color-text-tertiary);
  font-weight: 500;
}

.link-dir {
  font-size: 12px;
  color: var(--color-text-tertiary);
}

.link-title {
  font-size: 13px;
  color: var(--color-text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.link-rationale {
  font-size: 11px;
  color: var(--color-text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.history-items {
  display: flex;
  flex-direction: column;
  margin-bottom: var(--space-2);
}

.suggestions {
  display: flex;
  flex-direction: column;
}

.suggestions-label {
  font-size: 11px;
  color: var(--color-text-tertiary);
  text-transform: uppercase;
  letter-spacing: 0.06em;
  font-weight: 600;
  margin-bottom: var(--space-1);
}

.suggestion-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-2) var(--space-1);
  background: none;
  border: none;
  border-bottom: 1px dashed var(--color-border-subtle);
  cursor: pointer;
  transition: background 150ms;
  width: 100%;
  text-align: left;
}

.suggestion-item:last-child {
  border-bottom: none;
}

.suggestion-item:hover {
  background: var(--color-accent-subtle);
}

.suggestion-title {
  font-size: 13px;
  color: var(--color-text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 80%;
}

.suggestion-score {
  font-size: 11px;
  color: var(--color-text-tertiary);
  font-weight: 500;
  font-variant-numeric: tabular-nums;
}

.links-empty {
  font-size: 13px;
  color: var(--color-text-tertiary);
}
</style>

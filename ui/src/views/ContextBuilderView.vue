<script setup lang="ts">
import { ref, computed, watch, onMounted, nextTick } from "vue";
import { useMemoryStore } from "@/stores/memories";
import * as api from "@/api/memory";
import type { RecallItem } from "@/api/types";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";

// ── Types ──

interface BriefBlock {
  id: string;
  type: "memory" | "heading" | "narrative";
  // memory block
  memoryId?: string;
  memoryTitle?: string | null;
  memoryContent?: string;
  memoryNamespace?: string;
  memoryKind?: string;
  memoryTags?: string[];
  // heading / narrative block
  text?: string;
}

// ── State ──

const store = useMemoryStore();

const blocks = ref<BriefBlock[]>(loadState());
const searchQuery = ref("");
const searchResults = ref<RecallItem[]>([]);
const searchLoading = ref(false);
const showSearch = ref(false);
const exportMessage = ref<string | null>(null);
const searchInputRef = ref<HTMLInputElement | null>(null);

const MAX_BLOCKS = 50;

const memoryCount = computed(
  () => blocks.value.filter((b) => b.type === "memory").length,
);

const isEmpty = computed(() => blocks.value.length === 0);

// ── Persistence (session-scoped via sessionStorage) ──

function loadState(): BriefBlock[] {
  try {
    const raw = sessionStorage.getItem("clio-context-builder");
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
}

function saveState() {
  sessionStorage.setItem(
    "clio-context-builder",
    JSON.stringify(blocks.value),
  );
}

watch(blocks, saveState, { deep: true });

// ── Block manipulation ──

function generateId(): string {
  return Date.now().toString(36) + Math.random().toString(36).slice(2, 6);
}

function addMemoryBlock(item: RecallItem) {
  if (blocks.value.length >= MAX_BLOCKS) return;
  // Prevent adding the same memory twice
  if (blocks.value.some((b) => b.memoryId === item.id)) return;

  blocks.value.push({
    id: generateId(),
    type: "memory",
    memoryId: item.id,
    memoryTitle: item.title,
    memoryContent: item.content,
    memoryNamespace: item.namespace,
    memoryKind: item.kind,
    memoryTags: item.tags,
  });
}

function addHeading() {
  if (blocks.value.length >= MAX_BLOCKS) return;
  blocks.value.push({
    id: generateId(),
    type: "heading",
    text: "",
  });
}

function addNarrative() {
  if (blocks.value.length >= MAX_BLOCKS) return;
  blocks.value.push({
    id: generateId(),
    type: "narrative",
    text: "",
  });
}

function removeBlock(blockId: string) {
  blocks.value = blocks.value.filter((b) => b.id !== blockId);
}

function moveBlockUp(index: number) {
  if (index <= 0) return;
  const arr = [...blocks.value];
  [arr[index - 1], arr[index]] = [arr[index], arr[index - 1]];
  blocks.value = arr;
}

function moveBlockDown(index: number) {
  if (index >= blocks.value.length - 1) return;
  const arr = [...blocks.value];
  [arr[index], arr[index + 1]] = [arr[index + 1], arr[index]];
  blocks.value = arr;
}

function clearAll() {
  blocks.value = [];
  sessionStorage.removeItem("clio-context-builder");
}

// ── Search ──

let searchTimer: ReturnType<typeof setTimeout> | null = null;

function onSearchInput(query: string) {
  searchQuery.value = query;
  if (searchTimer) clearTimeout(searchTimer);
  if (!query.trim()) {
    searchResults.value = [];
    return;
  }
  searchTimer = setTimeout(async () => {
    searchLoading.value = true;
    try {
      const result = await api.recall({
        query,
        namespace: store.selectedNamespace ?? undefined,
        limit: 15,
      });
      searchResults.value = result.items;
    } catch {
      searchResults.value = [];
    } finally {
      searchLoading.value = false;
    }
  }, 200);
}

function openSearch() {
  showSearch.value = true;
  nextTick(() => searchInputRef.value?.focus());
}

function closeSearch() {
  showSearch.value = false;
  searchQuery.value = "";
  searchResults.value = [];
}

function selectSearchResult(item: RecallItem) {
  addMemoryBlock(item);
}

function isAlreadyAdded(memoryId: string): boolean {
  return blocks.value.some((b) => b.memoryId === memoryId);
}

// ── Export ──

function buildMarkdown(): string {
  const lines: string[] = [];
  lines.push("# Knowledge Brief");
  lines.push("");
  lines.push(
    `*Generated from Clio on ${new Date().toLocaleDateString("en-GB", { day: "numeric", month: "long", year: "numeric" })}*`,
  );
  lines.push("");

  for (const block of blocks.value) {
    if (block.type === "heading") {
      lines.push(`## ${block.text || "Untitled section"}`);
      lines.push("");
    } else if (block.type === "narrative") {
      if (block.text) {
        lines.push(block.text);
        lines.push("");
      }
    } else if (block.type === "memory") {
      if (block.memoryTitle) {
        lines.push(`### ${block.memoryTitle}`);
      }
      lines.push("");
      const meta: string[] = [];
      if (block.memoryNamespace) meta.push(`**Namespace:** ${block.memoryNamespace}`);
      if (block.memoryKind) meta.push(`**Kind:** ${block.memoryKind}`);
      if (block.memoryTags?.length) meta.push(`**Tags:** ${block.memoryTags.join(", ")}`);
      if (meta.length) {
        lines.push(meta.join(" · "));
        lines.push("");
      }
      if (block.memoryContent) {
        lines.push(block.memoryContent);
        lines.push("");
      }
      lines.push("---");
      lines.push("");
    }
  }

  return lines.join("\n");
}

async function exportToClipboard() {
  const md = buildMarkdown();
  try {
    await writeText(md);
    exportMessage.value = "Copied to clipboard";
  } catch {
    // Fallback to navigator.clipboard
    try {
      await navigator.clipboard.writeText(md);
      exportMessage.value = "Copied to clipboard";
    } catch {
      exportMessage.value = "Failed to copy — check clipboard permissions";
    }
  }
  setTimeout(() => (exportMessage.value = null), 2500);
}

function exportToFile() {
  const md = buildMarkdown();
  const blob = new Blob([md], { type: "text/markdown;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  const ts = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
  a.download = `clio-brief-${ts}.md`;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
  exportMessage.value = "Downloaded as Markdown";
  setTimeout(() => (exportMessage.value = null), 2500);
}

function truncate(text: string, max = 150): string {
  if (text.length <= max) return text;
  return text.slice(0, max) + "\u2026";
}

// ── Drag and drop state ──

const dragIndex = ref<number | null>(null);
const dragOverIndex = ref<number | null>(null);

function onDragStart(index: number) {
  dragIndex.value = index;
}

function onDragOver(e: DragEvent, index: number) {
  e.preventDefault();
  dragOverIndex.value = index;
}

function onDrop(index: number) {
  if (dragIndex.value === null || dragIndex.value === index) {
    dragIndex.value = null;
    dragOverIndex.value = null;
    return;
  }
  const arr = [...blocks.value];
  const [moved] = arr.splice(dragIndex.value, 1);
  arr.splice(index, 0, moved);
  blocks.value = arr;
  dragIndex.value = null;
  dragOverIndex.value = null;
}

function onDragEnd() {
  dragIndex.value = null;
  dragOverIndex.value = null;
}

// ── Add memories from current list via drag from memory list ──

onMounted(() => {
  // Load any previously saved state
});
</script>

<template>
  <div class="builder-view">
    <div class="builder-header">
      <h1 class="builder-title">Context Builder</h1>
      <p class="builder-desc">
        Assemble memories into a curated knowledge brief. Add section headings and narrative text between memories, then export as Markdown.
      </p>
    </div>

    <!-- Toolbar -->
    <div class="builder-toolbar">
      <div class="toolbar-left">
        <button class="tool-btn" @click="openSearch">
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
            <circle cx="7" cy="7" r="4.5" stroke="currentColor" stroke-width="1.2"/>
            <path d="M10.5 10.5L14 14" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
          </svg>
          Add memories
        </button>
        <button class="tool-btn" @click="addHeading">
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
            <path d="M3 3v10M13 3v10M3 8h10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
          Heading
        </button>
        <button class="tool-btn" @click="addNarrative">
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
            <path d="M3 4h10M3 8h7M3 12h10" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
          </svg>
          Text
        </button>
      </div>

      <div class="toolbar-right">
        <span v-if="memoryCount > 0" class="block-count">
          {{ memoryCount }}/{{ MAX_BLOCKS }} memories
        </span>
        <button
          v-if="!isEmpty"
          class="tool-btn"
          @click="exportToClipboard"
        >
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
            <rect x="4" y="2" width="8" height="10" rx="1" stroke="currentColor" stroke-width="1.2"/>
            <path d="M4 6H3a1 1 0 00-1 1v6a1 1 0 001 1h6a1 1 0 001-1v-1" stroke="currentColor" stroke-width="1.2"/>
          </svg>
          Copy
        </button>
        <button
          v-if="!isEmpty"
          class="tool-btn primary"
          @click="exportToFile"
        >
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
            <path d="M8 2v8M5 7l3 3 3-3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>
            <path d="M2 12v1a1 1 0 001 1h10a1 1 0 001-1v-1" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
          </svg>
          Export
        </button>
        <button
          v-if="!isEmpty"
          class="tool-btn danger-ghost"
          @click="clearAll"
        >
          Clear
        </button>
      </div>
    </div>

    <!-- Export message -->
    <div v-if="exportMessage" class="export-msg">{{ exportMessage }}</div>

    <!-- Search panel -->
    <Transition name="expand">
      <div v-if="showSearch" class="search-panel">
        <div class="search-header">
          <input
            ref="searchInputRef"
            type="text"
            class="search-input"
            placeholder="Search memories to add…"
            :value="searchQuery"
            @input="onSearchInput(($event.target as HTMLInputElement).value)"
            @keydown.escape="closeSearch"
          />
          <button class="tool-btn-sm" @click="closeSearch">Close</button>
        </div>

        <div v-if="searchLoading" class="search-status">Searching…</div>
        <div v-else-if="searchQuery && !searchResults.length" class="search-status">
          No results found
        </div>

        <div v-if="searchResults.length" class="search-results">
          <button
            v-for="item in searchResults"
            :key="item.id"
            class="search-result"
            :class="{ added: isAlreadyAdded(item.id) }"
            :disabled="isAlreadyAdded(item.id)"
            @click="selectSearchResult(item)"
          >
            <div class="result-title">
              {{ item.title || truncate(item.content, 80) }}
            </div>
            <div class="result-meta">
              <span class="result-ns">{{ item.namespace }}</span>
              <span class="result-kind">{{ item.kind }}</span>
              <span v-if="isAlreadyAdded(item.id)" class="result-added">Added</span>
            </div>
          </button>
        </div>
      </div>
    </Transition>

    <!-- Empty state -->
    <div v-if="isEmpty && !showSearch" class="empty-state">
      <svg width="48" height="48" viewBox="0 0 48 48" fill="none">
        <rect x="6" y="10" width="36" height="28" rx="3" stroke="currentColor" stroke-width="1.5" opacity="0.3"/>
        <path d="M14 20h20M14 26h14M14 32h18" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" opacity="0.3"/>
      </svg>
      <p class="empty-title">Start building a brief</p>
      <p class="empty-desc">
        Search for memories to add, arrange them in order, and add section headings or narrative text between entries.
      </p>
      <button class="tool-btn primary" @click="openSearch">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <circle cx="7" cy="7" r="4.5" stroke="currentColor" stroke-width="1.2"/>
          <path d="M10.5 10.5L14 14" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
        </svg>
        Search memories
      </button>
    </div>

    <!-- Blocks list -->
    <div v-if="!isEmpty" class="blocks-list">
      <div
        v-for="(block, index) in blocks"
        :key="block.id"
        class="block-item"
        :class="{
          'block-memory': block.type === 'memory',
          'block-heading': block.type === 'heading',
          'block-narrative': block.type === 'narrative',
          'drag-over': dragOverIndex === index && dragIndex !== index,
        }"
        draggable="true"
        @dragstart="onDragStart(index)"
        @dragover="onDragOver($event, index)"
        @drop="onDrop(index)"
        @dragend="onDragEnd"
      >
        <!-- Drag handle + controls -->
        <div class="block-controls">
          <span class="drag-handle" title="Drag to reorder">
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
              <circle cx="4" cy="2.5" r="1" fill="currentColor"/>
              <circle cx="8" cy="2.5" r="1" fill="currentColor"/>
              <circle cx="4" cy="6" r="1" fill="currentColor"/>
              <circle cx="8" cy="6" r="1" fill="currentColor"/>
              <circle cx="4" cy="9.5" r="1" fill="currentColor"/>
              <circle cx="8" cy="9.5" r="1" fill="currentColor"/>
            </svg>
          </span>
          <button
            class="block-ctrl-btn"
            title="Move up"
            :disabled="index === 0"
            @click="moveBlockUp(index)"
          >
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
              <path d="M6 2v8M3 5l3-3 3 3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>
            </svg>
          </button>
          <button
            class="block-ctrl-btn"
            title="Move down"
            :disabled="index === blocks.length - 1"
            @click="moveBlockDown(index)"
          >
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
              <path d="M6 10V2M3 7l3 3 3-3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>
            </svg>
          </button>
          <button
            class="block-ctrl-btn danger"
            title="Remove"
            @click="removeBlock(block.id)"
          >
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
              <path d="M3 3l6 6M9 3l-6 6" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
            </svg>
          </button>
        </div>

        <!-- Memory block -->
        <div v-if="block.type === 'memory'" class="block-body">
          <div class="memory-block-title">
            {{ block.memoryTitle || "Untitled memory" }}
          </div>
          <div class="memory-block-content">
            {{ truncate(block.memoryContent || "", 200) }}
          </div>
          <div class="memory-block-meta">
            <span class="meta-ns">{{ block.memoryNamespace }}</span>
            <span class="meta-kind">{{ block.memoryKind }}</span>
            <span v-if="block.memoryTags?.length" class="meta-tags">
              {{ block.memoryTags.join(", ") }}
            </span>
          </div>
        </div>

        <!-- Heading block -->
        <div v-if="block.type === 'heading'" class="block-body">
          <input
            v-model="block.text"
            type="text"
            class="heading-input"
            placeholder="Section heading…"
          />
        </div>

        <!-- Narrative block -->
        <div v-if="block.type === 'narrative'" class="block-body">
          <textarea
            v-model="block.text"
            class="narrative-input"
            placeholder="Bridging narrative or notes…"
            rows="3"
          />
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.builder-view {
  padding-bottom: var(--space-12);
}

.builder-header {
  margin-bottom: var(--space-6);
}

.builder-title {
  font-size: var(--text-xl);
  font-weight: var(--font-semibold);
  letter-spacing: var(--tracking-tight);
  color: var(--colour-text);
  margin-bottom: var(--space-2);
}

.builder-desc {
  font-size: var(--text-sm);
  color: var(--colour-text-muted);
  line-height: var(--leading-relaxed);
  max-width: 600px;
}

/* ── Toolbar ── */
.builder-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  margin-bottom: var(--space-4);
  flex-wrap: wrap;
}

.toolbar-left,
.toolbar-right {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.tool-btn {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  background: var(--colour-surface-overlay);
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-md);
  color: var(--colour-text);
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  cursor: pointer;
  transition: all 150ms;
  white-space: nowrap;
}

.tool-btn:hover:not(:disabled) {
  border-color: var(--colour-border-hover);
  background: var(--colour-surface-input);
}

.tool-btn:disabled {
  opacity: 0.5;
  cursor: default;
}

.tool-btn.primary {
  background: var(--colour-accent);
  border-color: var(--colour-accent);
  color: white;
}

.tool-btn.primary:hover:not(:disabled) {
  background: var(--colour-accent-hover);
}

.tool-btn.danger-ghost {
  color: var(--colour-danger);
  border-color: transparent;
  background: transparent;
}

.tool-btn.danger-ghost:hover {
  background: color-mix(in srgb, var(--colour-danger) 10%, transparent);
}

.tool-btn-sm {
  padding: var(--space-1) var(--space-3);
  background: none;
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-sm);
  color: var(--colour-text-muted);
  font-size: var(--text-xs);
  cursor: pointer;
  transition: all 150ms;
  white-space: nowrap;
}

.tool-btn-sm:hover {
  color: var(--colour-text);
  border-color: var(--colour-border-hover);
}

.block-count {
  font-size: var(--text-xs);
  color: var(--colour-text-muted);
  font-variant-numeric: tabular-nums;
}

.export-msg {
  padding: var(--space-2) var(--space-3);
  background: color-mix(in srgb, var(--colour-success) 10%, transparent);
  border: 1px solid color-mix(in srgb, var(--colour-success) 20%, transparent);
  border-radius: var(--radius-md);
  color: var(--colour-success);
  font-size: var(--text-sm);
  margin-bottom: var(--space-4);
}

/* ── Search Panel ── */
.search-panel {
  padding: var(--space-4);
  background: var(--colour-surface-card);
  border: 1px solid var(--glass-border);
  border-radius: var(--radius-lg);
  margin-bottom: var(--space-4);
}

.search-header {
  display: flex;
  gap: var(--space-2);
  margin-bottom: var(--space-3);
}

.search-input {
  flex: 1;
  padding: var(--space-2) var(--space-3);
  background: var(--colour-surface-input);
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-md);
  color: var(--colour-text);
  font-size: var(--text-sm);
  outline: none;
  transition: border-color 150ms;
}

.search-input:focus {
  border-color: var(--colour-accent);
}

.search-input::placeholder {
  color: var(--colour-text-muted);
}

.search-status {
  font-size: var(--text-sm);
  color: var(--colour-text-muted);
  padding: var(--space-2) 0;
}

.search-results {
  display: flex;
  flex-direction: column;
  gap: 2px;
  max-height: 300px;
  overflow-y: auto;
}

.search-result {
  display: block;
  width: 100%;
  text-align: left;
  padding: var(--space-2) var(--space-3);
  background: transparent;
  border: none;
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: background 100ms;
}

.search-result:hover:not(:disabled) {
  background: var(--colour-surface-hover);
}

.search-result.added {
  opacity: 0.5;
  cursor: default;
}

.result-title {
  font-size: var(--text-sm);
  color: var(--colour-text);
  margin-bottom: 2px;
}

.result-meta {
  display: flex;
  gap: var(--space-2);
  font-size: var(--text-xs);
  color: var(--colour-text-muted);
}

.result-ns {
  color: var(--colour-accent);
}

.result-kind {
  text-transform: capitalize;
}

.result-added {
  color: var(--colour-success);
  font-weight: var(--font-medium);
}

/* ── Empty State ── */
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: var(--space-12) var(--space-6);
  text-align: center;
  color: var(--colour-text-muted);
}

.empty-title {
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--colour-text);
  margin: var(--space-4) 0 var(--space-2);
}

.empty-desc {
  font-size: var(--text-sm);
  color: var(--colour-text-muted);
  max-width: 380px;
  line-height: var(--leading-relaxed);
  margin-bottom: var(--space-5);
}

/* ── Blocks List ── */
.blocks-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.block-item {
  display: flex;
  gap: var(--space-3);
  padding: var(--space-3);
  background: var(--colour-surface-card);
  border: 1px solid var(--glass-border);
  border-radius: var(--radius-lg);
  transition: border-color 150ms, box-shadow 150ms;
}

.block-item.drag-over {
  border-color: var(--colour-accent);
  box-shadow: 0 0 0 1px var(--colour-accent);
}

.block-controls {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  padding-top: 2px;
}

.drag-handle {
  cursor: grab;
  color: var(--colour-text-muted);
  padding: 2px;
  opacity: 0.5;
  transition: opacity 150ms;
}

.block-item:hover .drag-handle {
  opacity: 1;
}

.block-ctrl-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  background: none;
  border: none;
  border-radius: var(--radius-sm);
  color: var(--colour-text-muted);
  cursor: pointer;
  transition: all 100ms;
}

.block-ctrl-btn:hover:not(:disabled) {
  color: var(--colour-text);
  background: var(--colour-surface-hover);
}

.block-ctrl-btn:disabled {
  opacity: 0.3;
  cursor: default;
}

.block-ctrl-btn.danger:hover {
  color: var(--colour-danger);
  background: color-mix(in srgb, var(--colour-danger) 10%, transparent);
}

.block-body {
  flex: 1;
  min-width: 0;
}

/* Memory block */
.memory-block-title {
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--colour-text);
  margin-bottom: var(--space-1);
}

.memory-block-content {
  font-size: var(--text-sm);
  color: var(--colour-text-secondary);
  line-height: var(--leading-relaxed);
  margin-bottom: var(--space-2);
  white-space: pre-wrap;
  word-break: break-word;
}

.memory-block-meta {
  display: flex;
  gap: var(--space-2);
  font-size: var(--text-xs);
  color: var(--colour-text-muted);
}

.meta-ns {
  color: var(--colour-accent);
}

.meta-kind {
  text-transform: capitalize;
}

.meta-tags {
  opacity: 0.7;
}

/* Heading block */
.heading-input {
  width: 100%;
  padding: var(--space-2) var(--space-3);
  background: transparent;
  border: 1px solid transparent;
  border-radius: var(--radius-md);
  color: var(--colour-text);
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  outline: none;
  transition: border-color 150ms;
}

.heading-input:focus {
  border-color: var(--colour-border);
  background: var(--colour-surface-input);
}

.heading-input::placeholder {
  color: var(--colour-text-muted);
  font-weight: var(--font-normal);
}

/* Narrative block */
.narrative-input {
  width: 100%;
  padding: var(--space-2) var(--space-3);
  background: transparent;
  border: 1px solid transparent;
  border-radius: var(--radius-md);
  color: var(--colour-text);
  font-size: var(--text-sm);
  line-height: var(--leading-relaxed);
  resize: vertical;
  outline: none;
  transition: border-color 150ms;
  font-family: inherit;
}

.narrative-input:focus {
  border-color: var(--colour-border);
  background: var(--colour-surface-input);
}

.narrative-input::placeholder {
  color: var(--colour-text-muted);
}

/* ── Transitions ── */
.expand-enter-active,
.expand-leave-active {
  transition: all 200ms ease;
  overflow: hidden;
}

.expand-enter-from,
.expand-leave-to {
  opacity: 0;
  max-height: 0;
}

.expand-enter-to,
.expand-leave-from {
  opacity: 1;
  max-height: 500px;
}
</style>

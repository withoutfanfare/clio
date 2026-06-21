<script setup lang="ts">
import { ref, computed, nextTick, onMounted, onUnmounted, watch } from "vue";
import { SCard, SBadge, STag, SDropdownMenu, SButton } from "@stuntrocket/ui";
import type { SDropdownMenuItem } from "@stuntrocket/ui";
import { useMemoryStore } from "@/stores/memories";
import { useNamespaceColours } from "@/composables/useNamespaceColours";
import { copyToClipboard, downloadMarkdown } from "@/utils/memoryExport";
import type { RecallItem } from "@/api/types";

const props = defineProps<{
  memory: RecallItem;
  mode?: "list" | "grid";
  focused?: boolean;
}>();

const store = useMemoryStore();
const { getColour } = useNamespaceColours();
const copied = ref(false);
const confirmingDelete = ref(false);

const nsColour = computed(() => getColour(props.memory.namespace));

const cardRef = ref<any>(null);

// Scroll the focused card into view when keyboard navigation moves focus to it.
watch(
  () => props.focused,
  (isFocused) => {
    if (isFocused) {
      nextTick(() => cardRef.value?.$el?.scrollIntoView?.({ block: "nearest" }));
    }
  },
);

function open(e: MouseEvent) {
  // Cmd/Ctrl+click toggles selection
  if (e.metaKey || e.ctrlKey) {
    e.preventDefault();
    store.toggleSelection(props.memory.id);
    return;
  }
  // Shift+click extends selection range
  if (e.shiftKey && store.selectionMode) {
    e.preventDefault();
    const myIndex = store.navigableItems.findIndex((i) => i.id === props.memory.id);
    if (myIndex >= 0) {
      const lastSelected = store.navigableItems.findIndex(
        (i) => store.isSelected(i.id) && i.id !== props.memory.id,
      );
      if (lastSelected >= 0) {
        store.selectRange(lastSelected, myIndex);
      } else {
        store.toggleSelection(props.memory.id);
      }
    }
    return;
  }
  // In selection mode, simple click toggles
  if (store.selectionMode) {
    store.toggleSelection(props.memory.id);
    return;
  }
  store.openDrawer(props.memory.id);
}

const isItemSelected = computed(() => store.isSelected(props.memory.id));

async function onCopy() {
  const ok = await copyToClipboard(props.memory);
  if (ok) {
    copied.value = true;
    setTimeout(() => (copied.value = false), 1500);
  }
}

function onDownload() {
  downloadMarkdown(props.memory);
}

function onTogglePin() {
  store.togglePin(props.memory.id);
}

async function onArchive() {
  if (props.memory.archived_at) {
    await store.unarchiveMemory(props.memory.id);
  } else {
    await store.archiveMemory(props.memory.id);
  }
}

async function onDelete() {
  if (!confirmingDelete.value) {
    confirmingDelete.value = true;
    return;
  }
  await store.deleteMemory(props.memory.id);
  confirmingDelete.value = false;
}

const menuItems = computed<SDropdownMenuItem[]>(() => [
  {
    label: store.isPinned(props.memory.id) ? "Unpin" : "Pin to top",
    value: "pin",
  },
  {
    label: copied.value ? "Copied!" : "Copy as Markdown",
    value: "copy",
  },
  {
    label: "Download .md",
    value: "download",
  },
  {
    label: props.memory.archived_at ? "Unarchive" : "Archive",
    value: "archive",
  },
  {
    label: confirmingDelete.value ? "Confirm delete" : "Delete",
    value: "delete",
    danger: true,
  },
]);

function handleMenuSelect(value: string) {
  switch (value) {
    case "pin":
      onTogglePin();
      break;
    case "copy":
      onCopy();
      break;
    case "download":
      onDownload();
      break;
    case "archive":
      onArchive();
      break;
    case "delete":
      onDelete();
      break;
  }
}

function formatTime(iso: string): string {
  const d = new Date(iso);
  return d.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
}
</script>

<template>
  <SCard
    ref="cardRef"
    variant="glass"
    hoverable
    class="memory-page"
    :class="[mode === 'grid' ? 'mode-grid' : 'mode-list', { 'kb-focused': focused, 'is-selected': isItemSelected }]"
    :style="{ '--ns-colour': nsColour }"
    @click="open"
    tabindex="0"
    @keydown.enter="open($event as unknown as MouseEvent)"
  >
    <div class="page-header">
      <div class="page-header-row">
        <h3 class="page-title" v-if="memory.title">{{ memory.title }}</h3>
        <div class="page-menu-wrapper" @click.stop>
          <SDropdownMenu
            :items="menuItems"
            align="right"
            @select="handleMenuSelect"
          >
            <template #trigger="{ toggle, open: menuOpen }">
              <SButton
                variant="icon"
                size="sm"
                class="page-menu-btn"
                :class="{ 'menu-visible': menuOpen }"
                @click="toggle"
                aria-label="More options"
              >
                <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
                  <circle cx="8" cy="3" r="1.5" fill="currentColor"/>
                  <circle cx="8" cy="8" r="1.5" fill="currentColor"/>
                  <circle cx="8" cy="13" r="1.5" fill="currentColor"/>
                </svg>
              </SButton>
            </template>
          </SDropdownMenu>
        </div>
      </div>
      <div class="page-meta">
        <SBadge variant="default">{{ memory.kind }}</SBadge>
        <span class="meta-importance" :title="`Importance ${memory.importance}/5`">
          <span
            v-for="n in 5"
            :key="n"
            class="imp-dot"
            :class="n <= memory.importance ? 'imp-on' : 'imp-off'"
          />
        </span>
        <span class="meta-sep" v-if="memory.namespace !== 'global'">&middot;</span>
        <span class="meta-ns" v-if="memory.namespace !== 'global'">
          <span class="ns-dot" :style="{ background: nsColour }" />
          {{ memory.namespace }}
        </span>
        <span v-if="memory.source" class="meta-source">{{ memory.source }}</span>
        <span class="meta-time">{{ formatTime(memory.updated_at) }}</span>
      </div>
    </div>

    <p class="page-content">{{ memory.content }}</p>

    <div class="page-tags" v-if="memory.tags.length">
      <STag v-for="tag in memory.tags" :key="tag">#{{ tag }}</STag>
    </div>
  </SCard>
</template>

<style scoped>
.memory-page {
  cursor: pointer;
  border-left: 3px solid var(--ns-colour, transparent);
}

.ns-dot {
  display: inline-block;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  vertical-align: middle;
  margin-right: 2px;
}

.memory-page:focus-visible,
.memory-page.kb-focused {
  outline: 2px solid color-mix(in srgb, var(--color-accent) 55%, transparent);
  outline-offset: 2px;
}

.memory-page.is-selected {
  border-color: var(--colour-accent);
  box-shadow: 0 0 0 1px var(--colour-accent), var(--glass-glow);
  background: color-mix(in srgb, var(--colour-accent) 5%, var(--colour-surface-card));
}

/* ── List mode ── */
.mode-list {
  padding: var(--space-4) var(--space-5);
  flex-shrink: 0;
  overflow: hidden;
}

.mode-list .page-content {
  -webkit-line-clamp: 2;
}

/* ── Grid mode ── */
.mode-grid {
  padding: var(--space-3) var(--space-4);
  display: flex;
  flex-direction: column;
  height: 160px;
  overflow: hidden;
}

.mode-grid .page-header {
  margin-bottom: var(--space-1);
  flex-shrink: 0;
}

.mode-grid .page-title {
  font-size: 11px;
  margin-bottom: 2px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.mode-grid .page-meta {
  flex-wrap: nowrap;
  overflow: hidden;
}

.mode-grid .meta-ns,
.mode-grid .meta-sep {
  display: none;
}

.mode-grid .page-content {
  font-size: 11px;
  -webkit-line-clamp: 2;
  line-height: 1.5;
  flex: 1;
  min-height: 0;
}

.mode-grid .page-tags {
  margin-top: auto;
  padding-top: var(--space-1);
  flex-shrink: 0;
  overflow: hidden;
  white-space: nowrap;
}

/* ── Header ── */
.page-header {
  margin-bottom: var(--space-2);
}

.page-header-row {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-2);
}

.page-title {
  font-size: 15px;
  font-weight: 500;
  line-height: 1.3;
  letter-spacing: -0.01em;
  color: var(--color-text-primary);
  margin-bottom: var(--space-1);
  flex: 1;
  min-width: 0;
}

/* ── Three-dot menu ── */
.page-menu-wrapper {
  position: relative;
  flex-shrink: 0;
}

.page-menu-btn {
  opacity: 0;
  transition: opacity 150ms;
}

.memory-page:hover .page-menu-btn,
.page-menu-btn.menu-visible {
  opacity: 1;
}

.page-meta {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  color: var(--color-text-tertiary);
}

/* ── Importance dots ── */
.meta-importance {
  display: inline-flex;
  align-items: center;
  gap: 2px;
}

.imp-dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  transition: background 150ms;
}

.imp-off {
  background: var(--color-surface-hover);
}

.imp-on {
  background: var(--color-accent);
}

.meta-sep {
  color: var(--colour-text-disabled);
}

.meta-ns {
  color: var(--colour-text-disabled);
}

.meta-source {
  display: inline-flex;
  align-items: center;
  padding: 0px 4px;
  border-radius: 3px;
  background: var(--colour-surface-overlay);
  color: var(--colour-text-disabled);
  font-size: 9px;
  font-weight: var(--font-medium);
  text-transform: uppercase;
  letter-spacing: var(--tracking-caps);
}

.meta-time {
  margin-left: auto;
  font-variant-numeric: tabular-nums;
}

/* ── Content ── */
.page-content {
  font-size: 13px;
  line-height: 1.65;
  color: var(--color-text-secondary);
  white-space: pre-wrap;
  word-break: break-word;
  display: -webkit-box;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.page-tags {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  margin-top: var(--space-3);
}
</style>

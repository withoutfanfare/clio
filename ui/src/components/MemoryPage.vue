<script setup lang="ts">
import { ref, nextTick, onMounted, onUnmounted } from "vue";
import { useMemoryStore } from "@/stores/memories";
import { copyToClipboard, downloadMarkdown } from "@/utils/memoryExport";
import * as api from "@/api/memory";
import type { RecallItem } from "@/api/types";

const props = defineProps<{
  memory: RecallItem;
  mode?: "list" | "grid";
}>();

const store = useMemoryStore();
const menuOpen = ref(false);
const confirmingDelete = ref(false);
const copied = ref(false);
const btnRef = ref<HTMLElement | null>(null);
const dropdownRef = ref<HTMLElement | null>(null);
const menuStyle = ref({ top: "0px", left: "0px" });

function positionMenu() {
  if (!btnRef.value) return;
  const rect = btnRef.value.getBoundingClientRect();
  menuStyle.value = {
    top: `${rect.bottom + 4}px`,
    left: `${rect.right}px`,
  };
}

function handleClickOutside(e: MouseEvent) {
  if (!menuOpen.value) return;
  const target = e.target as Node;
  if (btnRef.value?.contains(target)) return;
  if (dropdownRef.value?.contains(target)) return;
  closeMenu();
}

onMounted(() => document.addEventListener("pointerdown", handleClickOutside, true));
onUnmounted(() => document.removeEventListener("pointerdown", handleClickOutside, true));

function open() {
  if (menuOpen.value) return;
  store.openDrawer(props.memory.id);
}

function toggleMenu(e: Event) {
  e.stopPropagation();
  if (menuOpen.value) {
    closeMenu();
  } else {
    positionMenu();
    menuOpen.value = true;
    confirmingDelete.value = false;
  }
}

function closeMenu() {
  menuOpen.value = false;
  confirmingDelete.value = false;
}

async function onCopy(e: Event) {
  e.stopPropagation();
  const ok = await copyToClipboard(props.memory);
  if (ok) {
    copied.value = true;
    setTimeout(() => (copied.value = false), 1500);
  }
  closeMenu();
}

function onDownload(e: Event) {
  e.stopPropagation();
  downloadMarkdown(props.memory);
  closeMenu();
}

async function onArchive(e: Event) {
  e.stopPropagation();
  try {
    if (props.memory.archived_at) {
      await api.unarchive(props.memory.id);
    } else {
      await api.archive(props.memory.id);
    }
    store.loadRecent();
  } catch {
    // Archive failed
  }
  closeMenu();
}

async function onDelete(e: Event) {
  e.stopPropagation();
  if (!confirmingDelete.value) {
    confirmingDelete.value = true;
    return;
  }
  try {
    await api.deleteMemory(props.memory.id);
    store.loadRecent();
  } catch {
    // Delete failed
  }
  closeMenu();
}

function formatTime(iso: string): string {
  const d = new Date(iso);
  return d.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
}
</script>

<template>
  <article
    class="memory-page"
    :class="mode === 'grid' ? 'mode-grid' : 'mode-list'"
    @click="open"
    tabindex="0"
    @keydown.enter="open"
  >
    <div class="page-header">
      <div class="page-header-row">
        <h3 class="page-title" v-if="memory.title">{{ memory.title }}</h3>
        <div class="page-menu-wrapper">
          <button
            ref="btnRef"
            class="page-menu-btn"
            :class="{ 'menu-visible': menuOpen }"
            @click="toggleMenu"
            aria-label="More options"
          >
            <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
              <circle cx="8" cy="3" r="1.5" fill="currentColor"/>
              <circle cx="8" cy="8" r="1.5" fill="currentColor"/>
              <circle cx="8" cy="13" r="1.5" fill="currentColor"/>
            </svg>
          </button>
        </div>
      </div>
      <div class="page-meta">
        <span class="meta-kind-pill">{{ memory.kind }}</span>
        <span class="meta-importance" :title="`Importance ${memory.importance}/5`">
          <span
            v-for="n in 5"
            :key="n"
            class="imp-dot"
            :class="[n <= memory.importance ? `imp-${memory.importance}` : 'imp-off']"
          />
        </span>
        <span class="meta-sep" v-if="memory.namespace !== 'global'">&middot;</span>
        <span class="meta-ns" v-if="memory.namespace !== 'global'">{{ memory.namespace }}</span>
        <span class="meta-time">{{ formatTime(memory.updated_at) }}</span>
      </div>
    </div>

    <p class="page-content">{{ memory.content }}</p>

    <div class="page-tags" v-if="memory.tags.length">
      <span v-for="tag in memory.tags" :key="tag" class="tag">#{{ tag }}</span>
    </div>
  </article>

  <Teleport to="body">
    <Transition name="fade">
      <div
        v-if="menuOpen"
        ref="dropdownRef"
        class="page-overflow-menu"
        :style="menuStyle"
        @click.stop
      >
        <button class="pmenu-item" @click="onCopy">
          {{ copied ? "Copied!" : "Copy as Markdown" }}
        </button>
        <button class="pmenu-item" @click="onDownload">
          Download .md
        </button>
        <div class="pmenu-sep" />
        <button class="pmenu-item" @click="onArchive">
          {{ memory.archived_at ? "Unarchive" : "Archive" }}
        </button>
        <button
          class="pmenu-item"
          :class="confirmingDelete ? 'pmenu-item--danger-confirm' : 'pmenu-item--danger'"
          @click="onDelete"
        >
          {{ confirmingDelete ? "Confirm delete" : "Delete" }}
        </button>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.memory-page {
  cursor: pointer;
  position: relative;
  background: var(--colour-surface-card);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  border: 1px solid var(--glass-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-card), var(--glass-glow);
  transition: border-color 150ms cubic-bezier(0.4, 0, 0.2, 1),
              box-shadow 150ms cubic-bezier(0.4, 0, 0.2, 1);
}

.memory-page:hover {
  border-color: var(--glass-border-hover);
  box-shadow: var(--shadow-card), var(--glass-glow-strong);
}

.memory-page:active {
  box-shadow: var(--shadow-sm);
}

.memory-page:focus-visible {
  outline: 2px solid var(--colour-border-focus);
  outline-offset: 2px;
  box-shadow: var(--shadow-focus);
}

/* ── List mode ── */
.mode-list {
  padding: var(--space-4) var(--space-5);
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
  font-size: var(--text-xs);
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
  font-size: var(--text-xs);
  -webkit-line-clamp: 2;
  line-height: var(--leading-normal);
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

.mode-grid .page-tags .tag {
  font-size: var(--text-xs);
  display: inline;
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
  font-size: var(--text-base);
  font-weight: var(--font-medium);
  line-height: var(--leading-tight);
  letter-spacing: var(--tracking-tight);
  color: var(--colour-text);
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
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  background: none;
  border: none;
  border-radius: var(--radius-sm);
  color: var(--colour-text-disabled);
  cursor: pointer;
  opacity: 0;
  transition: opacity 150ms, color 150ms, background 150ms;
}

.memory-page:hover .page-menu-btn,
.page-menu-btn.menu-visible {
  opacity: 1;
}

.page-menu-btn:hover,
.page-menu-btn.menu-visible {
  color: var(--colour-text);
  background: var(--colour-surface-overlay);
}

/* Dropdown menu styles are below in unscoped block (Teleported to body) */

.page-meta {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: var(--text-xs);
  color: var(--colour-text-disabled);
}

/* ── Kind pill ── */
.meta-kind-pill {
  display: inline-flex;
  align-items: center;
  padding: 1px 6px;
  border-radius: 99px;
  background: var(--colour-surface-overlay);
  color: var(--colour-text-muted);
  font-size: 10px;
  font-weight: var(--font-semibold);
  text-transform: uppercase;
  letter-spacing: var(--tracking-caps);
  line-height: 1.4;
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
  background: var(--colour-surface-overlay);
}

.imp-5 { background: #f87171; }
.imp-4 { background: #fb923c; }
.imp-3 { background: #fbbf24; }
.imp-2 { background: #a3e635; }
.imp-1 { background: #facc15; }

.meta-sep {
  color: var(--colour-text-disabled);
}

.meta-ns {
  color: var(--colour-text-disabled);
}

.meta-time {
  margin-left: auto;
  font-variant-numeric: tabular-nums;
  color: var(--colour-text-disabled);
}

/* ── Content ── */
.page-content {
  font-size: var(--text-sm);
  line-height: var(--leading-relaxed);
  color: var(--colour-text-secondary);
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

.tag {
  font-size: var(--text-xs);
  color: var(--colour-accent);
}
</style>

<style>
.page-overflow-menu {
  position: fixed;
  transform: translateX(-100%);
  background: var(--colour-surface-dropdown);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-md);
  padding: var(--space-1);
  min-width: 160px;
  box-shadow: var(--shadow-overlay);
  z-index: 9999;
}

.pmenu-item {
  width: 100%;
  padding: var(--space-2) var(--space-3);
  background: none;
  border: none;
  border-radius: var(--radius-sm);
  color: var(--colour-text-secondary);
  font-size: var(--text-sm);
  text-align: left;
  cursor: pointer;
  transition: background 150ms, color 150ms;
}

.pmenu-item:hover {
  background: var(--colour-surface-overlay);
  color: var(--colour-text);
}

.pmenu-sep {
  height: 1px;
  background: var(--colour-border);
  margin: var(--space-1) var(--space-2);
}

.pmenu-item--danger {
  color: var(--colour-text-secondary);
}

.pmenu-item--danger:hover {
  background: color-mix(in srgb, var(--colour-danger) 12%, transparent);
  color: var(--colour-danger);
}

.pmenu-item--danger-confirm {
  color: var(--colour-danger);
  font-weight: var(--font-medium);
}

.pmenu-item--danger-confirm:hover {
  background: color-mix(in srgb, var(--colour-danger) 12%, transparent);
}
</style>

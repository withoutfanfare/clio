<script setup lang="ts">
import { ref, computed, watch, nextTick } from "vue";
import { useMemoryStore } from "@/stores/memories";
import { useAutoSave } from "@/composables/useAutoSave";
import TagInput from "./TagInput.vue";
import KindSelector from "./KindSelector.vue";
import LinkList from "./LinkList.vue";
import * as api from "@/api/memory";
import { copyToClipboard, downloadMarkdown } from "@/utils/memoryExport";

const store = useMemoryStore();

const availableTags = computed(() => {
  const stats = store.currentStats;
  if (!stats?.top_tags?.length) return [];
  return stats.top_tags.map(([tag]: [string, number]) => tag);
});
const { saving, dirty, saved, error: saveError, scheduleAutoSave, cancel } = useAutoSave();

const editContent = ref("");
const editTitle = ref("");
const editKind = ref("note");
const editNamespace = ref("global");
const editTags = ref<string[]>([]);
const editImportance = ref(3);
const metaOpen = ref(false);
const menuOpen = ref(false);
const copied = ref(false);
const confirmingDelete = ref(false);
const contentRef = ref<HTMLTextAreaElement | null>(null);

watch(
  () => store.drawerMemory,
  (memory) => {
    if (memory) {
      editContent.value = memory.content;
      editTitle.value = memory.title ?? "";
      editKind.value = memory.kind;
      editNamespace.value = memory.namespace;
      editTags.value = [...memory.tags];
      editImportance.value = memory.importance;
      metaOpen.value = false;
      menuOpen.value = false;
      cancel();
      nextTick(() => contentRef.value?.focus());
    }
  },
);

function onContentChange() {
  if (!store.drawerMemory) return;
  scheduleAutoSave(store.drawerMemory, { content: editContent.value });
}

function onTitleChange() {
  if (!store.drawerMemory) return;
  scheduleAutoSave(store.drawerMemory, {
    content: editContent.value,
    title: editTitle.value || undefined,
  });
}

function onMetaChange() {
  if (!store.drawerMemory) return;
  scheduleAutoSave(store.drawerMemory, {
    content: editContent.value,
    title: editTitle.value || undefined,
    kind: editKind.value,
    namespace: editNamespace.value,
    tags: editTags.value,
    importance: editImportance.value,
  });
}

async function onCopy() {
  if (!store.drawerMemory) return;
  const ok = await copyToClipboard(store.drawerMemory);
  if (ok) {
    copied.value = true;
    setTimeout(() => (copied.value = false), 1500);
  }
  menuOpen.value = false;
}

function onDownload() {
  if (!store.drawerMemory) return;
  downloadMarkdown(store.drawerMemory);
  menuOpen.value = false;
}

function onTogglePin() {
  if (!store.drawerMemory) return;
  store.togglePin(store.drawerMemory.id);
  menuOpen.value = false;
}

async function archiveMemory() {
  if (!store.drawerMemory) return;
  try {
    if (store.drawerMemory.archived_at) {
      await api.unarchive(store.drawerMemory.id);
    } else {
      await api.archive(store.drawerMemory.id);
    }
    store.closeDrawer();
    store.loadRecent();
  } catch {
    // Archive failed
  }
}

async function deleteMemory() {
  if (!store.drawerMemory) return;
  if (!confirmingDelete.value) {
    confirmingDelete.value = true;
    return;
  }
  try {
    await api.deleteMemory(store.drawerMemory.id);
    store.closeDrawer();
    store.loadRecent();
  } catch {
    // Delete failed
  } finally {
    confirmingDelete.value = false;
  }
}

function close() {
  cancel();
  confirmingDelete.value = false;
  store.closeDrawer();
}

function formatDate(iso: string): string {
  return new Date(iso).toLocaleDateString("en-GB", {
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}
</script>

<template>
  <Teleport to="body">
    <Transition name="fade">
      <div
        v-if="store.drawerOpen"
        class="drawer-backdrop"
        @click="close"
      />
    </Transition>
    <Transition name="slide-right">
      <div v-if="store.drawerOpen && store.drawerMemory" class="drawer">
        <div class="drawer-header">
          <div class="drawer-status">
            <Transition name="status-fade" mode="out-in">
              <span v-if="saveError" key="error" class="status-pill status-error">
                <svg width="12" height="12" viewBox="0 0 16 16" fill="none">
                  <circle cx="8" cy="8" r="7" stroke="currentColor" stroke-width="1.5"/>
                  <path d="M8 5v3.5M8 10.5v.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
                </svg>
                Save failed
              </span>
              <span v-else-if="saving" key="saving" class="status-pill status-saving">
                <svg width="12" height="12" viewBox="0 0 16 16" fill="none" class="spin">
                  <path d="M8 2a6 6 0 105.292 3.143" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
                </svg>
                Saving&hellip;
              </span>
              <span v-else-if="saved" key="saved" class="status-pill status-saved">
                <svg width="12" height="12" viewBox="0 0 16 16" fill="none">
                  <path d="M3.5 8.5l3 3 6-7" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
                </svg>
                Saved
              </span>
              <span v-else-if="dirty" key="dirty" class="status-pill status-dirty">
                <svg width="8" height="8" viewBox="0 0 8 8" fill="none">
                  <circle cx="4" cy="4" r="4" fill="currentColor"/>
                </svg>
                Unsaved changes
              </span>
            </Transition>
          </div>
          <div class="drawer-actions">
            <div class="menu-wrapper">
              <button
                class="drawer-btn"
                @click="menuOpen = !menuOpen; if (!menuOpen) confirmingDelete = false"
                aria-label="More options"
              >
                <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
                  <circle cx="8" cy="3" r="1.5" fill="currentColor"/>
                  <circle cx="8" cy="8" r="1.5" fill="currentColor"/>
                  <circle cx="8" cy="13" r="1.5" fill="currentColor"/>
                </svg>
              </button>
              <Transition name="fade">
                <div v-if="menuOpen" class="overflow-menu">
                  <button class="menu-item" @click="onTogglePin">
                    {{ store.drawerMemory && store.isPinned(store.drawerMemory.id) ? "Unpin" : "Pin to top" }}
                  </button>
                  <button class="menu-item" @click="onCopy">
                    {{ copied ? "Copied!" : "Copy as Markdown" }}
                  </button>
                  <button class="menu-item" @click="onDownload">
                    Download .md
                  </button>
                  <div class="menu-sep" />
                  <button class="menu-item" @click="archiveMemory">
                    {{ store.drawerMemory.archived_at ? "Unarchive" : "Archive" }}
                  </button>
                  <button
                    class="menu-item"
                    :class="confirmingDelete ? 'menu-item--danger-confirm' : 'menu-item--danger'"
                    @click="deleteMemory"
                  >
                    {{ confirmingDelete ? "Confirm delete" : "Delete" }}
                  </button>
                </div>
              </Transition>
            </div>
            <button class="drawer-btn" @click="close" aria-label="Close">
              <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
                <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
              </svg>
            </button>
          </div>
        </div>

        <div class="drawer-body">
          <input
            v-model="editTitle"
            class="drawer-title"
            placeholder="Untitled"
            @input="onTitleChange"
          />

          <textarea
            ref="contentRef"
            v-model="editContent"
            class="drawer-content"
            placeholder="Write something..."
            @input="onContentChange"
          />

          <div class="drawer-meta">
            <button
              class="meta-toggle"
              @click="metaOpen = !metaOpen"
            >
              <svg
                width="10" height="10" viewBox="0 0 12 12" fill="none"
                class="meta-chevron"
                :class="{ open: metaOpen }"
              >
                <path d="M4 2l4 4-4 4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
              </svg>
              Details
            </button>

            <Transition name="fade">
              <div v-if="metaOpen" class="meta-fields">
                <div class="meta-row">
                  <label class="meta-label">Kind</label>
                  <KindSelector
                    v-model="editKind"
                    @update:model-value="onMetaChange"
                  />
                </div>

                <div class="meta-row">
                  <label class="meta-label">Namespace</label>
                  <input
                    v-model="editNamespace"
                    class="meta-input"
                    @input="onMetaChange"
                  />
                </div>

                <div class="meta-row">
                  <label class="meta-label">Tags</label>
                  <TagInput
                    v-model="editTags"
                    :suggestions="availableTags"
                    @update:model-value="onMetaChange"
                  />
                </div>

                <div class="meta-row">
                  <label class="meta-label">Importance</label>
                  <div class="importance-dots">
                    <button
                      v-for="n in 5"
                      :key="n"
                      class="importance-dot"
                      :class="{ active: n <= editImportance }"
                      @click="editImportance = n; onMetaChange()"
                      :aria-label="`Importance ${n}`"
                    />
                  </div>
                </div>

                <div class="meta-info">
                  <span>Created {{ formatDate(store.drawerMemory.created_at) }}</span>
                  <span>Updated {{ formatDate(store.drawerMemory.updated_at) }}</span>
                  <span class="meta-id">{{ store.drawerMemory.id }}</span>
                </div>
              </div>
            </Transition>
          </div>

          <LinkList :memory-id="store.drawerMemory.id" />
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.drawer-backdrop {
  position: fixed;
  inset: 0;
  background: color-mix(in srgb, var(--grey-950) 60%, transparent);
  backdrop-filter: blur(2px);
  z-index: 300;
}

.drawer {
  position: fixed;
  top: 0;
  right: 0;
  bottom: 0;
  width: 60vw;
  max-width: 720px;
  min-width: 400px;
  background: var(--colour-surface-panel);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  border-left: 1px solid var(--colour-border);
  box-shadow: var(--shadow-panel), var(--glass-glow-strong);
  z-index: 301;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.drawer-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-3) var(--space-5);
  border-bottom: 1px solid var(--colour-border);
  min-height: 48px;
}

.drawer-status {
  display: flex;
  align-items: center;
  min-height: 24px;
}

.status-pill {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  padding: 2px 10px 2px 7px;
  border-radius: 99px;
  line-height: 1;
  white-space: nowrap;
}

.status-saving {
  color: var(--colour-text-muted);
  background: var(--colour-surface-overlay);
}

.status-saved {
  color: var(--colour-success);
  background: color-mix(in srgb, var(--colour-success) 12%, transparent);
}

.status-dirty {
  color: var(--colour-warning);
  background: color-mix(in srgb, var(--colour-warning) 12%, transparent);
}

.status-error {
  color: var(--colour-danger);
  background: color-mix(in srgb, var(--colour-danger) 12%, transparent);
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

.spin {
  animation: spin 0.8s linear infinite;
}

.status-fade-enter-active {
  transition: opacity 150ms ease, transform 150ms ease;
}
.status-fade-leave-active {
  transition: opacity 100ms ease;
}
.status-fade-enter-from {
  opacity: 0;
  transform: translateY(-2px);
}
.status-fade-leave-to {
  opacity: 0;
}

.drawer-actions {
  display: flex;
  align-items: center;
  gap: 2px;
}

.drawer-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  background: none;
  border: none;
  border-radius: var(--radius-sm);
  color: var(--colour-text-muted);
  cursor: pointer;
  transition: color 150ms, background 150ms;
}

.drawer-btn:hover {
  color: var(--colour-text);
  background: var(--colour-surface-overlay);
}

.menu-wrapper {
  position: relative;
}

.overflow-menu {
  position: absolute;
  right: 0;
  top: 100%;
  margin-top: var(--space-1);
  background: var(--colour-surface-dropdown);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-md);
  padding: var(--space-1);
  min-width: 180px;
  box-shadow: var(--shadow-overlay);
  z-index: 10;
}

.menu-item {
  width: 100%;
  white-space: nowrap;
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

.menu-item:hover {
  background: var(--colour-surface-overlay);
  color: var(--colour-text);
}

.menu-sep {
  height: 1px;
  background: var(--colour-border);
  margin: var(--space-1) var(--space-2);
}

.menu-item--danger {
  color: var(--colour-text-secondary);
}

.menu-item--danger:hover {
  background: color-mix(in srgb, var(--colour-danger) 12%, transparent);
  color: var(--colour-danger);
}

.menu-item--danger-confirm {
  color: var(--colour-danger);
  font-weight: var(--font-medium);
}

.menu-item--danger-confirm:hover {
  background: color-mix(in srgb, var(--colour-danger) 12%, transparent);
}

.drawer-body {
  flex: 1;
  overflow-y: auto;
  padding: var(--space-8) var(--space-8);
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.drawer-title {
  width: 100%;
  background: none;
  border: none;
  outline: none;
  font-size: var(--text-lg);
  font-weight: var(--font-medium);
  letter-spacing: var(--tracking-tight);
  line-height: var(--leading-tight);
  color: var(--colour-text);
  font-family: inherit;
}

.drawer-title::placeholder {
  color: var(--colour-text-disabled);
}

.drawer-content {
  width: 100%;
  flex: 1;
  min-height: 200px;
  background: none;
  border: none;
  outline: none;
  font-size: var(--text-sm);
  line-height: var(--leading-relaxed);
  color: var(--colour-text);
  resize: none;
  font-family: inherit;
  padding-top: var(--space-2);
}

.drawer-content::placeholder {
  color: var(--colour-text-disabled);
}

.drawer-meta {
  border-top: 1px solid var(--colour-border);
  padding-top: var(--space-3);
  margin-top: var(--space-5);
}

.meta-toggle {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  background: none;
  border: none;
  color: var(--colour-text-muted);
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  text-transform: uppercase;
  letter-spacing: var(--tracking-caps);
  cursor: pointer;
  transition: color 150ms;
  padding: var(--space-1) 0;
}

.meta-chevron {
  transition: transform 200ms cubic-bezier(0.4, 0, 0.2, 1);
}

.meta-chevron.open {
  transform: rotate(90deg);
}

.meta-toggle:hover {
  color: var(--colour-text);
}

.meta-fields {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  margin-top: var(--space-3);
}

.meta-row {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.meta-label {
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  text-transform: uppercase;
  letter-spacing: var(--tracking-caps);
  color: var(--colour-text-muted);
}

.meta-input {
  padding: var(--space-2) var(--space-3);
  background: var(--colour-surface-input);
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-md);
  color: var(--colour-text);
  font-size: var(--text-sm);
  font-family: inherit;
  outline: none;
  transition: border-color 150ms;
}

.meta-input:hover {
  border-color: var(--colour-border-hover);
}

.meta-input:focus {
  border-color: var(--colour-border-focus);
  box-shadow: var(--shadow-focus);
}

.importance-dots {
  display: flex;
  gap: var(--space-2);
}

.importance-dot {
  width: 14px;
  height: 14px;
  border-radius: 9999px;
  border: 2px solid var(--colour-border-hover);
  background: transparent;
  cursor: pointer;
  transition: border-color 150ms, background 150ms;
  padding: 0;
}

.importance-dot.active {
  background: var(--colour-accent);
  border-color: var(--colour-accent);
}

.importance-dot:hover {
  border-color: var(--colour-accent);
}

.meta-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  font-size: var(--text-xs);
  color: var(--colour-text-disabled);
  font-variant-numeric: tabular-nums;
  margin-top: var(--space-2);
}

.meta-id {
  font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace;
  font-size: 10px;
  opacity: 0.7;
}
</style>

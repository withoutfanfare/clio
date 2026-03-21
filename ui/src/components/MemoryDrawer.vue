<script setup lang="ts">
import { ref, computed, watch, nextTick } from "vue";
import { SButton, SFormField, SInput, SBadge, SDropdownMenu } from "@stuntrocket/ui";
import type { SDropdownMenuItem } from "@stuntrocket/ui";
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
const copied = ref(false);
const confirmingDelete = ref(false);
const contentRef = ref<HTMLTextAreaElement | null>(null);
const revisionsOpen = ref(false);

// Revision tracking: store previous content versions in localStorage
interface Revision {
  content: string;
  timestamp: string;
}

const revisions = ref<Revision[]>([]);

function loadRevisions(memoryId: string) {
  try {
    const raw = localStorage.getItem(`clio-revisions-${memoryId}`);
    revisions.value = raw ? JSON.parse(raw) : [];
  } catch {
    revisions.value = [];
  }
}

function saveRevision(memoryId: string, oldContent: string) {
  if (!oldContent.trim()) return;
  const newRevisions = [
    ...revisions.value,
    { content: oldContent, timestamp: new Date().toISOString() },
  ].slice(-20); // Keep last 20 revisions
  revisions.value = newRevisions;
  localStorage.setItem(`clio-revisions-${memoryId}`, JSON.stringify(newRevisions));
}

let previousContent = "";

watch(
  () => store.drawerMemory,
  (memory) => {
    if (memory) {
      previousContent = memory.content;
      editContent.value = memory.content;
      editTitle.value = memory.title ?? "";
      editKind.value = memory.kind;
      editNamespace.value = memory.namespace;
      editTags.value = [...memory.tags];
      editImportance.value = memory.importance;
      metaOpen.value = false;
      revisionsOpen.value = false;
      confirmingDelete.value = false;
      loadRevisions(memory.id);
      cancel();
      nextTick(() => contentRef.value?.focus());
    }
  },
);

function onContentChange() {
  if (!store.drawerMemory) return;
  // Save revision if content has meaningfully changed
  if (previousContent && previousContent !== editContent.value && previousContent.trim() !== editContent.value.trim()) {
    saveRevision(store.drawerMemory.id, previousContent);
    previousContent = editContent.value;
  }
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
}

function onDownload() {
  if (!store.drawerMemory) return;
  downloadMarkdown(store.drawerMemory);
}

function onTogglePin() {
  if (!store.drawerMemory) return;
  store.togglePin(store.drawerMemory.id);
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

const menuItems = computed<SDropdownMenuItem[]>(() => {
  if (!store.drawerMemory) return [];
  return [
    {
      label: store.isPinned(store.drawerMemory.id) ? "Unpin" : "Pin to top",
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
      label: store.drawerMemory.archived_at ? "Unarchive" : "Archive",
      value: "archive",
    },
    {
      label: confirmingDelete.value ? "Confirm delete" : "Delete",
      value: "delete",
      danger: true,
    },
  ];
});

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
      archiveMemory();
      break;
    case "delete":
      deleteMemory();
      break;
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
              <SBadge v-if="saveError" key="error" variant="error">Save failed</SBadge>
              <SBadge v-else-if="saving" key="saving" variant="default">Saving&hellip;</SBadge>
              <SBadge v-else-if="saved" key="saved" variant="success">Saved</SBadge>
              <SBadge v-else-if="dirty" key="dirty" variant="warning">Unsaved changes</SBadge>
            </Transition>
          </div>
          <div class="drawer-actions">
            <SDropdownMenu
              :items="menuItems"
              align="right"
              @select="handleMenuSelect"
            >
              <template #trigger="{ toggle }">
                <SButton variant="icon" size="sm" @click="toggle" aria-label="More options">
                  <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
                    <circle cx="8" cy="3" r="1.5" fill="currentColor"/>
                    <circle cx="8" cy="8" r="1.5" fill="currentColor"/>
                    <circle cx="8" cy="13" r="1.5" fill="currentColor"/>
                  </svg>
                </SButton>
              </template>
            </SDropdownMenu>
            <SButton variant="icon" size="sm" @click="close" aria-label="Close">
              <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
                <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
              </svg>
            </SButton>
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
                <SFormField label="Kind">
                  <KindSelector
                    v-model="editKind"
                    @update:model-value="onMetaChange"
                  />
                </SFormField>

                <SFormField label="Namespace">
                  <SInput
                    v-model="editNamespace"
                    @update:model-value="onMetaChange"
                  />
                </SFormField>

                <SFormField label="Tags">
                  <TagInput
                    v-model="editTags"
                    :suggestions="availableTags"
                    @update:model-value="onMetaChange"
                  />
                </SFormField>

                <SFormField label="Importance">
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
                </SFormField>

                <div class="meta-row" v-if="store.drawerMemory.source">
                  <label class="meta-label">Source</label>
                  <span class="meta-source-value">{{ store.drawerMemory.source }}</span>
                </div>

                <div class="meta-info">
                  <span>Created {{ formatDate(store.drawerMemory.created_at) }}</span>
                  <span>Updated {{ formatDate(store.drawerMemory.updated_at) }}</span>
                  <span class="meta-id">{{ store.drawerMemory.id }}</span>
                </div>

                <!-- Revision history -->
                <div v-if="revisions.length" class="revision-section">
                  <button class="revision-toggle" @click="revisionsOpen = !revisionsOpen">
                    <svg
                      width="10" height="10" viewBox="0 0 12 12" fill="none"
                      class="revision-chevron"
                      :class="{ open: revisionsOpen }"
                    >
                      <path d="M4 2l4 4-4 4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
                    </svg>
                    Revisions ({{ revisions.length }})
                  </button>
                  <Transition name="fade">
                    <div v-if="revisionsOpen" class="revision-list">
                      <div
                        v-for="(rev, i) in revisions"
                        :key="i"
                        class="revision-item"
                      >
                        <span class="revision-time">{{ formatDate(rev.timestamp) }}</span>
                        <p class="revision-content">{{ rev.content.slice(0, 200) }}{{ rev.content.length > 200 ? "\u2026" : "" }}</p>
                      </div>
                    </div>
                  </Transition>
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
  background: rgba(0, 0, 0, 0.6);
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
  border-left: 1px solid var(--color-border-subtle);
  box-shadow: var(--shadow-sheet), var(--glass-glow-strong);
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
  border-bottom: 1px solid var(--color-border-subtle);
  min-height: 48px;
}

.drawer-status {
  display: flex;
  align-items: center;
  min-height: 24px;
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
  font-size: 17px;
  font-weight: 500;
  letter-spacing: -0.01em;
  line-height: 1.3;
  color: var(--color-text-primary);
  font-family: inherit;
}

.drawer-title::placeholder {
  color: var(--color-text-tertiary);
}

.drawer-content {
  width: 100%;
  flex: 1;
  min-height: 200px;
  background: none;
  border: none;
  outline: none;
  font-size: 13px;
  line-height: 1.65;
  color: var(--color-text-primary);
  resize: none;
  font-family: inherit;
  padding-top: var(--space-2);
}

.drawer-content::placeholder {
  color: var(--color-text-tertiary);
}

.drawer-meta {
  border-top: 1px solid var(--color-border-subtle);
  padding-top: var(--space-3);
  margin-top: var(--space-5);
}

.meta-toggle {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  background: none;
  border: none;
  color: var(--color-text-tertiary);
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
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
  color: var(--color-text-primary);
}

.meta-fields {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  margin-top: var(--space-3);
}

.importance-dots {
  display: flex;
  gap: var(--space-2);
}

.importance-dot {
  width: 14px;
  height: 14px;
  border-radius: 9999px;
  border: 2px solid var(--color-border-strong);
  background: transparent;
  cursor: pointer;
  transition: border-color 150ms, background 150ms;
  padding: 0;
}

.importance-dot.active {
  background: var(--color-accent);
  border-color: var(--color-accent);
}

.importance-dot:hover {
  border-color: var(--color-accent);
}

.meta-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  font-size: 11px;
  color: var(--color-text-tertiary);
  font-variant-numeric: tabular-nums;
  margin-top: var(--space-2);
}

.meta-id {
  font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace;
  font-size: 10px;
  opacity: 0.7;
}

.meta-row {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.meta-label {
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--color-text-tertiary);
}

.meta-source-value {
  font-size: 13px;
  color: var(--color-text-secondary);
  padding: var(--space-1) 0;
  text-transform: capitalize;
}

/* ── Revision History ── */
.revision-section {
  margin-top: var(--space-3);
  padding-top: var(--space-3);
  border-top: 1px solid var(--colour-border);
}

.revision-toggle {
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

.revision-toggle:hover {
  color: var(--colour-text);
}

.revision-chevron {
  transition: transform 200ms cubic-bezier(0.4, 0, 0.2, 1);
}

.revision-chevron.open {
  transform: rotate(90deg);
}

.revision-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  margin-top: var(--space-2);
  max-height: 300px;
  overflow-y: auto;
}

.revision-item {
  padding: var(--space-2) var(--space-3);
  background: var(--colour-surface-overlay);
  border-radius: var(--radius-md);
}

.revision-time {
  font-size: var(--text-xs);
  color: var(--colour-text-disabled);
  font-variant-numeric: tabular-nums;
}

.revision-content {
  font-size: var(--text-xs);
  color: var(--colour-text-secondary);
  margin: var(--space-1) 0 0;
  line-height: var(--leading-relaxed);
  white-space: pre-wrap;
  word-break: break-word;
}
</style>

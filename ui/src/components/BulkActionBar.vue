<script setup lang="ts">
import { ref } from "vue";
import { useMemoryStore } from "@/stores/memories";
import * as api from "@/api/memory";

const store = useMemoryStore();
const tagInput = ref("");
const tagMode = ref<"add" | "remove" | null>(null);
const confirmingDelete = ref(false);
const processing = ref(false);

async function bulkArchive() {
  if (processing.value) return;
  processing.value = true;
  try {
    const ids = Array.from(store.selectedIds);
    await api.bulkArchive(ids);
    store.clearSelection();
    store.invalidateSearchCache();
    await store.loadRecent();
  } finally {
    processing.value = false;
  }
}

async function bulkDelete() {
  if (processing.value) return;
  if (!confirmingDelete.value) {
    confirmingDelete.value = true;
    return;
  }
  processing.value = true;
  try {
    const ids = Array.from(store.selectedIds);
    await api.bulkDelete(ids);
    store.clearSelection();
    store.invalidateSearchCache();
    await store.loadRecent();
  } finally {
    processing.value = false;
    confirmingDelete.value = false;
  }
}

function showTagInput(mode: "add" | "remove") {
  tagMode.value = mode;
  tagInput.value = "";
}

async function applyTag() {
  if (!tagInput.value.trim() || processing.value) return;
  processing.value = true;
  try {
    const ids = Array.from(store.selectedIds);
    if (tagMode.value === "add") {
      await api.bulkAddTag(ids, tagInput.value.trim());
    } else {
      await api.bulkRemoveTag(ids, tagInput.value.trim());
    }
    tagMode.value = null;
    tagInput.value = "";
    store.invalidateSearchCache();
    await store.loadRecent();
  } finally {
    processing.value = false;
  }
}
</script>

<template>
  <Transition name="slide-up">
    <div v-if="store.selectedCount >= 2" class="bulk-bar">
      <div class="bulk-info">
        <span class="bulk-count">{{ store.selectedCount }} selected</span>
        <button class="bulk-clear" @click="store.clearSelection()">Clear</button>
      </div>

      <div v-if="tagMode" class="bulk-tag-input">
        <input
          v-model="tagInput"
          class="tag-field"
          :placeholder="tagMode === 'add' ? 'Tag to add...' : 'Tag to remove...'"
          @keydown.enter.prevent="applyTag"
          @keydown.escape="tagMode = null"
          autofocus
        />
        <button class="bulk-action" @click="applyTag" :disabled="!tagInput.trim()">
          {{ tagMode === "add" ? "Add" : "Remove" }}
        </button>
        <button class="bulk-cancel" @click="tagMode = null">Cancel</button>
      </div>

      <div v-else class="bulk-actions">
        <button class="bulk-action" @click="bulkArchive" :disabled="processing">
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
            <rect x="2" y="2" width="12" height="4" rx="1" stroke="currentColor" stroke-width="1.3"/>
            <path d="M3 6v7a1 1 0 001 1h8a1 1 0 001-1V6" stroke="currentColor" stroke-width="1.3"/>
            <path d="M6.5 9h3" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
          </svg>
          Archive
        </button>
        <button class="bulk-action" @click="showTagInput('add')" :disabled="processing">
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
            <path d="M8 3v10M3 8h10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
          Add tag
        </button>
        <button class="bulk-action" @click="showTagInput('remove')" :disabled="processing">
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
            <path d="M3 8h10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
          Remove tag
        </button>
        <button
          class="bulk-action bulk-danger"
          :class="{ confirming: confirmingDelete }"
          @click="bulkDelete"
          :disabled="processing"
        >
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
            <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
          {{ confirmingDelete ? "Confirm delete" : "Delete" }}
        </button>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.bulk-bar {
  position: fixed;
  bottom: var(--space-6);
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  align-items: center;
  gap: var(--space-4);
  padding: var(--space-3) var(--space-5);
  background: var(--colour-surface-dropdown);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-xl);
  box-shadow: var(--shadow-overlay);
  z-index: 200;
}

.bulk-info {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.bulk-count {
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  color: var(--colour-accent);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.bulk-clear {
  padding: 2px 8px;
  background: none;
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-sm);
  color: var(--colour-text-muted);
  font-size: var(--text-xs);
  cursor: pointer;
  transition: color 150ms, border-color 150ms;
}

.bulk-clear:hover {
  color: var(--colour-text);
  border-color: var(--colour-border-hover);
}

.bulk-actions,
.bulk-tag-input {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.bulk-action {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  padding: var(--space-2) var(--space-3);
  background: none;
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-md);
  color: var(--colour-text-secondary);
  font-size: var(--text-sm);
  cursor: pointer;
  transition: all 150ms;
  white-space: nowrap;
}

.bulk-action:hover:not(:disabled) {
  color: var(--colour-text);
  border-color: var(--colour-border-hover);
  background: var(--colour-surface-overlay);
}

.bulk-action:disabled {
  opacity: 0.5;
  cursor: default;
}

.bulk-danger:hover:not(:disabled) {
  color: var(--colour-danger);
  border-color: color-mix(in srgb, var(--colour-danger) 40%, transparent);
  background: color-mix(in srgb, var(--colour-danger) 8%, transparent);
}

.bulk-danger.confirming {
  color: var(--colour-danger);
  font-weight: var(--font-medium);
  border-color: var(--colour-danger);
}

.bulk-cancel {
  padding: var(--space-2) var(--space-3);
  background: none;
  border: none;
  color: var(--colour-text-muted);
  font-size: var(--text-sm);
  cursor: pointer;
}

.tag-field {
  padding: var(--space-2) var(--space-3);
  background: var(--colour-surface-input);
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-md);
  color: var(--colour-text);
  font-size: var(--text-sm);
  font-family: inherit;
  outline: none;
  width: 160px;
}

.tag-field:focus {
  border-color: var(--colour-border-focus);
}

/* Slide up transition */
.slide-up-enter-active {
  transition: all 200ms ease-out;
}
.slide-up-leave-active {
  transition: all 150ms ease-in;
}
.slide-up-enter-from,
.slide-up-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(20px);
}
</style>

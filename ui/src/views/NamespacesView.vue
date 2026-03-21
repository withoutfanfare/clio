<script setup lang="ts">
import { ref, onMounted } from "vue";
import * as api from "@/api/memory";
import { useMemoryStore } from "@/stores/memories";
import type { NamespaceInfo } from "@/api/types";

const store = useMemoryStore();
const namespaces = ref<NamespaceInfo[]>([]);
const loading = ref(false);
const error = ref<string | null>(null);
const success = ref<string | null>(null);

// Create
const showCreate = ref(false);
const newName = ref("");
const creating = ref(false);

// Rename
const renamingNs = ref<string | null>(null);
const renameValue = ref("");

// Merge
const mergingNs = ref<string | null>(null);
const mergeTarget = ref("");

// Delete
const confirmingDelete = ref<string | null>(null);

async function loadNamespaces() {
  loading.value = true;
  error.value = null;
  try {
    namespaces.value = await api.namespaceDetails();
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }
}

async function createNamespace() {
  if (!newName.value.trim() || creating.value) return;
  creating.value = true;
  error.value = null;
  try {
    // Create a namespace by creating a memory in it, then immediately deleting
    // Actually, we just remember something and the namespace is created.
    // Better approach: remember a placeholder and then archive it.
    // Simplest: just create a memory with content "Namespace created"
    await api.remember({
      content: `Namespace '${newName.value.trim()}' initialised`,
      namespace: newName.value.trim(),
      kind: "note",
      source: "desktop",
      tags: ["system"],
    });
    success.value = `Namespace '${newName.value.trim()}' created`;
    newName.value = "";
    showCreate.value = false;
    store.invalidateSearchCache();
    await loadNamespaces();
    await store.fetchNamespaces();
  } catch (e) {
    error.value = String(e);
  } finally {
    creating.value = false;
  }
}

function startRename(ns: string) {
  renamingNs.value = ns;
  renameValue.value = ns;
}

async function applyRename() {
  if (!renamingNs.value || !renameValue.value.trim()) return;
  error.value = null;
  try {
    await api.renameNamespace(renamingNs.value, renameValue.value.trim());
    success.value = `Renamed '${renamingNs.value}' to '${renameValue.value.trim()}'`;
    renamingNs.value = null;
    store.invalidateSearchCache();
    await loadNamespaces();
    await store.fetchNamespaces();
  } catch (e) {
    error.value = String(e);
  }
}

function startMerge(ns: string) {
  mergingNs.value = ns;
  mergeTarget.value = "";
}

async function applyMerge() {
  if (!mergingNs.value || !mergeTarget.value.trim()) return;
  error.value = null;
  try {
    const count = await api.mergeNamespaces(mergingNs.value, mergeTarget.value.trim());
    success.value = `Merged ${count} memories from '${mergingNs.value}' into '${mergeTarget.value.trim()}'`;
    mergingNs.value = null;
    store.invalidateSearchCache();
    await loadNamespaces();
    await store.fetchNamespaces();
  } catch (e) {
    error.value = String(e);
  }
}

async function deleteNs(ns: string, memoryCount: number) {
  if (confirmingDelete.value !== ns) {
    confirmingDelete.value = ns;
    return;
  }
  error.value = null;
  try {
    if (memoryCount > 0) {
      const count = await api.purgeNamespace(ns);
      success.value = `Deleted namespace '${ns}' and ${count} ${count === 1 ? "memory" : "memories"}`;
    } else {
      await api.deleteNamespace(ns);
      success.value = `Namespace '${ns}' deleted`;
    }
    confirmingDelete.value = null;
    store.invalidateSearchCache();
    await loadNamespaces();
    await store.fetchNamespaces();
  } catch (e) {
    error.value = String(e);
    confirmingDelete.value = null;
  }
}

function formatDate(iso: string | null): string {
  if (!iso) return "—";
  return new Date(iso).toLocaleDateString("en-GB", {
    day: "numeric",
    month: "short",
    year: "numeric",
  });
}

function clearMessages() {
  error.value = null;
  success.value = null;
}

onMounted(() => {
  loadNamespaces();
});
</script>

<template>
  <div class="ns-view">
    <div class="ns-header">
      <h1 class="ns-title">Namespaces</h1>
      <button class="ns-create-btn" @click="showCreate = !showCreate; clearMessages()">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <path d="M8 3v10M3 8h10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
        </svg>
        {{ showCreate ? "Cancel" : "Create" }}
      </button>
    </div>

    <Transition name="fade">
      <div v-if="showCreate" class="create-form">
        <input
          v-model="newName"
          class="ns-input"
          placeholder="Namespace name..."
          @keydown.enter="createNamespace"
        />
        <button
          class="ns-action-btn primary"
          @click="createNamespace"
          :disabled="!newName.trim() || creating"
        >
          {{ creating ? "Creating\u2026" : "Create" }}
        </button>
      </div>
    </Transition>

    <div v-if="success" class="ns-success" @click="success = null">{{ success }}</div>
    <div v-if="error" class="ns-error" @click="error = null">{{ error }}</div>

    <div v-if="loading" class="ns-loading">Loading namespaces...</div>

    <div v-else class="ns-list">
      <div
        v-for="ns in namespaces"
        :key="ns.name"
        class="ns-card"
      >
        <div class="ns-card-header">
          <template v-if="renamingNs === ns.name">
            <input
              v-model="renameValue"
              class="ns-input ns-rename-input"
              @keydown.enter="applyRename"
              @keydown.escape="renamingNs = null"
              autofocus
            />
            <button class="ns-action-btn primary" @click="applyRename">Save</button>
            <button class="ns-action-btn" @click="renamingNs = null">Cancel</button>
          </template>
          <template v-else-if="mergingNs === ns.name">
            <span class="ns-name">Merge '{{ ns.name }}' into:</span>
            <select v-model="mergeTarget" class="ns-input ns-merge-select">
              <option value="">Select target...</option>
              <option
                v-for="other in namespaces.filter((n) => n.name !== ns.name)"
                :key="other.name"
                :value="other.name"
              >
                {{ other.name }} ({{ other.memory_count }})
              </option>
            </select>
            <button class="ns-action-btn primary" @click="applyMerge" :disabled="!mergeTarget">Merge</button>
            <button class="ns-action-btn" @click="mergingNs = null">Cancel</button>
          </template>
          <template v-else>
            <div class="ns-info">
              <span class="ns-name">{{ ns.name }}</span>
              <span class="ns-meta">
                {{ ns.memory_count }} {{ ns.memory_count === 1 ? "memory" : "memories" }}
                &middot; Last active {{ formatDate(ns.last_activity) }}
              </span>
            </div>
            <div class="ns-actions">
              <button class="ns-action-btn" @click="startRename(ns.name)" title="Rename">
                <svg width="12" height="12" viewBox="0 0 16 16" fill="none">
                  <path d="M11.5 1.5l3 3L5 14H2v-3l9.5-9.5z" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round"/>
                </svg>
              </button>
              <button class="ns-action-btn" @click="startMerge(ns.name)" title="Merge">
                <svg width="12" height="12" viewBox="0 0 16 16" fill="none">
                  <path d="M8 2v12M2 8l6-6M14 8l-6-6" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>
                </svg>
              </button>
              <button
                class="ns-action-btn"
                :class="{ danger: confirmingDelete === ns.name }"
                @click="deleteNs(ns.name, ns.memory_count)"
                :title="confirmingDelete === ns.name
                  ? `Click again to delete ${ns.memory_count} ${ns.memory_count === 1 ? 'memory' : 'memories'}`
                  : 'Delete namespace'"
              >
                <template v-if="confirmingDelete === ns.name && ns.memory_count > 0">
                  Delete {{ ns.memory_count }}?
                </template>
                <svg v-else width="12" height="12" viewBox="0 0 16 16" fill="none">
                  <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
                </svg>
              </button>
            </div>
          </template>
        </div>
      </div>

      <div v-if="!namespaces.length && !loading" class="ns-empty">
        No namespaces found. Create one to get started.
      </div>
    </div>
  </div>
</template>

<style scoped>
.ns-view {
  padding-bottom: var(--space-12);
}

.ns-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: var(--space-6);
}

.ns-title {
  font-size: var(--text-xl);
  font-weight: var(--font-semibold);
  letter-spacing: var(--tracking-tight);
  color: var(--colour-text);
}

.ns-create-btn {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  padding: var(--space-2) var(--space-3);
  background: var(--colour-accent);
  border: none;
  border-radius: var(--radius-md);
  color: white;
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  cursor: pointer;
  transition: background 150ms;
}

.ns-create-btn:hover {
  background: var(--colour-accent-hover);
}

.create-form {
  display: flex;
  gap: var(--space-2);
  margin-bottom: var(--space-4);
}

.ns-input {
  flex: 1;
  padding: var(--space-2) var(--space-3);
  background: var(--colour-surface-input);
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-md);
  color: var(--colour-text);
  font-size: var(--text-sm);
  font-family: inherit;
  outline: none;
}

.ns-input:focus {
  border-color: var(--colour-border-focus);
  box-shadow: var(--shadow-focus);
}

.ns-input::placeholder {
  color: var(--colour-text-disabled);
}

.ns-success {
  padding: var(--space-2) var(--space-3);
  background: color-mix(in srgb, var(--colour-success) 10%, transparent);
  border: 1px solid color-mix(in srgb, var(--colour-success) 20%, transparent);
  border-radius: var(--radius-md);
  color: var(--colour-success);
  font-size: var(--text-sm);
  margin-bottom: var(--space-4);
  cursor: pointer;
}

.ns-error {
  padding: var(--space-2) var(--space-3);
  background: color-mix(in srgb, var(--colour-danger) 10%, transparent);
  border: 1px solid color-mix(in srgb, var(--colour-danger) 20%, transparent);
  border-radius: var(--radius-md);
  color: var(--colour-danger);
  font-size: var(--text-sm);
  margin-bottom: var(--space-4);
  cursor: pointer;
}

.ns-loading {
  padding: var(--space-8);
  text-align: center;
  color: var(--colour-text-muted);
  font-size: var(--text-sm);
}

.ns-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.ns-card {
  padding: var(--space-4);
  background: var(--colour-surface-card);
  border: 1px solid var(--glass-border);
  border-radius: var(--radius-lg);
  transition: border-color 150ms;
}

.ns-card:hover {
  border-color: var(--glass-border-hover);
}

.ns-card-header {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.ns-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.ns-name {
  font-size: var(--text-base);
  font-weight: var(--font-medium);
  color: var(--colour-text);
}

.ns-meta {
  font-size: var(--text-xs);
  color: var(--colour-text-muted);
  font-variant-numeric: tabular-nums;
}

.ns-actions {
  display: flex;
  gap: var(--space-1);
}

.ns-action-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-1);
  padding: var(--space-1) var(--space-2);
  background: none;
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-sm);
  color: var(--colour-text-muted);
  font-size: var(--text-sm);
  cursor: pointer;
  transition: all 150ms;
  white-space: nowrap;
}

.ns-action-btn:hover:not(:disabled) {
  color: var(--colour-text);
  border-color: var(--colour-border-hover);
}

.ns-action-btn:disabled {
  opacity: 0.3;
  cursor: not-allowed;
}

.ns-action-btn.primary {
  background: var(--colour-accent);
  border-color: var(--colour-accent);
  color: white;
}

.ns-action-btn.primary:hover {
  background: var(--colour-accent-hover);
}

.ns-action-btn.danger {
  color: var(--colour-danger);
  border-color: var(--colour-danger);
}

.ns-rename-input {
  max-width: 200px;
}

.ns-merge-select {
  appearance: none;
  max-width: 200px;
  cursor: pointer;
}

.ns-empty {
  padding: var(--space-8);
  text-align: center;
  color: var(--colour-text-disabled);
  font-size: var(--text-sm);
}
</style>

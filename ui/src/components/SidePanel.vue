<script setup lang="ts">
import { ref, computed } from "vue";
import { useMemoryStore } from "@/stores/memories";
import { useRouter } from "vue-router";
import { open } from "@tauri-apps/plugin-dialog";
import * as api from "@/api/memory";

const store = useMemoryStore();
const router = useRouter();

const showNewProject = ref(false);
const newProjectDir = ref("");
const newProjectName = ref("");
const projectError = ref<string | null>(null);
const projectCreating = ref(false);

const memoryCount = computed(() => store.total);

function selectNamespace(ns: string | null) {
  store.setNamespace(ns);
  store.loadRecent();
  router.push({ name: "home" });
}

function goToStats() {
  router.push({ name: "stats" });
}

function toggleNewProject() {
  showNewProject.value = !showNewProject.value;
  projectError.value = null;
  if (!showNewProject.value) {
    newProjectDir.value = "";
    newProjectName.value = "";
  }
}

async function pickFolder() {
  const selected = await open({
    directory: true,
    multiple: false,
    title: "Choose project folder",
  });
  if (selected) {
    newProjectDir.value = selected as string;
  }
}

async function createProject() {
  const dir = newProjectDir.value.trim();
  const name = newProjectName.value.trim();

  if (!dir || !name) {
    projectError.value = "Both fields are required.";
    return;
  }

  projectCreating.value = true;
  projectError.value = null;

  try {
    await api.initNamespace(dir, name);
    newProjectDir.value = "";
    newProjectName.value = "";
    showNewProject.value = false;
    await store.fetchNamespaces();
    selectNamespace(name);
  } catch (e) {
    projectError.value = String(e);
  } finally {
    projectCreating.value = false;
  }
}
</script>

<template>
  <aside class="side-panel">
    <!-- Brand / Memory count -->
    <div class="panel-brand">
      <div class="brand-icon">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none">
          <circle cx="12" cy="12" r="3" fill="currentColor" opacity="0.8"/>
          <circle cx="12" cy="12" r="8" stroke="currentColor" stroke-width="1.5" opacity="0.4"/>
          <circle cx="12" cy="12" r="11" stroke="currentColor" stroke-width="0.75" opacity="0.2"/>
        </svg>
      </div>
      <div class="brand-text">
        <span class="brand-name">Clio</span>
        <span class="brand-count">{{ memoryCount }} memories</span>
      </div>
    </div>

    <!-- Pinned count -->
    <div v-if="store.pinnedCount > 0" class="pinned-badge">
      <svg width="12" height="12" viewBox="0 0 16 16" fill="none">
        <path d="M9.828 1.172a1 1 0 011.414 0l3.586 3.586a1 1 0 010 1.414L12 9l-1 4-4.5-1.5L3 15l.5-3.5L2 7l3-2.828 4.828-3z" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" fill="none"/>
      </svg>
      <span>{{ store.pinnedCount }} pinned</span>
    </div>

    <!-- Section label -->
    <div class="section-label">Namespaces</div>

    <nav class="panel-nav">
      <button
        class="ns-item"
        :class="{ active: store.selectedNamespace === null }"
        @click="selectNamespace(null)"
      >
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <path d="M2 4.5h12M2 8h12M2 11.5h12" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
        </svg>
        <span>All memories</span>
      </button>
      <button
        v-for="ns in store.allNamespaces"
        :key="ns"
        class="ns-item"
        :class="{ active: store.selectedNamespace === ns }"
        @click="selectNamespace(ns)"
      >
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <path d="M2 4.5A1.5 1.5 0 013.5 3h3.379a1.5 1.5 0 011.06.44l.622.62a1.5 1.5 0 001.06.44H12.5A1.5 1.5 0 0114 6v5.5a1.5 1.5 0 01-1.5 1.5h-9A1.5 1.5 0 012 11.5v-7z" stroke="currentColor" stroke-width="1.1"/>
        </svg>
        <span>{{ ns }}</span>
      </button>
    </nav>

    <div class="panel-actions">
      <button class="action-btn" @click="toggleNewProject">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <path d="M8 3v10M3 8h10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
        </svg>
        {{ showNewProject ? "Cancel" : "New project" }}
      </button>
    </div>

    <Transition name="expand">
      <div v-if="showNewProject" class="new-project-form">
        <label class="form-label">Project name</label>
        <input
          v-model="newProjectName"
          class="form-input"
          type="text"
          placeholder="e.g. my-app"
          @keydown.enter="createProject"
        />

        <label class="form-label">Base folder</label>
        <button class="browse-btn" @click="pickFolder">
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
            <path d="M2 4.5A1.5 1.5 0 013.5 3h3.379a1.5 1.5 0 011.06.44l.622.62a1.5 1.5 0 001.06.44H12.5A1.5 1.5 0 0114 6v5.5a1.5 1.5 0 01-1.5 1.5h-9A1.5 1.5 0 012 11.5v-7z" stroke="currentColor" stroke-width="1.2"/>
          </svg>
          {{ newProjectDir ? "" : "Choose folder\u2026" }}
        </button>
        <span v-if="newProjectDir" class="folder-path">{{ newProjectDir }}</span>

        <p v-if="projectError" class="form-error">{{ projectError }}</p>

        <button
          class="create-btn"
          :disabled="projectCreating"
          @click="createProject"
        >
          {{ projectCreating ? "Creating\u2026" : "Create project" }}
        </button>
      </div>
    </Transition>

    <div class="panel-footer">
      <button class="footer-btn" @click="goToStats">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <rect x="2" y="9" width="3" height="5" rx="0.5" stroke="currentColor" stroke-width="1.2"/>
          <rect x="6.5" y="5" width="3" height="9" rx="0.5" stroke="currentColor" stroke-width="1.2"/>
          <rect x="11" y="2" width="3" height="12" rx="0.5" stroke="currentColor" stroke-width="1.2"/>
        </svg>
        Statistics
      </button>
      <button class="footer-btn" @click="router.push({ name: 'namespaces' })">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <path d="M2 4.5A1.5 1.5 0 013.5 3h3.379a1.5 1.5 0 011.06.44l.622.62a1.5 1.5 0 001.06.44H12.5A1.5 1.5 0 0114 6v5.5a1.5 1.5 0 01-1.5 1.5h-9A1.5 1.5 0 012 11.5v-7z" stroke="currentColor" stroke-width="1.1"/>
        </svg>
        Manage namespaces
      </button>
      <button class="footer-btn" @click="router.push({ name: 'tools' })">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <path d="M6 2L4.5 5.5 2 6l2 2-.5 3.5L6 10l2.5 1.5L9 8l2-2-2.5-.5L6 2z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/>
          <circle cx="12" cy="12" r="2" stroke="currentColor" stroke-width="1.2"/>
        </svg>
        Tools
      </button>
      <button class="footer-btn" @click="store.toggleCompose()">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <path d="M8 3v10M3 8h10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
        </svg>
        New memory
        <kbd class="footer-kbd">N</kbd>
      </button>
    </div>
  </aside>
</template>

<style scoped>
.side-panel {
  width: 220px;
  min-width: 220px;
  background: var(--colour-surface-panel);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-xl);
  box-shadow: var(--shadow-panel), var(--glass-glow-strong);
  display: flex;
  flex-direction: column;
  padding: var(--space-4) var(--space-3);
  overflow-y: auto;
  z-index: 10;
}

/* ── Brand ── */
.panel-brand {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-2) var(--space-2);
  margin-bottom: var(--space-4);
}

.brand-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border-radius: var(--radius-md);
  background: var(--colour-accent-muted);
  color: var(--colour-accent);
  flex-shrink: 0;
}

.brand-text {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
}

.brand-name {
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--colour-text);
  line-height: 1;
}

.brand-count {
  font-size: 10px;
  color: var(--colour-text-disabled);
  font-variant-numeric: tabular-nums;
  line-height: 1;
}

/* ── Pinned Badge ── */
.pinned-badge {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px var(--space-2);
  margin-bottom: var(--space-2);
  font-size: 10px;
  color: var(--colour-accent);
  font-variant-numeric: tabular-nums;
}

/* ── Section Label ── */
.section-label {
  font-size: 10px;
  font-weight: var(--font-semibold);
  text-transform: uppercase;
  letter-spacing: var(--tracking-caps);
  color: var(--colour-text-disabled);
  padding: 0 var(--space-2);
  margin-bottom: var(--space-2);
}

/* ── Namespace List ── */
.panel-nav {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 1px;
  overflow-y: auto;
  min-height: 0;
}

.ns-item {
  width: 100%;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 6px var(--space-2);
  background: transparent;
  border: none;
  border-radius: var(--radius-md);
  color: var(--colour-text-muted);
  cursor: pointer;
  font-size: var(--text-sm);
  text-align: left;
  transition: color 150ms, background 150ms;
}

.ns-item svg {
  flex-shrink: 0;
  opacity: 0.5;
  transition: opacity 150ms;
}

.ns-item span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ns-item:hover {
  color: var(--colour-text);
  background: var(--colour-surface-overlay);
}

.ns-item:hover svg {
  opacity: 0.8;
}

.ns-item.active {
  color: var(--colour-accent);
  background: var(--colour-accent-muted);
  font-weight: var(--font-medium);
}

.ns-item.active svg {
  opacity: 1;
}

/* ── Actions ── */
.panel-actions {
  padding: var(--space-2) 0;
}

.action-btn {
  width: 100%;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 6px var(--space-2);
  background: transparent;
  border: none;
  border-radius: var(--radius-md);
  color: var(--colour-text-muted);
  cursor: pointer;
  font-size: var(--text-sm);
  text-align: left;
  transition: color 150ms, background 150ms;
}

.action-btn:hover {
  color: var(--colour-accent);
  background: var(--colour-accent-muted);
}

/* ── New Project Form ── */
.new-project-form {
  padding: var(--space-3) var(--space-2);
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.form-label {
  font-size: 10px;
  font-weight: var(--font-semibold);
  text-transform: uppercase;
  letter-spacing: var(--tracking-caps);
  color: var(--colour-text-disabled);
}

.form-input {
  width: 100%;
  padding: 6px var(--space-2);
  background: var(--colour-surface-input);
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-md);
  color: var(--colour-text);
  font-size: var(--text-sm);
  font-family: inherit;
  outline: none;
  transition: border-color 150ms;
}

.form-input::placeholder {
  color: var(--colour-text-disabled);
}

.form-input:focus {
  border-color: var(--colour-accent);
  box-shadow: var(--shadow-focus);
}

.browse-btn {
  width: 100%;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 6px var(--space-2);
  background: var(--colour-surface-input);
  border: 1px dashed var(--colour-border);
  border-radius: var(--radius-md);
  color: var(--colour-text-muted);
  font-size: var(--text-sm);
  font-family: inherit;
  cursor: pointer;
  transition: border-color 150ms, color 150ms;
}

.browse-btn:hover {
  border-color: var(--colour-accent);
  color: var(--colour-accent);
}

.folder-path {
  display: block;
  font-size: 10px;
  color: var(--colour-text-secondary);
  word-break: break-all;
  line-height: var(--leading-normal);
  padding: 0 var(--space-2);
}

.form-error {
  font-size: 10px;
  color: var(--colour-danger);
  margin: 0;
  padding: 0 var(--space-2);
}

.create-btn {
  padding: 6px var(--space-3);
  background: var(--colour-accent);
  border: none;
  border-radius: var(--radius-md);
  color: #fff;
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  cursor: pointer;
  transition: opacity 150ms;
}

.create-btn:hover {
  opacity: 0.9;
}

.create-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* ── Expand Transition ── */
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
  max-height: 300px;
}

/* ── Footer ── */
.panel-footer {
  margin-top: auto;
  padding-top: var(--space-3);
  border-top: 1px solid var(--colour-border);
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.footer-btn {
  width: 100%;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 6px var(--space-2);
  background: transparent;
  border: none;
  border-radius: var(--radius-md);
  color: var(--colour-text-muted);
  cursor: pointer;
  font-size: var(--text-sm);
  text-align: left;
  transition: color 150ms, background 150ms;
}

.footer-btn:hover {
  color: var(--colour-text);
  background: var(--colour-surface-overlay);
}

.footer-kbd {
  margin-left: auto;
  font-size: 10px;
  font-family: inherit;
  font-weight: var(--font-medium);
  padding: 1px 5px;
  border-radius: 4px;
  border: 1px solid var(--colour-border);
  color: var(--colour-text-disabled);
  line-height: 1;
}
</style>

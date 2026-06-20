<script setup lang="ts">
import { ref, computed, nextTick } from "vue";
import { SButton, SFormField, SInput, SSidebarLink, SKbd } from "@stuntrocket/ui";
import { useMemoryStore } from "@/stores/memories";
import { useNamespaceColours } from "@/composables/useNamespaceColours";
import { useRouter } from "vue-router";
import { open } from "@tauri-apps/plugin-dialog";
import * as api from "@/api/memory";

const store = useMemoryStore();
const { getColour } = useNamespaceColours();
const router = useRouter();

// Context menu state
const ctxMenu = ref<{ x: number; y: number; ns: string } | null>(null);
const ctxConfirming = ref(false);

function onContextMenu(e: MouseEvent, ns: string) {
  e.preventDefault();
  ctxConfirming.value = false;
  ctxMenu.value = { x: e.clientX, y: e.clientY, ns };
  nextTick(() => document.addEventListener("click", closeCtxMenu, { once: true }));
}

function closeCtxMenu() {
  ctxMenu.value = null;
  ctxConfirming.value = false;
}

async function ctxDelete() {
  if (!ctxMenu.value) return;
  if (!ctxConfirming.value) {
    ctxConfirming.value = true;
    return;
  }
  const ns = ctxMenu.value.ns;
  try {
    await api.purgeNamespace(ns);
    ctxMenu.value = null;
    ctxConfirming.value = false;
    if (store.selectedNamespace === ns) {
      selectNamespace(null);
    }
    await store.fetchNamespaces();
    store.loadRecent();
    store.pushToast(`Deleted project "${ns}"`, "info");
  } catch {
    ctxMenu.value = null;
    ctxConfirming.value = false;
    store.pushToast(`Couldn't delete project "${ns}"`, "error");
  }
}

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
    <!-- Section label -->
    <div class="section-label">Namespaces</div>

    <nav class="panel-nav">
      <SSidebarLink
        :active="store.selectedNamespace === null"
        @click="selectNamespace(null)"
      >
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <path d="M2 4.5h12M2 8h12M2 11.5h12" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
        </svg>
        <span>All memories</span>
      </SSidebarLink>
      <SSidebarLink
        v-for="ns in store.allNamespaces"
        :key="ns"
        :active="store.selectedNamespace === ns"
        @click="selectNamespace(ns)"
        @contextmenu="onContextMenu($event, ns)"
      >
        <span class="ns-dot" :style="{ background: getColour(ns) }" />
        <span>{{ ns }}</span>
      </SSidebarLink>
    </nav>

    <!-- Right-click context menu -->
    <Teleport to="body">
      <div
        v-if="ctxMenu"
        class="ctx-menu"
        :style="{ left: ctxMenu.x + 'px', top: ctxMenu.y + 'px' }"
        @click.stop
      >
        <button
          class="ctx-item danger"
          @click="ctxDelete"
        >
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
            <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
          {{ ctxConfirming ? `Delete "${ctxMenu.ns}" and all memories?` : `Delete project` }}
        </button>
      </div>
    </Teleport>

    <div class="panel-actions">
      <SButton variant="ghost" size="sm" @click="toggleNewProject" class="action-btn-full">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <path d="M8 3v10M3 8h10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
        </svg>
        {{ showNewProject ? "Cancel" : "New project" }}
      </SButton>
    </div>

    <Transition name="expand">
      <div v-if="showNewProject" class="new-project-form">
        <SFormField label="Project name">
          <SInput
            v-model="newProjectName"
            type="text"
            placeholder="e.g. my-app"
            @keydown.enter="createProject"
          />
        </SFormField>

        <SFormField label="Base folder">
          <SButton variant="secondary" size="sm" @click="pickFolder" class="browse-btn">
            <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
              <path d="M2 4.5A1.5 1.5 0 013.5 3h3.379a1.5 1.5 0 011.06.44l.622.62a1.5 1.5 0 001.06.44H12.5A1.5 1.5 0 0114 6v5.5a1.5 1.5 0 01-1.5 1.5h-9A1.5 1.5 0 012 11.5v-7z" stroke="currentColor" stroke-width="1.2"/>
            </svg>
            {{ newProjectDir ? "" : "Choose folder\u2026" }}
          </SButton>
        </SFormField>
        <span v-if="newProjectDir" class="folder-path">{{ newProjectDir }}</span>

        <p v-if="projectError" class="form-error">{{ projectError }}</p>

        <SButton
          variant="primary"
          size="sm"
          :disabled="projectCreating"
          :loading="projectCreating"
          @click="createProject"
        >
          {{ projectCreating ? "Creating\u2026" : "Create project" }}
        </SButton>
      </div>
    </Transition>

    <div class="panel-footer">
      <SSidebarLink @click="goToStats">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <rect x="2" y="9" width="3" height="5" rx="0.5" stroke="currentColor" stroke-width="1.2"/>
          <rect x="6.5" y="5" width="3" height="9" rx="0.5" stroke="currentColor" stroke-width="1.2"/>
          <rect x="11" y="2" width="3" height="12" rx="0.5" stroke="currentColor" stroke-width="1.2"/>
        </svg>
        Statistics
      </SSidebarLink>
      <SSidebarLink @click="router.push({ name: 'namespaces' })">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <path d="M2 4.5A1.5 1.5 0 013.5 3h3.379a1.5 1.5 0 011.06.44l.622.62a1.5 1.5 0 001.06.44H12.5A1.5 1.5 0 0114 6v5.5a1.5 1.5 0 01-1.5 1.5h-9A1.5 1.5 0 012 11.5v-7z" stroke="currentColor" stroke-width="1.1"/>
        </svg>
        Manage namespaces
      </SSidebarLink>
      <SSidebarLink @click="router.push({ name: 'context-builder' })">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <rect x="2" y="2" width="12" height="12" rx="1.5" stroke="currentColor" stroke-width="1.1"/>
          <path d="M5 5h6M5 8h4M5 11h5" stroke="currentColor" stroke-width="1.1" stroke-linecap="round"/>
        </svg>
        Context builder
      </SSidebarLink>
      <SSidebarLink @click="router.push({ name: 'tools' })">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <path d="M6 2L4.5 5.5 2 6l2 2-.5 3.5L6 10l2.5 1.5L9 8l2-2-2.5-.5L6 2z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/>
          <circle cx="12" cy="12" r="2" stroke="currentColor" stroke-width="1.2"/>
        </svg>
        Tools
      </SSidebarLink>
      <SSidebarLink @click="store.toggleCompose()">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <path d="M8 3v10M3 8h10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
        </svg>
        New memory
        <SKbd>N</SKbd>
      </SSidebarLink>
    </div>
  </aside>
</template>

<style scoped>
.side-panel {
  width: 220px;
  min-width: 220px;
  background: rgba(18, 16, 22, 0.82);
  backdrop-filter: blur(24px) saturate(1.5);
  -webkit-backdrop-filter: blur(24px) saturate(1.5);
  border: 1px solid rgba(255, 255, 255, 0.10);
  border-radius: var(--radius-lg);
  box-shadow:
    0 4px 24px rgba(0, 0, 0, 0.5),
    inset 0 1px 0 0 rgba(255, 255, 255, 0.08),
    inset 0 0 20px rgba(139, 92, 246, 0.03);
  display: flex;
  flex-direction: column;
  padding: var(--space-2) var(--space-3) var(--space-3);
  overflow: hidden;
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
  background: var(--color-accent-subtle);
  color: var(--color-accent);
  flex-shrink: 0;
}

.brand-text {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
}

.brand-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--color-text-primary);
  line-height: 1;
}

.brand-count {
  font-size: 10px;
  color: var(--color-text-tertiary);
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
  color: var(--color-accent);
  font-variant-numeric: tabular-nums;
}

/* ── Namespace Colour Dot ── */
.ns-dot {
  display: inline-block;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}

/* ── Section Label ── */
.section-label {
  font-size: 10px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--color-text-tertiary);
  padding: 0 var(--space-2);
  margin-bottom: var(--space-2);
}

/* ── Namespace List ── */
.panel-nav {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
  overflow-y: auto;
  min-height: 0;
}

/* Sidebar link overrides — fix icon shrinking, add gap, improve readability */
.panel-nav :deep(button),
.panel-footer :deep(button) {
  gap: var(--space-2);
  padding-top: 10px;
  padding-bottom: 10px;
  padding-left: var(--space-2) !important;
  font-size: 13px;
  line-height: 1.65;
  overflow: hidden;
  text-overflow: ellipsis;
}

.panel-nav :deep(button) svg,
.panel-footer :deep(button) svg {
  flex-shrink: 0;
}

.panel-nav :deep(button) span,
.panel-footer :deep(button) span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  min-width: 0;
}

/* ── Actions ── */
.panel-actions {
  padding: var(--space-2) 0;
}

.action-btn-full {
  width: 100%;
  justify-content: flex-start;
}

/* ── New Project Form ── */
.new-project-form {
  padding: var(--space-3) var(--space-2);
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.browse-btn {
  width: 100%;
  justify-content: flex-start;
}

.folder-path {
  display: block;
  font-size: 10px;
  color: var(--color-text-secondary);
  word-break: break-all;
  line-height: 1.5;
  padding: 0 var(--space-2);
}

.form-error {
  font-size: 10px;
  color: var(--color-danger);
  margin: 0;
  padding: 0 var(--space-2);
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
  border-top: 1px solid var(--color-border-subtle);
  display: flex;
  flex-direction: column;
  gap: 2px;
}

/* ── Context Menu ── */
.ctx-menu {
  position: fixed;
  z-index: 500;
  min-width: 180px;
  background: var(--colour-surface-dropdown);
  border: 1px solid rgba(255, 255, 255, 0.10);
  border-radius: var(--radius-md);
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
  padding: 4px;
}

.ctx-item {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  width: 100%;
  padding: 8px 12px;
  background: transparent;
  border: none;
  border-radius: var(--radius-sm);
  color: var(--color-text-secondary);
  font-size: 13px;
  cursor: pointer;
  text-align: left;
  white-space: nowrap;
}

.ctx-item:hover {
  background: var(--color-surface-hover);
  color: var(--color-text-primary);
}

.ctx-item.danger {
  color: var(--color-danger);
}

.ctx-item.danger:hover {
  background: var(--color-danger-subtle);
}
</style>

<script setup lang="ts">
import { ref, computed, nextTick } from "vue";
import { SButton, SFormField, SInput, SSidebarLink, SKbd } from "@stuntrocket/ui";
import { useMemoryStore } from "@/stores/memories";
import { useNamespaceColours } from "@/composables/useNamespaceColours";
import { useSidebarResize } from "@/composables/useSidebarResize";
import { useRouter } from "vue-router";
import { open } from "@tauri-apps/plugin-dialog";
import * as api from "@/api/memory";
import { MAX_SIDEBAR_WIDTH, MIN_SIDEBAR_WIDTH } from "@/utils/sidebarWidth";

const store = useMemoryStore();
const { getColour } = useNamespaceColours();
const router = useRouter();
const {
  width: sidebarWidth,
  isResizing,
  startResize,
  resize,
  stopResize,
  stopResizeAfterCaptureLoss,
  resizeWithKeyboard,
} = useSidebarResize();

// Workspace shortcuts stay on this Mac and retain exact namespace identities.
const workspaceQuery = ref("");
function readWorkspaceList(key: string): string[] {
  try {
    const value = JSON.parse(localStorage.getItem(key) || "[]");
    return Array.isArray(value) ? value.filter((entry): entry is string => typeof entry === "string").slice(0, 8) : [];
  } catch { return []; }
}
const pinnedWorkspaces = ref(readWorkspaceList("clio-workspace-pins"));
const recentWorkspaces = ref(readWorkspaceList("clio-workspace-recent"));
const workspaceSections = computed(() => {
  const all = store.allNamespaces;
  const query = workspaceQuery.value.trim().toLocaleLowerCase();
  if (query) return [{ title: "Matching workspaces", names: all.filter(ns => ns.toLocaleLowerCase().includes(query)) }];
  const pinned = pinnedWorkspaces.value.filter(ns => all.includes(ns));
  const recent = recentWorkspaces.value.filter(ns => all.includes(ns) && !pinned.includes(ns));
  return [
    { title: "Pinned workspaces", names: pinned },
    { title: "Recent workspaces", names: recent },
    { title: "All workspaces", names: all.filter(ns => !pinned.includes(ns) && !recent.includes(ns)) },
  ].filter(section => section.names.length);
});
function saveWorkspaceList(key: string, value: string[]) {
  try { localStorage.setItem(key, JSON.stringify(value)); }
  catch { store.pushToast("Couldn't save workspace shortcuts on this Mac", "error"); }
}
function toggleWorkspacePin(ns: string) {
  // This action comes from a workspace in the successfully loaded list.
  pinnedWorkspaces.value = pinnedWorkspaces.value.filter(item => store.allNamespaces.includes(item));
  if (pinnedWorkspaces.value.includes(ns)) pinnedWorkspaces.value = pinnedWorkspaces.value.filter(item => item !== ns);
  else if (pinnedWorkspaces.value.length < 8) pinnedWorkspaces.value.push(ns);
  else { store.pushToast("You can pin up to eight workspaces", "info"); return; }
  saveWorkspaceList("clio-workspace-pins", pinnedWorkspaces.value);
  closeCtxMenu();
}

// Workspace deletion state
const ctxMenu = ref<{
  x: number;
  y: number;
  ns: string;
  memoryCount: number | null;
} | null>(null);
const ctxConfirming = ref(false);
const ctxDeleting = ref(false);
let ctxMenuRequest = 0;

async function openDeleteMenu(e: MouseEvent, ns: string) {
  e.preventDefault();
  e.stopPropagation();
  ctxConfirming.value = false;
  ctxDeleting.value = false;
  const target = e.currentTarget as HTMLElement;
  const bounds = target.getBoundingClientRect();
  const openedFromButton = e.type === "click";
  ctxMenu.value = {
    x: openedFromButton ? bounds.right + 6 : e.clientX,
    y: openedFromButton ? bounds.top : e.clientY,
    ns,
    memoryCount: null,
  };
  const request = ++ctxMenuRequest;
  nextTick(() => document.addEventListener("click", closeCtxMenu, { once: true }));

  try {
    const details = await api.namespaceDetails();
    const current = ctxMenu.value;
    if (request === ctxMenuRequest && current?.ns === ns) {
      current.memoryCount = details.find((item) => item.name === ns)?.memory_count ?? 0;
    }
  } catch {
    if (request === ctxMenuRequest && ctxMenu.value?.ns === ns) {
      store.pushToast(`Workspace deletion is unavailable: could not check "${ns}"`, "error");
    }
  }
}

function closeCtxMenu() {
  ctxMenu.value = null;
  ctxConfirming.value = false;
  ctxDeleting.value = false;
}

async function ctxDelete() {
  if (!ctxMenu.value || ctxMenu.value.memoryCount === null || ctxDeleting.value) return;
  if (ctxMenu.value.ns === "global") return;
  if (!ctxConfirming.value) {
    ctxConfirming.value = true;
    return;
  }
  const ns = ctxMenu.value.ns;
  ctxDeleting.value = true;
  try {
    const count = await api.purgeNamespace(ns);
    pinnedWorkspaces.value = pinnedWorkspaces.value.filter(item => item !== ns);
    recentWorkspaces.value = recentWorkspaces.value.filter(item => item !== ns);
    saveWorkspaceList("clio-workspace-pins", pinnedWorkspaces.value);
    saveWorkspaceList("clio-workspace-recent", recentWorkspaces.value);
    ctxMenu.value = null;
    ctxConfirming.value = false;
    if (store.selectedNamespace === ns) {
      selectNamespace(null);
    } else {
      await store.loadRecent();
    }
    await store.fetchNamespaces();
    store.pushToast(
      `Deleted workspace "${ns}" and ${count} ${count === 1 ? "memory" : "memories"}. Backup saved.`,
      "info",
    );
  } catch {
    ctxMenu.value = null;
    ctxConfirming.value = false;
    store.pushToast(`Couldn't delete workspace "${ns}"`, "error");
  } finally {
    ctxDeleting.value = false;
  }
}

const showNewProject = ref(false);
const newProjectDir = ref("");
const newProjectName = ref("");
const projectError = ref<string | null>(null);
const projectCreating = ref(false);

const memoryCount = computed(() => store.total);

function selectNamespace(ns: string | null) {
  if (ns) {
    recentWorkspaces.value = [ns, ...recentWorkspaces.value.filter(item => item !== ns)].slice(0, 5);
    saveWorkspaceList("clio-workspace-recent", recentWorkspaces.value);
  }
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
  <aside
    class="side-panel"
    :class="{ 'is-resizing': isResizing }"
    :style="{ width: `${sidebarWidth}px`, minWidth: `${sidebarWidth}px` }"
  >
    <!-- Section label -->
    <div class="section-label">Workspaces</div>

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
      <input v-model="workspaceQuery" class="workspace-search" type="search" aria-label="Find a workspace" placeholder="Find a workspace…" />
      <p v-if="workspaceQuery && !workspaceSections[0]?.names.length" class="section-label">No matching workspaces</p>
      <template v-for="section in workspaceSections" :key="section.title">
        <p class="section-label workspace-section">{{ section.title }}</p>
      <div
        v-for="ns in section.names"
        :key="ns"
        class="workspace-row is-deletable"
      >
        <SSidebarLink
          class="workspace-link"
          :active="store.selectedNamespace === ns"
          @click="selectNamespace(ns)"
          @contextmenu="openDeleteMenu($event, ns)"
        >
          <span class="ns-dot" :style="{ background: getColour(ns) }" />
          <span class="workspace-name" :title="ns">{{ ns }}</span>
        </SSidebarLink>
        <button
          class="workspace-delete"
          type="button"
          :aria-label="`Manage workspace ${ns}`"
          :title="`Manage workspace ${ns}`"
          @click="openDeleteMenu($event, ns)"
        >
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
            <circle cx="3" cy="8" r="1" fill="currentColor"/><circle cx="8" cy="8" r="1" fill="currentColor"/><circle cx="13" cy="8" r="1" fill="currentColor"/>
          </svg>
        </button>
      </div>
      </template>
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
          class="ctx-item"
          @click="toggleWorkspacePin(ctxMenu.ns)"
        >{{ pinnedWorkspaces.includes(ctxMenu.ns) ? "Unpin workspace" : "Pin workspace" }}</button>
        <button
          v-if="ctxMenu.ns !== 'global'"
          class="ctx-item danger"
          :disabled="ctxMenu.memoryCount === null || ctxDeleting"
          @click="ctxDelete"
        >
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
            <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
          <template v-if="ctxDeleting">Deleting…</template>
          <template v-else-if="ctxMenu.memoryCount === null">Checking memories…</template>
          <template v-else-if="ctxConfirming">
            {{ ctxMenu.memoryCount === 0
              ? "Permanently delete workspace?"
              : `Permanently delete ${ctxMenu.memoryCount} ${ctxMenu.memoryCount === 1 ? "memory" : "memories"}?` }}
          </template>
          <template v-else>Delete workspace</template>
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
      <SSidebarLink @click="router.push({ name: 'attention' })">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <circle cx="8" cy="8" r="6" stroke="currentColor" stroke-width="1.2"/>
          <path d="M8 4.5V8l2.5 1.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
        </svg>
        Needs attention
      </SSidebarLink>
      <SSidebarLink @click="router.push({ name: 'inbox' })">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <path d="M2 3h12v10H2V3zM2 9h3l1 2h4l1-2h3" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/>
        </svg>
        Review inbox
      </SSidebarLink>
      <SSidebarLink @click="goToStats">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <rect x="2" y="9" width="3" height="5" rx="0.5" stroke="currentColor" stroke-width="1.2"/>
          <rect x="6.5" y="5" width="3" height="9" rx="0.5" stroke="currentColor" stroke-width="1.2"/>
          <rect x="11" y="2" width="3" height="12" rx="0.5" stroke="currentColor" stroke-width="1.2"/>
        </svg>
        Statistics
      </SSidebarLink>
      <SSidebarLink v-if="!store.isRemote" @click="router.push({ name: 'namespaces' })">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <path d="M2 4.5A1.5 1.5 0 013.5 3h3.379a1.5 1.5 0 011.06.44l.622.62a1.5 1.5 0 001.06.44H12.5A1.5 1.5 0 0114 6v5.5a1.5 1.5 0 01-1.5 1.5h-9A1.5 1.5 0 012 11.5v-7z" stroke="currentColor" stroke-width="1.1"/>
        </svg>
        Manage workspaces
      </SSidebarLink>
      <SSidebarLink @click="router.push({ name: 'context-builder' })">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <rect x="2" y="2" width="12" height="12" rx="1.5" stroke="currentColor" stroke-width="1.1"/>
          <path d="M5 5h6M5 8h4M5 11h5" stroke="currentColor" stroke-width="1.1" stroke-linecap="round"/>
        </svg>
        Context builder
      </SSidebarLink>
      <SSidebarLink v-if="!store.isRemote" @click="router.push({ name: 'tools' })">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <path d="M6 2L4.5 5.5 2 6l2 2-.5 3.5L6 10l2.5 1.5L9 8l2-2-2.5-.5L6 2z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/>
          <circle cx="12" cy="12" r="2" stroke="currentColor" stroke-width="1.2"/>
        </svg>
        Tools
      </SSidebarLink>
      <SSidebarLink @click="router.push({ name: 'settings' })">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <circle cx="8" cy="8" r="2.2" stroke="currentColor" stroke-width="1.2"/>
          <path d="M8 1.8v1.3M8 12.9v1.3M14.2 8h-1.3M3.1 8H1.8M12.4 3.6l-.9.9M4.5 11.5l-.9.9M12.4 12.4l-.9-.9M4.5 4.5l-.9-.9" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
        </svg>
        Settings
      </SSidebarLink>
      <SSidebarLink @click="store.toggleCompose()">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <path d="M8 3v10M3 8h10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
        </svg>
        New memory
        <SKbd>N</SKbd>
      </SSidebarLink>
    </div>

    <div
      class="resize-handle"
      role="separator"
      aria-label="Resize sidebar"
      aria-orientation="vertical"
      :aria-valuemin="MIN_SIDEBAR_WIDTH"
      :aria-valuemax="MAX_SIDEBAR_WIDTH"
      :aria-valuenow="sidebarWidth"
      tabindex="0"
      @pointerdown="startResize"
      @pointermove="resize"
      @pointerup="stopResize"
      @pointercancel="stopResize"
      @lostpointercapture="stopResizeAfterCaptureLoss"
      @keydown="resizeWithKeyboard"
    />
  </aside>
</template>

<style scoped>
.workspace-search { width: 100%; min-height: 36px; margin: 8px 0; padding: 6px 10px; border: 1px solid var(--color-border-default); border-radius: 6px; background: var(--colour-surface-input); color: var(--color-text-primary); font: inherit; font-size: 13px; }
.workspace-section { margin-top: 14px; }
.side-panel {
  width: 220px;
  min-width: 220px;
  flex: 0 0 auto;
  position: relative;
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

.resize-handle {
  position: absolute;
  inset: 0 0 0 auto;
  width: 12px;
  border: 0;
  cursor: col-resize;
  touch-action: none;
}

.resize-handle::after {
  content: "";
  position: absolute;
  top: var(--space-3);
  right: 0;
  bottom: var(--space-3);
  width: 1px;
  border-radius: 1px;
  background: transparent;
  transition: background-color 120ms ease, box-shadow 120ms ease;
}

.resize-handle:hover::after,
.resize-handle:focus-visible::after,
.side-panel.is-resizing .resize-handle::after {
  background: var(--color-accent);
  box-shadow: -2px 0 8px rgba(139, 92, 246, 0.22);
}

.resize-handle:focus-visible {
  outline: 2px solid var(--color-accent);
  outline-offset: -3px;
}

.side-panel.is-resizing {
  user-select: none;
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
  font-size: 12px;
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

.workspace-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  align-items: center;
  min-width: 0;
}

.workspace-row.is-deletable {
  grid-template-columns: minmax(0, 1fr) 36px;
}

.workspace-link {
  min-width: 0;
}

.workspace-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.workspace-delete {
  width: 36px;
  height: 36px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--color-text-tertiary);
  cursor: pointer;
}

.workspace-delete:hover {
  color: var(--color-danger);
  background: var(--color-danger-subtle);
}

.workspace-delete:focus-visible {
  outline: 2px solid var(--color-accent);
  outline-offset: 1px;
  color: var(--color-danger);
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

.ctx-item:disabled {
  cursor: wait;
  opacity: 0.65;
}

.ctx-item.danger:hover {
  background: var(--color-danger-subtle);
}
</style>

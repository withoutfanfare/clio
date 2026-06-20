<script setup lang="ts">
import { useMemoryStore } from "@/stores/memories";

const store = useMemoryStore();

function sourceLabel(source: string | null): string {
  switch (source) {
    case "cli": return "CLI";
    case "mcp": return "MCP";
    case "hook": return "Hook";
    case "import": return "Import";
    case "desktop": return "Desktop";
    default: return "External";
  }
}

function openMemory(id: string) {
  store.openDrawer(id);
  store.dismissNotification(id);
}
</script>

<template>
  <Teleport to="body">
    <div class="toast-container">
    <TransitionGroup name="toast" tag="div" class="toast-group">
      <div
        v-for="t in store.toasts"
        :key="t.id"
        class="toast action-toast"
        :class="`is-${t.variant}`"
        :role="t.variant === 'error' ? 'alert' : 'status'"
        :aria-live="t.variant === 'error' ? 'assertive' : 'polite'"
      >
        <div class="toast-body">
          <span class="toast-title">{{ t.message }}</span>
        </div>
        <button
          v-if="t.action"
          class="toast-action"
          @click="store.runToastAction(t.id)"
        >
          {{ t.action.label }}
        </button>
        <button
          class="toast-dismiss"
          @click="store.dismissToast(t.id)"
          aria-label="Dismiss"
        >
          <svg width="12" height="12" viewBox="0 0 16 16" fill="none">
            <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
        </button>
      </div>
    </TransitionGroup>
    <TransitionGroup name="toast" tag="div" class="toast-group">
      <div
        v-for="notif in store.notifications"
        :key="notif.id"
        class="toast"
        @click="openMemory(notif.id)"
      >
        <div class="toast-icon">
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
            <circle cx="8" cy="8" r="3" fill="currentColor" opacity="0.8"/>
            <circle cx="8" cy="8" r="6" stroke="currentColor" stroke-width="1" opacity="0.4"/>
          </svg>
        </div>
        <div class="toast-body">
          <span class="toast-title">{{ notif.title || "New memory" }}</span>
          <span class="toast-meta">
            {{ notif.namespace }}
            <span class="toast-source">via {{ sourceLabel(notif.source) }}</span>
          </span>
        </div>
        <button
          class="toast-dismiss"
          @click.stop="store.dismissNotification(notif.id)"
          aria-label="Dismiss"
        >
          <svg width="12" height="12" viewBox="0 0 16 16" fill="none">
            <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
        </button>
      </div>
    </TransitionGroup>
    </div>
  </Teleport>
</template>

<style scoped>
.toast-container {
  position: fixed;
  top: var(--space-4);
  right: var(--space-4);
  z-index: 250;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  pointer-events: none;
}

/* Lets both transition groups share the container's column + gap. */
.toast-group {
  display: contents;
}

/* ── Action toasts (feedback / undo) ── */
.action-toast {
  cursor: default;
  border-left: 3px solid var(--colour-text-disabled);
}

.action-toast.is-success {
  border-left-color: var(--colour-accent);
}

.action-toast.is-error {
  border-left-color: var(--colour-danger);
}

.toast-action {
  flex-shrink: 0;
  padding: var(--space-1) var(--space-3);
  background: none;
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-sm);
  color: var(--colour-accent);
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  cursor: pointer;
  transition: background 150ms, border-color 150ms;
}

.toast-action:hover {
  background: var(--colour-accent-muted);
  border-color: var(--colour-accent);
}

.toast {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
  background: var(--colour-surface-dropdown);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-overlay);
  cursor: pointer;
  pointer-events: auto;
  min-width: 280px;
  max-width: 380px;
  transition: all 150ms;
}

.toast:hover {
  border-color: var(--colour-border-hover);
}

.toast-icon {
  color: var(--colour-accent);
  flex-shrink: 0;
}

.toast-body {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.toast-title {
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  color: var(--colour-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.toast-meta {
  font-size: var(--text-xs);
  color: var(--colour-text-muted);
}

.toast-source {
  color: var(--colour-text-disabled);
}

.toast-dismiss {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  background: none;
  border: none;
  border-radius: var(--radius-sm);
  color: var(--colour-text-disabled);
  cursor: pointer;
  flex-shrink: 0;
  transition: color 150ms, background 150ms;
}

.toast-dismiss:hover {
  color: var(--colour-text);
  background: var(--colour-surface-overlay);
}

.toast-enter-active {
  transition: all 200ms ease-out;
}
.toast-leave-active {
  transition: all 150ms ease-in;
}
.toast-enter-from {
  opacity: 0;
  transform: translateX(40px);
}
.toast-leave-to {
  opacity: 0;
  transform: translateX(40px);
}
</style>

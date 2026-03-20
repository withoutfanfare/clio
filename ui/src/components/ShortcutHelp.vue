<script setup lang="ts">
import { useMemoryStore } from "@/stores/memories";

const store = useMemoryStore();

const isMac = navigator.platform.toUpperCase().includes("MAC");
const mod = isMac ? "\u2318" : "Ctrl";

const shortcuts = [
  { keys: [`${mod}N`], description: "New memory" },
  { keys: [`${mod}K`], description: "Search / command palette" },
  { keys: [`${mod}D`], description: "Archive focused memory" },
  { keys: ["j"], description: "Navigate down" },
  { keys: ["k"], description: "Navigate up" },
  { keys: ["Enter"], description: "Open focused memory" },
  { keys: ["Esc"], description: "Close panel / deselect" },
  { keys: [`${mod}/`], description: "Toggle this help" },
];

function close() {
  store.shortcutHelpOpen = false;
}
</script>

<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="store.shortcutHelpOpen" class="help-backdrop" @click="close" />
    </Transition>
    <Transition name="scale">
      <div v-if="store.shortcutHelpOpen" class="help-modal">
        <div class="help-header">
          <h2 class="help-title">Keyboard shortcuts</h2>
          <button class="help-close" @click="close" aria-label="Close">
            <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
              <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
            </svg>
          </button>
        </div>
        <div class="help-list">
          <div v-for="s in shortcuts" :key="s.description" class="help-row">
            <span class="help-desc">{{ s.description }}</span>
            <span class="help-keys">
              <kbd v-for="key in s.keys" :key="key">{{ key }}</kbd>
            </span>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.help-backdrop {
  position: fixed;
  inset: 0;
  background: color-mix(in srgb, var(--grey-950) 60%, transparent);
  backdrop-filter: blur(2px);
  z-index: 400;
}

.help-modal {
  position: fixed;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  width: 380px;
  max-width: 90vw;
  background: var(--colour-surface-dropdown);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-overlay);
  z-index: 401;
  padding: var(--space-5);
}

.help-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: var(--space-4);
}

.help-title {
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--colour-text);
}

.help-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  background: none;
  border: none;
  border-radius: var(--radius-sm);
  color: var(--colour-text-muted);
  cursor: pointer;
  transition: color 150ms, background 150ms;
}

.help-close:hover {
  color: var(--colour-text);
  background: var(--colour-surface-overlay);
}

.help-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.help-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-2) 0;
  border-bottom: 1px solid var(--colour-border);
}

.help-row:last-child {
  border-bottom: none;
}

.help-desc {
  font-size: var(--text-sm);
  color: var(--colour-text-secondary);
}

.help-keys {
  display: flex;
  gap: 4px;
}

.help-keys kbd {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 24px;
  padding: 2px 6px;
  font-size: var(--text-xs);
  font-family: inherit;
  font-weight: var(--font-medium);
  color: var(--colour-text-muted);
  background: var(--colour-surface-overlay);
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-sm);
  line-height: 1.4;
}
</style>

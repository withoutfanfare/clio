<script setup lang="ts">
import { SModal, SKbd } from "@stuntrocket/ui";
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
  <SModal
    :open="store.shortcutHelpOpen"
    max-width="380px"
    @close="close"
  >
    <template #header>
      <h2 class="help-title">Keyboard shortcuts</h2>
    </template>

    <div class="help-list">
      <div v-for="s in shortcuts" :key="s.description" class="help-row">
        <span class="help-desc">{{ s.description }}</span>
        <span class="help-keys">
          <SKbd v-for="key in s.keys" :key="key">{{ key }}</SKbd>
        </span>
      </div>
    </div>
  </SModal>
</template>

<style scoped>
.help-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--color-text-primary);
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
  border-bottom: 1px solid var(--color-border-subtle);
}

.help-row:last-child {
  border-bottom: none;
}

.help-desc {
  font-size: 13px;
  color: var(--color-text-secondary);
}

.help-keys {
  display: flex;
  gap: 4px;
}
</style>

<script setup lang="ts">
import { useMemoryStore } from "@/stores/memories";

const store = useMemoryStore();
</script>

<template>
  <header class="appbar">
    <div class="appbar-inner">
      <div class="appbar-left" />

      <div class="appbar-right">
        <button
          class="appbar-btn"
          @click="store.paletteOpen = true"
          title="Search (Cmd+K)"
        >
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
            <circle cx="7" cy="7" r="4.5" stroke="currentColor" stroke-width="1.5"/>
            <path d="M10.5 10.5L14 14" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
          <kbd class="kbd-hint">K</kbd>
        </button>
        <button
          class="appbar-btn"
          @click="store.toggleCompose()"
          title="New memory (Cmd+N)"
        >
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
            <path d="M8 3v10M3 8h10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
        </button>
      </div>
    </div>
  </header>
</template>

<style scoped>
.appbar {
  position: sticky;
  top: 0;
  height: var(--appbar-height);
  background: transparent;
  z-index: 100;
  flex-shrink: 0;
}

.appbar-inner {
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  padding: 0 var(--space-4);
}

.appbar-left {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.appbar-right {
  display: flex;
  align-items: center;
  gap: var(--space-1);
}

/* ── Icon Buttons ── */
.appbar-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
  width: 32px;
  height: 32px;
  padding: 0;
  background: none;
  border: none;
  border-radius: var(--radius-md);
  color: var(--colour-text-secondary);
  cursor: pointer;
  transition: color 150ms, background 150ms;
}

.appbar-btn:hover {
  color: var(--colour-text);
  background: var(--colour-surface-overlay);
}

/* Widen the search button to fit the kbd hint */
.appbar-btn:has(.kbd-hint) {
  width: auto;
  padding: 0 var(--space-2);
}

/* ── Keyboard Hint ── */
.kbd-hint {
  font-size: var(--text-xs);
  font-family: inherit;
  font-weight: var(--font-medium);
  line-height: 1;
  padding: 2px 5px;
  border-radius: 4px;
  border: 1px solid var(--colour-border);
  color: var(--colour-text-disabled);
}
</style>

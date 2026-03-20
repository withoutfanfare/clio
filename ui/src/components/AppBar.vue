<script setup lang="ts">
import { SButton, SKbd } from "@stuntrocket/ui";
import { useMemoryStore } from "@/stores/memories";

const store = useMemoryStore();
</script>

<template>
  <header class="appbar">
    <div class="appbar-inner">
      <div class="appbar-left" />

      <div class="appbar-right">
        <SButton
          variant="ghost"
          size="sm"
          @click="store.paletteOpen = true"
          title="Search (Cmd+K)"
        >
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
            <circle cx="7" cy="7" r="4.5" stroke="currentColor" stroke-width="1.5"/>
            <path d="M10.5 10.5L14 14" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
          <SKbd>K</SKbd>
        </SButton>
        <SButton
          variant="icon"
          size="sm"
          @click="store.toggleCompose()"
          title="New memory (Cmd+N)"
        >
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
            <path d="M8 3v10M3 8h10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
        </SButton>
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
</style>

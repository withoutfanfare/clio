<script setup lang="ts">
import { computed } from "vue";
import { SButton, SKbd } from "@stuntrocket/ui";
import { useMemoryStore } from "@/stores/memories";

const store = useMemoryStore();
const statusText = computed(() => {
  const status = store.connectionStatus;
  if (!status) return "Checking connection…";
  return `${status.label} · ${status.connected ? "Connected" : "Disconnected"}`;
});
</script>

<template>
  <header class="appbar">
    <div class="appbar-inner">
      <div class="appbar-left">
        <div
          class="backend-status"
          :class="{ disconnected: store.connectionStatus && !store.connectionStatus.connected }"
          :title="store.connectionStatus?.detail ?? undefined"
          role="status"
        >
          <span class="backend-dot" />
          {{ statusText }}
        </div>
      </div>

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
  justify-content: space-between;
  padding: 0 var(--space-4);
}

.backend-status {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  color: var(--color-text-secondary);
  font-size: 11px;
}

.backend-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--colour-success);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--colour-success) 15%, transparent);
}

.backend-status.disconnected .backend-dot {
  background: var(--colour-danger);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--colour-danger) 15%, transparent);
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

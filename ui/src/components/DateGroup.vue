<script setup lang="ts">
import { SSectionHeader } from "@stuntrocket/ui";

defineProps<{
  label: string;
  mode?: "list" | "grid";
}>();
</script>

<template>
  <div class="date-group">
    <SSectionHeader :title="label" />
    <div class="date-items" :class="mode === 'grid' ? 'layout-grid' : 'layout-list'">
      <slot />
    </div>
  </div>
</template>

<style scoped>
.date-group {
  margin-top: var(--space-8);
}

.date-group:first-child {
  margin-top: 0;
}

.layout-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.layout-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: var(--space-3);
}
</style>

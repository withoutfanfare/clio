<script setup lang="ts">
const props = defineProps<{
  modelValue: string;
}>();

const emit = defineEmits<{
  "update:modelValue": [kind: string];
}>();

const kinds = ["note", "fact", "decision", "summary", "task", "observation", "snippet", "knowledgebase"];
</script>

<template>
  <div class="kind-selector">
    <button
      v-for="kind in kinds"
      :key="kind"
      class="kind-pill"
      :class="{ active: modelValue === kind }"
      @click="emit('update:modelValue', kind)"
    >
      {{ kind }}
    </button>
  </div>
</template>

<style scoped>
.kind-selector {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-1);
}

.kind-pill {
  padding: var(--space-1) var(--space-3);
  background: var(--colour-surface-overlay);
  border: none;
  border-radius: var(--radius-sm);
  color: var(--colour-text-muted);
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  cursor: pointer;
  transition: color 150ms, background 150ms;
}

.kind-pill:hover {
  color: var(--colour-text);
  background: color-mix(in srgb, var(--colour-surface-overlay), white 4%);
}

.kind-pill.active {
  background: var(--colour-accent-muted);
  color: var(--colour-accent);
}
</style>

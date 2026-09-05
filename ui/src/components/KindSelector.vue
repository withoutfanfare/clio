<script setup lang="ts">
import { computed } from "vue";
import { useMemoryStore } from "@/stores/memories";
import { memoryKinds } from "@/utils/memoryKinds";
const props = defineProps<{
  modelValue: string;
}>();

const emit = defineEmits<{
  "update:modelValue": [kind: string];
}>();

const store = useMemoryStore();
const kinds = computed(() => memoryKinds(store.availableKinds, [props.modelValue]));
</script>

<template>
  <div class="kind-selector">
    <button
      v-for="kind in kinds"
      :key="kind"
      class="kind-pill"
      :class="{ active: modelValue === kind }"
      :aria-pressed="modelValue === kind"
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
  background: var(--color-surface-hover);
  border: none;
  border-radius: var(--radius-sm);
  color: var(--color-text-tertiary);
  font-size: 11px;
  font-weight: 500;
  cursor: pointer;
  transition: color 150ms, background 150ms;
}

.kind-pill:hover {
  color: var(--color-text-primary);
  background: var(--color-surface-selected);
}

.kind-pill.active {
  background: var(--color-accent-subtle);
  color: var(--color-accent);
}
</style>

<script setup lang="ts">
import { ref, computed } from "vue";

const props = withDefaults(
  defineProps<{
    modelValue: string[];
    suggestions?: string[];
    placeholder?: string;
  }>(),
  {
    suggestions: () => [],
    placeholder: "Add tag...",
  },
);

const emit = defineEmits<{
  "update:modelValue": [tags: string[]];
}>();

const input = ref("");
const dropdownOpen = ref(false);

const filteredSuggestions = computed(() => {
  const active = new Set(props.modelValue);
  const available = props.suggestions.filter((t) => !active.has(t));
  const query = input.value.trim().toLowerCase();
  if (!query) return available.slice(0, 20);
  return available.filter((t) => t.includes(query)).slice(0, 20);
});

function addTag() {
  const tag = input.value.trim().toLowerCase().replace(/\s+/g, "-");
  if (tag && !props.modelValue.includes(tag)) {
    emit("update:modelValue", [...props.modelValue, tag]);
  }
  input.value = "";
  dropdownOpen.value = false;
}

function selectTag(tag: string) {
  if (!props.modelValue.includes(tag)) {
    emit("update:modelValue", [...props.modelValue, tag]);
  }
  input.value = "";
  dropdownOpen.value = false;
}

function removeTag(tag: string) {
  emit(
    "update:modelValue",
    props.modelValue.filter((t) => t !== tag),
  );
}

function handleKeydown(e: KeyboardEvent) {
  if (e.key === "Enter" || e.key === ",") {
    e.preventDefault();
    addTag();
  }
  if (e.key === "Backspace" && !input.value && props.modelValue.length) {
    removeTag(props.modelValue[props.modelValue.length - 1]);
  }
  if (e.key === "Escape") {
    dropdownOpen.value = false;
  }
}

function onFocus() {
  if (props.suggestions.length) {
    dropdownOpen.value = true;
  }
}

function onBlur() {
  // Delay to allow click on dropdown items.
  setTimeout(() => {
    dropdownOpen.value = false;
    if (input.value.trim()) addTag();
  }, 150);
}
</script>

<template>
  <div class="tag-input-wrapper">
    <div class="tag-input">
      <span
        v-for="tag in modelValue"
        :key="tag"
        class="chip"
      >
        {{ tag }}
        <button class="chip-remove" @click="removeTag(tag)" aria-label="Remove tag">
          <svg width="10" height="10" viewBox="0 0 12 12" fill="none">
            <path d="M3 3l6 6M9 3l-6 6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
        </button>
      </span>
      <input
        v-model="input"
        class="chip-input"
        :placeholder="placeholder"
        @keydown="handleKeydown"
        @focus="onFocus"
        @blur="onBlur"
      />
    </div>
    <div v-if="dropdownOpen && filteredSuggestions.length" class="tag-dropdown">
      <button
        v-for="tag in filteredSuggestions"
        :key="tag"
        class="tag-option"
        @mousedown.prevent="selectTag(tag)"
      >
        <span class="tag-option-hash">#</span>{{ tag }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.tag-input-wrapper {
  position: relative;
}

.tag-input {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-1);
  padding: var(--space-1) var(--space-2);
  background: var(--colour-surface-input);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  min-height: 36px;
  align-items: center;
  transition: border-color 150ms, box-shadow 150ms;
}

.tag-input:focus-within {
  border-color: var(--color-accent);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-accent) 22%, transparent);
}

.tag-input:hover:not(:focus-within) {
  border-color: var(--color-border-strong);
}

.chip {
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 2px var(--space-2);
  background: var(--color-accent-subtle);
  color: var(--color-accent);
  border-radius: 99px;
  font-size: 11px;
  font-weight: 500;
}

.chip-remove {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 14px;
  height: 14px;
  background: none;
  border: none;
  border-radius: 9999px;
  color: var(--color-accent);
  cursor: pointer;
  opacity: 0;
  transition: opacity 150ms;
}

.chip:hover .chip-remove {
  opacity: 0.7;
}

.chip-remove:hover {
  opacity: 1 !important;
}

.chip-input {
  flex: 1;
  min-width: 80px;
  background: none;
  border: none;
  outline: none;
  color: var(--color-text-primary);
  font-size: 13px;
  font-family: inherit;
  padding: 2px var(--space-1);
}

.chip-input::placeholder {
  color: var(--color-text-tertiary);
}

.tag-dropdown {
  position: absolute;
  top: 100%;
  left: 0;
  right: 0;
  z-index: 10;
  margin-top: 2px;
  max-height: 180px;
  overflow-y: auto;
  background: var(--colour-surface-dropdown);
  border: 1px solid var(--glass-border);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-sheet);
  padding: 4px;
}

.tag-option {
  display: flex;
  align-items: center;
  gap: 2px;
  width: 100%;
  padding: 4px 8px;
  background: transparent;
  border: none;
  border-radius: var(--radius-sm);
  color: var(--color-text-secondary);
  font-size: 13px;
  cursor: pointer;
  text-align: left;
}

.tag-option:hover {
  background: var(--color-surface-hover);
  color: var(--color-text-primary);
}

.tag-option-hash {
  color: var(--color-accent);
  font-weight: 500;
}
</style>

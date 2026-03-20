<script setup lang="ts">
import { ref, nextTick, watch } from "vue";
import { SButton, SCard, SFormField, SInput, SKbd } from "@stuntrocket/ui";
import { useMemoryStore } from "@/stores/memories";
import TagInput from "./TagInput.vue";
import KindSelector from "./KindSelector.vue";

const store = useMemoryStore();
const text = ref("");
const detailsOpen = ref(false);
const title = ref("");
const kind = ref("note");
const namespace = ref("global");
const tags = ref<string[]>([]);
const importance = ref(3);
const submitting = ref(false);
const textareaRef = ref<HTMLTextAreaElement | null>(null);

watch(
  () => store.composeOpen,
  (open) => {
    if (open) {
      nextTick(() => textareaRef.value?.focus());
    } else {
      reset();
    }
  },
);

function reset() {
  text.value = "";
  title.value = "";
  kind.value = "note";
  namespace.value = "global";
  tags.value = [];
  importance.value = 3;
  detailsOpen.value = false;
}

async function submit() {
  if (!text.value.trim() || submitting.value) return;
  submitting.value = true;
  try {
    await store.captureMemory(
      text.value.trim(),
      namespace.value !== "global" ? namespace.value : undefined,
    );
    reset();
  } finally {
    submitting.value = false;
  }
}

function handleKeydown(e: KeyboardEvent) {
  if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
    e.preventDefault();
    submit();
  }
}

function autoResize(e: Event) {
  const el = e.target as HTMLTextAreaElement;
  el.style.height = "auto";
  el.style.height = el.scrollHeight + "px";
}
</script>

<template>
  <Transition name="fade">
    <SCard v-if="store.composeOpen" variant="glass" class="compose-area">
      <textarea
        ref="textareaRef"
        v-model="text"
        class="compose-input"
        placeholder="What's on your mind?"
        @keydown="handleKeydown"
        @input="autoResize"
        rows="3"
      />

      <div class="compose-footer">
        <button
          class="details-toggle"
          @click="detailsOpen = !detailsOpen"
        >
          <svg
            width="10" height="10" viewBox="0 0 12 12" fill="none"
            class="toggle-chevron"
            :class="{ open: detailsOpen }"
          >
            <path d="M4 2l4 4-4 4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
          </svg>
          {{ detailsOpen ? "Hide details" : "Add details" }}
        </button>

        <div class="compose-actions">
          <SButton
            variant="ghost"
            size="sm"
            @click="store.composeOpen = false"
          >
            Cancel
          </SButton>
          <SButton
            variant="primary"
            size="sm"
            @click="submit"
            :disabled="!text.trim() || submitting"
            :loading="submitting"
          >
            {{ submitting ? "Saving\u2026" : "Save" }}
            <SKbd>&#8984;&#9166;</SKbd>
          </SButton>
        </div>
      </div>

      <Transition name="fade">
        <div v-if="detailsOpen" class="compose-details">
          <SFormField label="Title">
            <SInput v-model="title" placeholder="Optional title" />
          </SFormField>

          <SFormField label="Kind">
            <KindSelector v-model="kind" />
          </SFormField>

          <SFormField label="Namespace">
            <SInput v-model="namespace" placeholder="global" />
          </SFormField>

          <SFormField label="Tags">
            <TagInput v-model="tags" />
          </SFormField>

          <SFormField label="Importance">
            <div class="importance-dots">
              <button
                v-for="n in 5"
                :key="n"
                class="importance-dot"
                :class="{ active: n <= importance }"
                @click="importance = n"
              />
            </div>
          </SFormField>
        </div>
      </Transition>
    </SCard>
  </Transition>
</template>

<style scoped>
.compose-area {
  padding: var(--space-4) var(--space-5);
  margin-bottom: var(--space-6);
}

.compose-input {
  width: 100%;
  background: none;
  border: none;
  outline: none;
  font-size: 15px;
  line-height: 1.65;
  color: var(--color-text-primary);
  resize: none;
  font-family: inherit;
  min-height: 72px;
}

.compose-input::placeholder {
  color: var(--color-text-tertiary);
}

.compose-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: var(--space-3);
  padding-top: var(--space-3);
  border-top: 1px solid var(--color-border-subtle);
}

.details-toggle {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  background: none;
  border: none;
  color: var(--color-text-tertiary);
  font-size: 13px;
  cursor: pointer;
  transition: color 150ms;
}

.details-toggle:hover {
  color: var(--color-text-primary);
}

.toggle-chevron {
  transition: transform 200ms cubic-bezier(0.4, 0, 0.2, 1);
}

.toggle-chevron.open {
  transform: rotate(90deg);
}

.compose-actions {
  display: flex;
  gap: var(--space-2);
  align-items: center;
}

.compose-details {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  margin-top: var(--space-3);
  padding-top: var(--space-3);
  border-top: 1px solid var(--color-border-subtle);
}

.importance-dots {
  display: flex;
  gap: var(--space-2);
}

.importance-dot {
  width: 14px;
  height: 14px;
  border-radius: 9999px;
  border: 2px solid var(--color-border-strong);
  background: transparent;
  cursor: pointer;
  transition: border-color 150ms, background 150ms;
  padding: 0;
}

.importance-dot.active {
  background: var(--color-accent);
  border-color: var(--color-accent);
}

.importance-dot:hover {
  border-color: var(--color-accent);
}
</style>

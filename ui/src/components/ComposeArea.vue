<script setup lang="ts">
import { ref, nextTick, watch } from "vue";
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
    <div v-if="store.composeOpen" class="compose-area glass">
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
          <button
            class="btn-ghost"
            @click="store.composeOpen = false"
          >
            Cancel
          </button>
          <button
            class="btn-primary"
            @click="submit"
            :disabled="!text.trim() || submitting"
          >
            {{ submitting ? "Saving\u2026" : "Save" }}
            <kbd class="kbd">&#8984;&#9166;</kbd>
          </button>
        </div>
      </div>

      <Transition name="fade">
        <div v-if="detailsOpen" class="compose-details">
          <div class="detail-row">
            <label class="detail-label">Title</label>
            <input v-model="title" class="detail-input" placeholder="Optional title" />
          </div>

          <div class="detail-row">
            <label class="detail-label">Kind</label>
            <KindSelector v-model="kind" />
          </div>

          <div class="detail-row">
            <label class="detail-label">Namespace</label>
            <input v-model="namespace" class="detail-input" placeholder="global" />
          </div>

          <div class="detail-row">
            <label class="detail-label">Tags</label>
            <TagInput v-model="tags" />
          </div>

          <div class="detail-row">
            <label class="detail-label">Importance</label>
            <div class="importance-dots">
              <button
                v-for="n in 5"
                :key="n"
                class="importance-dot"
                :class="{ active: n <= importance }"
                @click="importance = n"
              />
            </div>
          </div>
        </div>
      </Transition>
    </div>
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
  font-size: var(--text-base);
  line-height: var(--leading-relaxed);
  color: var(--colour-text);
  resize: none;
  font-family: inherit;
  min-height: 72px;
}

.compose-input::placeholder {
  color: var(--colour-text-disabled);
}

.compose-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: var(--space-3);
  padding-top: var(--space-3);
  border-top: 1px solid var(--colour-border);
}

.details-toggle {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  background: none;
  border: none;
  color: var(--colour-text-muted);
  font-size: var(--text-sm);
  cursor: pointer;
  transition: color 150ms;
}

.details-toggle:hover {
  color: var(--colour-text);
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

.btn-ghost {
  padding: var(--space-2) var(--space-3);
  background: none;
  border: none;
  border-radius: var(--radius-md);
  color: var(--colour-text-muted);
  font-size: var(--text-sm);
  cursor: pointer;
  transition: color 150ms, background 150ms;
}

.btn-ghost:hover {
  color: var(--colour-text);
  background: var(--colour-surface-overlay);
}

.btn-primary {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-4);
  background: var(--colour-accent);
  border: none;
  border-radius: var(--radius-md);
  color: white;
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  cursor: pointer;
  transition: background 150ms;
}

.btn-primary:hover:not(:disabled) {
  background: var(--colour-accent-hover);
}

.btn-primary:disabled {
  opacity: 0.4;
  cursor: default;
}

.btn-primary:focus-visible {
  outline: 2px solid var(--colour-border-focus);
  outline-offset: 2px;
}

.kbd {
  font-size: var(--text-xs);
  opacity: 0.6;
  font-family: inherit;
}

.compose-details {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  margin-top: var(--space-3);
  padding-top: var(--space-3);
  border-top: 1px solid var(--colour-border);
}

.detail-row {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.detail-label {
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  text-transform: uppercase;
  letter-spacing: var(--tracking-caps);
  color: var(--colour-text-muted);
}

.detail-input {
  padding: var(--space-2) var(--space-3);
  background: var(--colour-surface-input);
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-md);
  color: var(--colour-text);
  font-size: var(--text-sm);
  font-family: inherit;
  outline: none;
  transition: border-color 150ms;
}

.detail-input:hover {
  border-color: var(--colour-border-hover);
}

.detail-input:focus {
  border-color: var(--colour-border-focus);
  box-shadow: var(--shadow-focus);
}

.detail-input::placeholder {
  color: var(--colour-text-disabled);
}

.importance-dots {
  display: flex;
  gap: var(--space-2);
}

.importance-dot {
  width: 14px;
  height: 14px;
  border-radius: 9999px;
  border: 2px solid var(--colour-border-hover);
  background: transparent;
  cursor: pointer;
  transition: border-color 150ms, background 150ms;
  padding: 0;
}

.importance-dot.active {
  background: var(--colour-accent);
  border-color: var(--colour-accent);
}

.importance-dot:hover {
  border-color: var(--colour-accent);
}
</style>

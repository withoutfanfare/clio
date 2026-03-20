<script setup lang="ts">
import { ref, watch, nextTick, computed } from "vue";
import { useMemoryStore } from "@/stores/memories";

const store = useMemoryStore();

const content = ref("");
const title = ref("");
const namespace = ref(store.quickCreateLastNamespace);
const kind = ref(store.quickCreateLastKind);
const tags = ref<string[]>([]);
const tagInput = ref("");
const importance = ref(3);
const submitting = ref(false);
const contentRef = ref<HTMLTextAreaElement | null>(null);

const kinds = ["note", "observation", "decision", "constraint", "summary", "preference", "snippet"];

const open = ref(false);

watch(
  () => store.composeOpen,
  (val) => {
    open.value = val;
    if (val) {
      namespace.value = store.quickCreateLastNamespace;
      kind.value = store.quickCreateLastKind;
      nextTick(() => contentRef.value?.focus());
    } else {
      reset();
    }
  },
);

function reset() {
  content.value = "";
  title.value = "";
  tags.value = [];
  tagInput.value = "";
  importance.value = 3;
  submitting.value = false;
}

function addTag() {
  const t = tagInput.value.trim().toLowerCase();
  if (t && !tags.value.includes(t)) {
    tags.value = [...tags.value, t];
  }
  tagInput.value = "";
}

function removeTag(tag: string) {
  tags.value = tags.value.filter((t) => t !== tag);
}

async function submit() {
  if (!content.value.trim() || submitting.value) return;
  submitting.value = true;
  try {
    await store.quickCreate({
      content: content.value.trim(),
      namespace: namespace.value || "global",
      kind: kind.value || "note",
      tags: tags.value.length ? tags.value : undefined,
      title: title.value.trim() || undefined,
      importance: importance.value,
    });
    store.composeOpen = false;
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

function close() {
  store.composeOpen = false;
}
</script>

<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="open" class="qc-backdrop" @click="close" />
    </Transition>
    <Transition name="scale">
      <div v-if="open" class="qc-modal">
        <div class="qc-header">
          <h2 class="qc-title">New memory</h2>
          <button class="qc-close" @click="close" aria-label="Close">
            <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
              <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
            </svg>
          </button>
        </div>

        <div class="qc-body">
          <input
            v-model="title"
            class="qc-input"
            placeholder="Title (optional)"
          />

          <textarea
            ref="contentRef"
            v-model="content"
            class="qc-textarea"
            placeholder="What would you like to remember?"
            @keydown="handleKeydown"
            rows="4"
          />

          <div class="qc-fields">
            <div class="qc-field">
              <label class="qc-label">Kind</label>
              <select v-model="kind" class="qc-select">
                <option v-for="k in kinds" :key="k" :value="k">{{ k }}</option>
              </select>
            </div>

            <div class="qc-field">
              <label class="qc-label">Namespace</label>
              <select v-model="namespace" class="qc-select">
                <option value="global">global</option>
                <option
                  v-for="ns in store.allNamespaces.filter((n: string) => n !== 'global')"
                  :key="ns"
                  :value="ns"
                >
                  {{ ns }}
                </option>
              </select>
            </div>

            <div class="qc-field">
              <label class="qc-label">Importance</label>
              <div class="qc-importance">
                <button
                  v-for="n in 5"
                  :key="n"
                  class="imp-dot"
                  :class="{ active: n <= importance }"
                  @click="importance = n"
                />
              </div>
            </div>

            <div class="qc-field">
              <label class="qc-label">Tags</label>
              <div class="qc-tags-row">
                <input
                  v-model="tagInput"
                  class="qc-tag-input"
                  placeholder="Add tag..."
                  @keydown.enter.prevent="addTag"
                />
                <div v-if="tags.length" class="qc-tags">
                  <span v-for="tag in tags" :key="tag" class="qc-tag">
                    #{{ tag }}
                    <button class="qc-tag-remove" @click="removeTag(tag)">&times;</button>
                  </span>
                </div>
              </div>
            </div>
          </div>
        </div>

        <div class="qc-footer">
          <button class="qc-btn-ghost" @click="close">Cancel</button>
          <button
            class="qc-btn-primary"
            @click="submit"
            :disabled="!content.trim() || submitting"
          >
            {{ submitting ? "Saving\u2026" : "Save" }}
            <kbd class="qc-kbd">&#8984;&#9166;</kbd>
          </button>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.qc-backdrop {
  position: fixed;
  inset: 0;
  background: color-mix(in srgb, var(--grey-950) 60%, transparent);
  backdrop-filter: blur(2px);
  z-index: 500;
}

.qc-modal {
  position: fixed;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  width: 520px;
  max-width: 90vw;
  max-height: 85vh;
  overflow-y: auto;
  background: var(--colour-surface-dropdown);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  border: 1px solid var(--colour-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-overlay);
  z-index: 501;
}

.qc-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-4) var(--space-5);
  border-bottom: 1px solid var(--colour-border);
}

.qc-title {
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--colour-text);
}

.qc-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  background: none;
  border: none;
  border-radius: var(--radius-sm);
  color: var(--colour-text-muted);
  cursor: pointer;
  transition: color 150ms, background 150ms;
}

.qc-close:hover {
  color: var(--colour-text);
  background: var(--colour-surface-overlay);
}

.qc-body {
  padding: var(--space-4) var(--space-5);
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.qc-input,
.qc-textarea,
.qc-select,
.qc-tag-input {
  width: 100%;
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

.qc-input:focus,
.qc-textarea:focus,
.qc-select:focus,
.qc-tag-input:focus {
  border-color: var(--colour-border-focus);
  box-shadow: var(--shadow-focus);
}

.qc-input::placeholder,
.qc-textarea::placeholder,
.qc-tag-input::placeholder {
  color: var(--colour-text-disabled);
}

.qc-textarea {
  resize: vertical;
  min-height: 100px;
  line-height: var(--leading-relaxed);
}

.qc-select {
  appearance: none;
  cursor: pointer;
  background-image: url("data:image/svg+xml,%3Csvg width='10' height='6' viewBox='0 0 10 6' fill='none' xmlns='http://www.w3.org/2000/svg'%3E%3Cpath d='M1 1l4 4 4-4' stroke='%2378736e' stroke-width='1.5' stroke-linecap='round' stroke-linejoin='round'/%3E%3C/svg%3E");
  background-repeat: no-repeat;
  background-position: right 8px center;
  padding-right: 28px;
}

.qc-fields {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.qc-field {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.qc-label {
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  text-transform: uppercase;
  letter-spacing: var(--tracking-caps);
  color: var(--colour-text-muted);
}

.qc-importance {
  display: flex;
  gap: var(--space-2);
}

.imp-dot {
  width: 14px;
  height: 14px;
  border-radius: 9999px;
  border: 2px solid var(--colour-border-hover);
  background: transparent;
  cursor: pointer;
  transition: border-color 150ms, background 150ms;
  padding: 0;
}

.imp-dot.active {
  background: var(--colour-accent);
  border-color: var(--colour-accent);
}

.imp-dot:hover {
  border-color: var(--colour-accent);
}

.qc-tags-row {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.qc-tag-input {
  flex: 1;
}

.qc-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.qc-tag {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  padding: 1px 6px;
  border-radius: 99px;
  background: var(--colour-accent-muted);
  color: var(--colour-accent);
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
}

.qc-tag-remove {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 14px;
  height: 14px;
  padding: 0;
  background: transparent;
  border: none;
  color: var(--colour-accent);
  cursor: pointer;
  font-size: 12px;
  line-height: 1;
  border-radius: 50%;
}

.qc-footer {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: var(--space-2);
  padding: var(--space-3) var(--space-5);
  border-top: 1px solid var(--colour-border);
}

.qc-btn-ghost {
  padding: var(--space-2) var(--space-3);
  background: none;
  border: none;
  border-radius: var(--radius-md);
  color: var(--colour-text-muted);
  font-size: var(--text-sm);
  cursor: pointer;
  transition: color 150ms, background 150ms;
}

.qc-btn-ghost:hover {
  color: var(--colour-text);
  background: var(--colour-surface-overlay);
}

.qc-btn-primary {
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

.qc-btn-primary:hover:not(:disabled) {
  background: var(--colour-accent-hover);
}

.qc-btn-primary:disabled {
  opacity: 0.4;
  cursor: default;
}

.qc-kbd {
  font-size: var(--text-xs);
  opacity: 0.6;
  font-family: inherit;
}
</style>

<script setup lang="ts">
import { ref, watch, nextTick, computed, onUnmounted, onMounted } from "vue";
import { useMemoryStore } from "@/stores/memories";
import NativeDialog from "./NativeDialog.vue";
import { memoryKinds } from "@/utils/memoryKinds";
import { readDraft, writeDraft, removeDraft } from "@/composables/draftStorage";

const storageKey = "clio-create-draft";
interface CreateDraft {
  content: string; title: string; namespace: string; kind: string;
  tags: string[]; tagInput: string; importance: number; mode: "manual" | "automatic";
}
function isCreateDraft(value: unknown): value is CreateDraft {
  if (!value || typeof value !== "object") return false;
  const draft = value as CreateDraft;
  return [draft.content, draft.title, draft.namespace, draft.kind, draft.tagInput].every(field => typeof field === "string")
    && Array.isArray(draft.tags) && draft.tags.every(tag => typeof tag === "string")
    && Number.isInteger(draft.importance) && draft.importance >= 1 && draft.importance <= 5
    && ["manual", "automatic"].includes(draft.mode);
}

const store = useMemoryStore();
const content = ref("");
const title = ref("");
const namespace = ref("global");
const kind = ref("note");
const tags = ref<string[]>([]);
const tagInput = ref("");
const importance = ref(3);
const mode = ref<"manual" | "automatic">("manual");
const submitting = ref(false);
const recoveryError = ref<string | null>(null);
const recoveredDraft = readDraft(storageKey, isCreateDraft);
const submitError = ref<string | null>(null);
const confirmingDiscard = ref(false);
const contentRef = ref<HTMLTextAreaElement | null>(null);
const open = computed(() => store.composeOpen);
const hasDraft = computed(() => !!(content.value.trim() || title.value.trim() || tags.value.length || tagInput.value.trim()));
const namespaces = computed(() => [...new Set(["global", namespace.value, ...store.allNamespaces])]);
const kinds = computed(() => memoryKinds(store.availableKinds, [kind.value]));
if (recoveredDraft) {
  content.value = recoveredDraft.content;
  title.value = recoveredDraft.title;
  namespace.value = recoveredDraft.namespace;
  kind.value = recoveredDraft.kind;
  tags.value = recoveredDraft.tags;
  tagInput.value = recoveredDraft.tagInput;
  importance.value = recoveredDraft.importance;
  mode.value = recoveredDraft.mode;
}
onMounted(() => {
  if (recoveredDraft) store.composeOpen = true;
});

watch(() => ({
  content: content.value, title: title.value, namespace: namespace.value, kind: kind.value,
  tags: tags.value, tagInput: tagInput.value, importance: importance.value, mode: mode.value,
}), (draft) => {
  try {
    if (hasDraft.value) writeDraft(storageKey, draft);
    else removeDraft(storageKey);
    recoveryError.value = null;
  } catch {
    recoveryError.value = "Draft recovery is unavailable. Keep Clio open until this draft is saved.";
  }
}, { flush: "sync" });

watch(
  () => store.composeOpen,
  (value) => {
    if (value) {
      if (!hasDraft.value) {
        namespace.value = store.selectedNamespace || store.quickCreateLastNamespace || "global";
        kind.value = store.quickCreateLastKind || "note";
      }
      confirmingDiscard.value = false;
      nextTick(() => contentRef.value?.focus());
    }
  },
);

function reset() {
  content.value = "";
  title.value = "";
  tags.value = [];
  tagInput.value = "";
  importance.value = 3;
  submitError.value = null;
  confirmingDiscard.value = false;
}

function canClose() {
  if (submitting.value) return false;
  if (hasDraft.value) {
    confirmingDiscard.value = true;
    return false;
  }
  reset();
  return true;
}
store.setComposeCloseGuard(canClose);
onUnmounted(() => store.setComposeCloseGuard(null));

function addTag() {
  const tag = tagInput.value.trim().toLowerCase();
  if (tag && !tags.value.includes(tag)) tags.value = [...tags.value, tag];
  tagInput.value = "";
}

function removeTag(tag: string) {
  tags.value = tags.value.filter((item) => item !== tag);
}

async function submit() {
  if (!content.value.trim() || submitting.value) return;
  addTag();
  submitting.value = true;
  submitError.value = null;
  try {
    if (mode.value === "automatic") {
      await store.captureMemory(content.value.trim(), namespace.value || "global");
    } else {
      await store.quickCreate({
        content: content.value.trim(),
        namespace: namespace.value || "global",
        kind: kind.value || "note",
        tags: tags.value.length ? tags.value : undefined,
        title: title.value.trim() || undefined,
        importance: importance.value,
      });
    }
    reset();
    store.composeOpen = false;
  } catch {
    submitError.value = "Could not save. Your draft is still here. Check recent memories before retrying.";
  } finally {
    submitting.value = false;
  }
}

function handleKeydown(event: KeyboardEvent) {
  if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
    event.preventDefault();
    void submit();
  }
}

async function close() {
  return store.closeCompose();
}

async function discardAndClose() {
  if (submitting.value) return;
  reset();
  await close();
}
</script>

<template>
  <NativeDialog :open="open" label="New memory" @close="close">
      <div class="qc-modal" :aria-busy="submitting" @keydown="handleKeydown">
        <div class="qc-header">
          <h2 class="qc-title">New memory</h2>
          <button class="qc-close" @click="close" aria-label="Close">
            <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
              <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
            </svg>
          </button>
        </div>

        <div class="qc-body" :inert="submitting">
          <fieldset class="qc-mode">
            <legend class="qc-label">Entry method</legend>
            <label><input v-model="mode" type="radio" value="manual" /> Manual entry</label>
            <label><input v-model="mode" type="radio" value="automatic" /> Automatic capture</label>
          </fieldset>
          <p v-if="mode === 'automatic'" class="qc-hint">Automatically choose details from your text. Captures that need checking go to the review inbox.</p>
          <input
            v-if="mode === 'manual'"
            v-model="title"
            class="qc-input"
            aria-label="Memory title"
            placeholder="Title (optional)"
          />

          <textarea
            ref="contentRef"
            v-model="content"
            class="qc-textarea"
            aria-label="Memory content"
            autofocus
            placeholder="What would you like to remember?"
            rows="4"
          />

          <div class="qc-fields">
            <div v-if="mode === 'manual'" class="qc-field">
              <label for="qc-kind" class="qc-label">Kind</label>
              <select id="qc-kind" v-model="kind" class="qc-select">
                <option v-for="k in kinds" :key="k" :value="k">{{ k }}</option>
              </select>
            </div>

            <div class="qc-field">
              <label for="qc-namespace" class="qc-label">Namespace</label>
              <select id="qc-namespace" v-model="namespace" class="qc-select">
                <option
                  v-for="ns in namespaces"
                  :key="ns"
                  :value="ns"
                >
                  {{ ns }}
                </option>
              </select>
            </div>

            <div v-if="mode === 'manual'" class="qc-field">
              <span class="qc-label">Importance</span>
              <div class="qc-importance" role="group" aria-label="Importance">
                <button
                  v-for="n in 5"
                  :key="n"
                  class="imp-dot"
                  :class="{ active: n <= importance }"
                  @click="importance = n"
                  :aria-label="`Importance ${n} of 5`"
                  :aria-pressed="importance === n"
                />
              </div>
            </div>

            <div v-if="mode === 'manual'" class="qc-field">
              <label for="qc-tags" class="qc-label">Tags</label>
              <div class="qc-tags-row">
                <input
                  id="qc-tags"
                  v-model="tagInput"
                  class="qc-tag-input"
                  placeholder="Add tag..."
                  @keydown.enter.prevent="addTag"
                />
                <div v-if="tags.length" class="qc-tags">
                  <span v-for="tag in tags" :key="tag" class="qc-tag">
                    #{{ tag }}
                    <button class="qc-tag-remove" @click="removeTag(tag)" :aria-label="`Remove tag ${tag}`">&times;</button>
                  </span>
                </div>
              </div>
            </div>
          </div>
        </div>

        <div class="qc-feedback">
          <p class="qc-hint" role="status">{{ mode === 'automatic' ? 'Capture destination' : 'Saving to' }}: <strong>{{ namespace || "global" }}</strong></p>
          <p v-if="recoveredDraft && hasDraft" class="qc-hint">Recovered an unfinished draft. Review it before saving.</p>
          <p v-if="recoveryError" class="qc-error" role="alert">{{ recoveryError }}</p>
          <p v-if="submitError" class="qc-error" role="alert">{{ submitError }}</p>
          <div v-if="confirmingDiscard" role="alert">
            <p>Discard this unsaved draft?</p>
            <button class="qc-btn-ghost" @click="confirmingDiscard = false; contentRef?.focus()">Keep editing</button>
            <button class="qc-btn-ghost" :disabled="submitting" @click="discardAndClose">Discard draft</button>
          </div>
        </div>
        <div class="qc-footer">
          <button class="qc-btn-ghost" @click="close">Cancel</button>
          <button
            class="qc-btn-primary"
            @click="submit"
            :disabled="!content.trim() || submitting"
          >
            {{ submitting ? "Saving\u2026" : mode === "automatic" ? "Capture" : "Save" }}
            <kbd class="qc-kbd">&#8984;&#9166;</kbd>
          </button>
        </div>
      </div>
  </NativeDialog>
</template>

<style scoped>
.qc-mode { display: flex; flex-wrap: wrap; gap: var(--space-3); border: 0; padding: 0; }
.qc-mode label { display: flex; gap: var(--space-1); align-items: center; font-size: var(--text-sm); }
.qc-hint { font-size: var(--text-sm); color: var(--colour-text-muted); }
.qc-feedback { padding: 0 var(--space-5) var(--space-3); font-size: var(--text-sm); }
.qc-error { color: var(--color-danger); }

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

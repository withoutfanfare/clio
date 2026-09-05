<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { SBadge, SButton, SCard, SHeading } from "@stuntrocket/ui";
import * as api from "@/api/memory";
import type { ReviewEditsInput, ReviewItem } from "@/api/types";
import { useMemoryStore } from "@/stores/memories";
import { memoryKinds } from "@/utils/memoryKinds";
import { memoryTimestamp } from "@/utils/memoryPresentation";
import { writeDraft, removeDraft } from "@/composables/draftStorage";

const store = useMemoryStore();
const items = ref<ReviewItem[]>([]);
const selectedId = ref<string | null>(null);
const selected = computed(() => items.value.find(item => item.id === selectedId.value) ?? null);
const loading = ref(false);
const loadError = ref<string | null>(null);
const error = ref<string | null>(null);
const busy = ref<"save" | "approve" | "reject" | null>(null);
const confirmingReject = ref(false);
const saved = ref(false);
const recoveryError = ref<string | null>(null);
const recoveryBlocked = ref(false);
const recovered = ref(false);
const storageKey = "clio-inbox-draft";
type StoredDraft = { reviewId: string; edits: ReviewEditsInput };
const recovery = ref<StoredDraft | null>(null);
let applyingDraft = false;
let active = true;
let request = 0;

type Draft = { namespace: string; kind: string; title: string; summary: string; tags: string; importance: number };
const draft = ref<Draft>({ namespace: "global", kind: "note", title: "", summary: "", tags: "", importance: 3 });
const kinds = computed(() => memoryKinds(store.availableKinds, items.value.map(item => item.suggested_kind), [draft.value.kind]));
const changes = computed<ReviewEditsInput>(() => {
  const item = selected.value;
  if (!item) return {};
  const edits: ReviewEditsInput = {};
  if (draft.value.namespace !== item.suggested_namespace) edits.namespace = draft.value.namespace;
  if (draft.value.kind !== item.suggested_kind) edits.kind = draft.value.kind;
  if (draft.value.title !== (item.suggested_title ?? "")) edits.title = draft.value.title;
  if (draft.value.summary !== (item.suggested_summary ?? "")) edits.summary = draft.value.summary;
  const tags = [...new Set(draft.value.tags.split(",").map(tag => tag.trim()).filter(Boolean))];
  if (JSON.stringify(tags) !== JSON.stringify(item.suggested_tags)) edits.tags = tags;
  if (draft.value.importance !== item.suggested_importance) edits.importance = draft.value.importance;
  return edits;
});
const dirty = computed(() => Object.keys(changes.value).length > 0);
const recoveryStatus = computed(() => {
  if (!recovery.value) return null;
  if (selectedId.value !== recovery.value.reviewId) return `An unsaved draft for capture ${recovery.value.reviewId} is not in the loaded queue. It may be outside the oldest 100 or unavailable on this backend. Refresh to retry, or explicitly discard it before reviewing another capture.`;
  if (recoveryError.value) return "Unsaved suggestions remain in this view.";
  return recovered.value ? "Recovered unsaved suggestions. Review them before saving." : "Unsaved suggestions are retained locally until saved or explicitly discarded.";
});

function readRecovery() {
  try {
    const raw = localStorage.getItem(storageKey);
    if (!raw) return;
    if (raw.length > 1_000_000) throw new Error("Recovery exceeds the storage limit");
    const stored = JSON.parse(raw);
    const value = stored.draft;
    if (stored.version !== 1 || !value || typeof value.reviewId !== "string" || !value.reviewId || !value.edits || typeof value.edits !== "object" || Array.isArray(value.edits)) throw new Error("Unsupported recovery format");
    const entries = Object.entries(value.edits);
    if (!entries.length || !entries.every(([key, field]) => {
      if (["namespace", "kind", "title", "summary"].includes(key)) return typeof field === "string";
      if (key === "tags") return Array.isArray(field) && field.every(tag => typeof tag === "string");
      return key === "importance" && Number.isInteger(field) && Number(field) >= 1 && Number(field) <= 5;
    })) throw new Error("Unsupported suggestion fields");
    recovery.value = value;
  } catch {
    recoveryBlocked.value = true;
    recoveryError.value = "Could not read the existing inbox recovery. It has been kept. Explicitly discard it before editing another capture.";
  }
}

function clearRecovery(): boolean {
  try {
    removeDraft(storageKey);
    recovery.value = null;
    recoveryBlocked.value = false;
    recoveryError.value = null;
    recovered.value = false;
    return true;
  } catch {
    recoveryError.value = "Could not clear the local recovery copy. Retry discarding it before editing another capture.";
    return false;
  }
}

function persistDraft() {
  if (applyingDraft || !selected.value || recoveryBlocked.value) return;
  if (recovery.value && recovery.value.reviewId !== selected.value.id) return;
  if (!dirty.value) {
    if (recovery.value) clearRecovery();
    return;
  }
  recovery.value = { reviewId: selected.value.id, edits: { ...changes.value } };
  try {
    writeDraft(storageKey, recovery.value);
    recoveryError.value = null;
  } catch {
    recoveryError.value = "Local draft recovery is unavailable. Keep this inbox open until your suggestions are saved, or they may be lost.";
  }
}

readRecovery();
watch(draft, persistDraft, { deep: true, flush: "sync" });

function resetDraft(item: ReviewItem) {
  applyingDraft = true;
  draft.value = {
    namespace: item.suggested_namespace,
    kind: item.suggested_kind,
    title: item.suggested_title ?? "",
    summary: item.suggested_summary ?? "",
    tags: item.suggested_tags.join(", "),
    importance: item.suggested_importance,
  };
  applyingDraft = false;
}

function restoreRecovery(item: ReviewItem) {
  if (recovery.value?.reviewId !== item.id) return;
  selectedId.value = item.id;
  resetDraft(item);
  applyingDraft = true;
  const { tags, ...fields } = recovery.value.edits;
  Object.assign(draft.value, fields);
  if (tags) draft.value.tags = tags.join(", ");
  applyingDraft = false;
  recovered.value = true;
}

function selectItem(item: ReviewItem | undefined) {
  if (!item || busy.value || loading.value) return false;
  if (recoveryBlocked.value || (recovery.value && recovery.value.reviewId !== item.id)) {
    error.value = "Resolve or explicitly discard the existing recovery before reviewing another capture.";
    return false;
  }
  if (dirty.value) {
    error.value = "Save or discard your suggestion changes before choosing another capture.";
    return false;
  }
  selectedId.value = item.id;
  resetDraft(item);
  error.value = null;
  confirmingReject.value = false;
  saved.value = false;
  return true;
}

function discardRecovery(): boolean {
  if (busy.value || !clearRecovery()) return false;
  if (selected.value) resetDraft(selected.value);
  error.value = null;
  saved.value = false;
  return true;
}

function discardSuggestions() { return discardRecovery(); }

async function load() {
  if (dirty.value) {
    error.value = "Save or discard your suggestion changes before refreshing.";
    return;
  }
  const current = ++request;
  loading.value = true;
  loadError.value = null;
  try {
    const result = await api.inboxList();
    if (current !== request) return;
    items.value = result;
    const recoveredItem = result.find(item => item.id === recovery.value?.reviewId);
    if (recoveredItem) {
      restoreRecovery(recoveredItem);
      return;
    }
    const currentItem = result.find(item => item.id === selectedId.value);
    if (currentItem) resetDraft(currentItem);
    else selectedId.value = null;
  } catch (e) {
    if (current === request) loadError.value = `Could not refresh the review inbox: ${e}`;
  } finally {
    if (current === request) loading.value = false;
  }
}

async function saveSuggestions(): Promise<boolean> {
  if (!selected.value || busy.value || loading.value) return false;
  if (!dirty.value) return true;
  if (!draft.value.namespace.trim() || !draft.value.kind.trim()) {
    error.value = "Choose an exact destination namespace and kind before saving.";
    return false;
  }
  const id = selected.value.id;
  busy.value = "save";
  error.value = null;
  saved.value = false;
  try {
    const updated = await api.inboxEdit(id, changes.value);
    if (!active) return true;
    items.value = items.value.map(item => item.id === id ? updated : item);
    clearRecovery();
    resetDraft(updated);
    confirmingReject.value = false;
    saved.value = true;
    return true;
  } catch (e) {
    error.value = `Could not save suggestions. Your draft is still here: ${e}`;
    return false;
  } finally {
    busy.value = null;
  }
}

async function resolveItem(action: "approve" | "reject") {
  if (!selected.value || busy.value || loading.value) return;
  if (dirty.value) {
    error.value = "Save or discard your suggestion changes before approving or rejecting this capture.";
    return;
  }
  if (action === "reject" && !confirmingReject.value) {
    confirmingReject.value = true;
    return;
  }
  const id = selected.value.id;
  const resolvingRecovery = recovery.value;
  busy.value = action;
  error.value = null;
  try {
    if (action === "approve") {
      const memory = await api.inboxApprove(id);
      store.invalidateSearchCache();
      store.pushToast(`Approved into ${memory.namespace}`, "success");
    } else {
      await api.inboxReject(id);
      store.pushToast("Capture rejected", "info");
    }
    if (!active) return;
    if (selectedId.value === id && recovery.value?.reviewId === id && recovery.value === resolvingRecovery) clearRecovery();
    // Only a confirmed decision removes the item. Refill the oldest 100 next.
    items.value = items.value.filter(item => item.id !== id);
    selectedId.value = null;
    confirmingReject.value = false;
    saved.value = false;
    await load();
  } catch (e) {
    error.value = `Could not ${action} this capture. It remains open for review: ${e}`;
  } finally {
    busy.value = null;
  }
}

async function approve() { await resolveItem("approve"); }
async function reject() { await resolveItem("reject"); }

onMounted(load);
onUnmounted(() => { active = false; request++; });
</script>

<template>
  <div class="inbox-view">
    <header class="inbox-header">
      <SHeading :level="1">Review inbox</SHeading>
      <SButton variant="ghost" size="sm" :disabled="loading || !!busy || dirty" @click="load">{{ loading ? "Refreshing…" : "Refresh" }}</SButton>
    </header>
    <p class="inbox-scope">All workspaces on {{ store.connectionStatus?.label || "the configured backend" }} · unresolved captures, oldest first</p>
    <p class="inbox-scope">{{ items.length }} loaded · shows up to the oldest 100. After each decision, the next unresolved captures are loaded.</p>
    <p v-if="loadError" class="inbox-error" role="alert">{{ loadError }} Loaded items are kept.</p>
    <p v-if="error" class="inbox-error" role="alert">{{ error }}</p>
    <div v-if="recoveryStatus || recoveryError" class="recovery-notice">
      <p v-if="recoveryStatus" role="status">{{ recoveryStatus }}</p>
      <p v-if="recoveryError" class="inbox-error" role="alert">{{ recoveryError }}</p>
      <SButton v-if="recoveryBlocked || (recovery && (!selected || !dirty))" variant="ghost" size="sm" :disabled="!!busy" @click="discardRecovery">Discard retained inbox draft</SButton>
    </div>
    <p v-if="!loading && !loadError && !items.length" class="inbox-empty">No captures await review.</p>

    <div class="inbox-layout">
      <nav v-if="items.length" class="inbox-list" aria-label="Captures awaiting review">
        <button v-for="item in items" :key="item.id" class="review-list-item" :class="{ selected: item.id === selectedId }" :aria-current="item.id === selectedId ? 'true' : undefined" :disabled="!!busy || loading" @click="selectItem(item)">
          <strong>{{ item.suggested_title || item.content.slice(0, 90) }}</strong>
          <span>{{ item.suggested_namespace }} · {{ item.suggested_kind }}</span>
          <span>{{ item.status === 'edited' ? 'Edited, awaiting decision' : 'Awaiting review' }}</span>
        </button>
      </nav>

      <SCard v-if="selected" variant="glass" class="review-editor" :aria-busy="!!busy">
        <SHeading :level="2">Review capture</SHeading>
        <div class="review-meta">
          <SBadge>{{ selected.status }}</SBadge>
          <span>{{ memoryTimestamp(selected.created_at) }}</span>
          <span v-if="selected.source_route">Source: {{ selected.source_route }}</span>
          <span v-if="selected.source_ref">Reference: {{ selected.source_ref }}</span>
          <span v-if="selected.suggested_confidence !== null">Suggested confidence: {{ Math.round(selected.suggested_confidence * 100) }}%</span>
        </div>
        <h3 class="field-label">Captured content</h3>
        <pre class="capture-content">{{ selected.content }}</pre>
        <p class="approval-destination">Approval destination: <strong>{{ draft.namespace }}</strong> · <strong>{{ draft.kind }}</strong></p>

        <fieldset class="review-fields" :disabled="!!busy || loading">
          <legend class="field-label">Suggested details</legend>
          <label>Namespace<input v-model="draft.namespace" list="review-namespaces" /></label>
          <datalist id="review-namespaces"><option v-for="namespace in store.allNamespaces" :key="namespace" :value="namespace" /></datalist>
          <label>Kind<select v-model="draft.kind"><option v-for="kind in kinds" :key="kind" :value="kind">{{ kind }}</option></select></label>
          <label>Title<input v-model="draft.title" /></label>
          <label>Summary<textarea v-model="draft.summary" rows="2" /></label>
          <label>Tags, separated by commas<input v-model="draft.tags" /></label>
          <label>Importance<select v-model.number="draft.importance"><option v-for="importance in 5" :key="importance" :value="importance">{{ importance }} of 5</option></select></label>
        </fieldset>
        <p v-if="dirty" class="save-state" role="status">Unsaved suggestions. Save them before making a decision.</p>
        <p v-else-if="saved" class="save-state" role="status">Suggestions saved. This capture still awaits your decision.</p>
        <div class="review-actions">
          <SButton :disabled="!dirty || !!busy || loading" @click="saveSuggestions">{{ busy === 'save' ? "Saving…" : "Save suggestions" }}</SButton>
          <SButton v-if="dirty" variant="ghost" :disabled="!!busy" @click="discardSuggestions">Discard suggestion changes</SButton>
        </div>
        <div v-if="confirmingReject" class="reject-confirm" role="alert">
          <p>Reject this capture and remove it from the review queue?</p>
          <SButton variant="ghost" :disabled="!!busy" @click="confirmingReject = false">Keep reviewing</SButton>
          <SButton variant="danger" :disabled="!!busy || dirty" @click="reject">{{ busy === 'reject' ? "Rejecting…" : "Confirm rejection" }}</SButton>
        </div>
        <div v-else class="review-actions">
          <SButton :disabled="!!busy || dirty || loading" @click="approve">{{ busy === 'approve' ? "Approving…" : "Approve capture" }}</SButton>
          <SButton variant="ghost" :disabled="!!busy || dirty || loading" @click="reject">Reject…</SButton>
        </div>
      </SCard>
      <p v-else-if="items.length" class="inbox-empty">Choose a capture to review its content and suggested destination.</p>
    </div>
  </div>
</template>

<style scoped>
.inbox-view { display: flex; flex-direction: column; gap: var(--space-4); padding: 24px; }
.inbox-header { display: flex; align-items: center; justify-content: space-between; gap: var(--space-3); }
.inbox-scope, .inbox-empty { color: var(--color-text-secondary); font-size: 13px; }
.inbox-error { color: var(--color-danger); font-size: 13px; }
.recovery-notice { padding: var(--space-3); border: 1px solid var(--color-border-subtle); border-radius: var(--radius-sm); font-size: 13px; }
.inbox-layout { display: grid; grid-template-columns: minmax(180px, 1fr) minmax(0, 2fr); gap: var(--space-4); align-items: start; }
.inbox-list { display: flex; flex-direction: column; gap: var(--space-2); max-height: 75vh; overflow-y: auto; }
.review-list-item { display: flex; flex-direction: column; gap: var(--space-1); padding: var(--space-3); text-align: left; border: 1px solid var(--color-border-subtle); border-radius: var(--radius-md); background: var(--colour-surface-panel); color: var(--color-text-primary); cursor: pointer; }
.review-list-item span { font-size: 12px; color: var(--color-text-secondary); }
.review-list-item.selected { border-color: var(--color-accent); background: var(--color-accent-subtle); }
.review-list-item:disabled { cursor: wait; }
.review-editor { display: flex; flex-direction: column; gap: var(--space-3); padding: var(--space-4); min-width: 0; }
.review-meta { display: flex; flex-wrap: wrap; gap: var(--space-2); color: var(--color-text-secondary); font-size: 12px; }
.field-label { font-size: 13px; font-weight: 600; }
.capture-content { white-space: pre-wrap; overflow-wrap: anywhere; max-height: 32vh; overflow-y: auto; margin: 0; font: inherit; font-size: 13px; line-height: 1.6; padding: var(--space-3); background: var(--colour-surface-input); border-radius: var(--radius-sm); }
.approval-destination { font-size: 13px; }
.review-fields { border: 0; margin: 0; padding: 0; display: grid; grid-template-columns: 1fr 1fr; gap: var(--space-3); }
.review-fields label { display: flex; flex-direction: column; gap: var(--space-1); font-size: 13px; }
.review-fields input, .review-fields select, .review-fields textarea { width: 100%; padding: var(--space-2); border: 1px solid var(--color-border-subtle); border-radius: var(--radius-sm); background: var(--colour-surface-input); color: var(--color-text-primary); font: inherit; }
.review-actions { display: flex; flex-wrap: wrap; gap: var(--space-2); }
.save-state { font-size: 13px; color: var(--color-text-secondary); }
.reject-confirm { padding: var(--space-3); border: 1px solid var(--color-danger); border-radius: var(--radius-sm); font-size: 13px; }
@media (max-width: 760px) { .inbox-layout { grid-template-columns: 1fr; } .inbox-list { max-height: 30vh; } .review-fields { grid-template-columns: 1fr; } }
</style>

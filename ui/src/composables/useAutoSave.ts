import { ref, onUnmounted } from "vue";
import * as api from "@/api/memory";
import type { Memory, MemoryPatch } from "@/api/types";
import { readDraft, writeDraft, removeDraft } from "./draftStorage";

const storageKey = "clio-editor-draft";
type StoredDraft = { memoryId: string; expectedUpdatedAt: string; updates: MemoryPatch };
function isStoredDraft(value: unknown): value is StoredDraft {
  if (!value || typeof value !== "object") return false;
  const draft = value as StoredDraft;
  if (typeof draft.memoryId !== "string" || !draft.memoryId || typeof draft.expectedUpdatedAt !== "string" || !draft.expectedUpdatedAt || !draft.updates || typeof draft.updates !== "object" || Array.isArray(draft.updates)) return false;
  return Object.entries(draft.updates).every(([key, field]) => {
    if (["content", "kind", "namespace"].includes(key)) return typeof field === "string";
    if (key === "title") return field === null || typeof field === "string";
    if (key === "importance") return Number.isInteger(field) && Number(field) >= 1 && Number(field) <= 5;
    if (key === "tags") return Array.isArray(field) && field.every(tag => typeof tag === "string");
    return false;
  });
}

export function useAutoSave(delay = 2000) {
  const saving = ref(false);
  const dirty = ref(false);
  const saved = ref(false);
  const error = ref<string | null>(null);
  const recoveryError = ref<string | null>(null);
  const initialRecovery = readDraft(storageKey, isStoredDraft);
  const recoveryMemoryId = initialRecovery.status === "ready" ? initialRecovery.draft.memoryId : undefined;
  const recoveryBlocked = ref(initialRecovery.status === "error");
  if (recoveryBlocked.value) recoveryError.value = "The saved editor draft could not be recovered. It has been kept. Retry recovery or explicitly discard it before editing.";
  const unresolvedRecoveryId = ref(recoveryMemoryId ?? null);
  let timeout: ReturnType<typeof setTimeout> | undefined;
  let savedTimeout: ReturnType<typeof setTimeout> | undefined;
  type Draft = { memory: Memory; updates: MemoryPatch; expectedUpdatedAt: string };
  let pending: Draft | null = null;
  let active: Draft | null = null;
  let inFlight: Promise<boolean> | null = null;

  function persistDraft() {
    if (recoveryBlocked.value || unresolvedRecoveryId.value) return;
    const current = pending ?? active;
    try {
      if (current) {
        writeDraft(storageKey, {
          memoryId: current.memory.id,
          expectedUpdatedAt: current.expectedUpdatedAt,
          updates: { ...active?.updates, ...pending?.updates },
        });
      } else {
        removeDraft(storageKey);
      }
      recoveryError.value = null;
    } catch {
      recoveryError.value = "Draft recovery is unavailable. Keep Clio open until your changes are saved.";
    }
  }

  function retryRecovery(): boolean {
    if (pending || active || saving.value) return false;
    const result = readDraft(storageKey, isStoredDraft);
    recoveryBlocked.value = result.status === "error";
    if (result.status === "error") {
      recoveryError.value = "The saved editor draft could not be recovered. It has been kept. Retry recovery or explicitly discard it before editing.";
      return false;
    }
    unresolvedRecoveryId.value = result.status === "ready" ? result.draft.memoryId : null;
    recoveryError.value = null;
    return true;
  }

  function restoreDraft(memory: Memory): MemoryPatch | null {
    const result = readDraft(storageKey, isStoredDraft);
    saved.value = false;
    if (result.status === "error") {
      recoveryBlocked.value = true;
      recoveryError.value = "The saved editor draft could not be recovered. It has been kept. Retry recovery or explicitly discard it before editing.";
      return null;
    }
    recoveryBlocked.value = false;
    recoveryError.value = null;
    if (result.status === "missing") { unresolvedRecoveryId.value = null; return null; }
    const draft = result.draft;
    if (draft.memoryId !== memory.id) { unresolvedRecoveryId.value = draft.memoryId; return null; }
    unresolvedRecoveryId.value = null;
    pending = { memory, updates: draft.updates, expectedUpdatedAt: draft.expectedUpdatedAt };
    dirty.value = true;
    error.value = "Recovered unsaved changes. Review your draft before retrying the save.";
    return draft.updates;
  }

  async function savePending(): Promise<boolean> {
    saving.value = true;
    try {
      while (pending) {
        const next: Draft = pending;
        pending = null;
        active = next;
        try {
          const updated = await api.updateMemory(next.memory.id, {
            ...next.updates,
            expected_updated_at: next.expectedUpdatedAt,
          });
          Object.assign(next.memory, updated);
          // Only our successful write can advance the draft's conflict token.
          if (pending) (pending as Draft).expectedUpdatedAt = updated.updated_at;
          active = null;
          persistDraft();
          error.value = null;
        } catch (e) {
          pending = {
            ...next,
            updates: { ...next.updates, ...(pending as Draft | null)?.updates },
          };
          dirty.value = true;
          persistDraft();
          error.value = String(e);
          return false;
        }
      }
      dirty.value = false;
      saved.value = true;
      savedTimeout = setTimeout(() => (saved.value = false), 4000);
      return true;
    } finally {
      active = null;
      saving.value = false;
    }
  }

  function flush(): Promise<boolean> {
    clearTimeout(timeout);
    if (recoveryBlocked.value || unresolvedRecoveryId.value) return Promise.resolve(false);
    if (inFlight) return inFlight;
    if (!pending) return Promise.resolve(true);
    inFlight = savePending().finally(() => { inFlight = null; });
    return inFlight;
  }

  function scheduleAutoSave(memory: Memory, updates: MemoryPatch) {
    if (recoveryBlocked.value || unresolvedRecoveryId.value) {
      recoveryError.value = "Open or explicitly discard the recovered draft before editing another memory.";
      return false;
    }
    // The editor must flush or explicitly discard before changing memories.
    const current = pending ?? active;
    if (current && current.memory.id !== memory.id) return false;
    clearTimeout(timeout);
    clearTimeout(savedTimeout);
    pending = {
      memory,
      updates: { ...pending?.updates, ...updates },
      expectedUpdatedAt: current?.expectedUpdatedAt ?? memory.updated_at,
    };
    dirty.value = true;
    saved.value = false;
    persistDraft();
    // A failed draft needs a deliberate retry; typing must not hide the error.
    if (!error.value) timeout = setTimeout(() => void flush(), delay);
    return true;
  }

  function discard() {
    if (saving.value) return false;
    try { removeDraft(storageKey); }
    catch {
      recoveryError.value = "Could not discard the saved editor draft. It remains protected; retry when storage is available.";
      return false;
    }
    clearTimeout(timeout);
    clearTimeout(savedTimeout);
    pending = null;
    unresolvedRecoveryId.value = null;
    recoveryBlocked.value = false;
    recoveryError.value = null;
    dirty.value = false;
    saved.value = false;
    error.value = null;
    return true;
  }

  onUnmounted(() => {
    clearTimeout(timeout);
    clearTimeout(savedTimeout);
  });

  return { saving, dirty, saved, error, recoveryError, recoveryMemoryId, recoveryBlocked, unresolvedRecoveryId, retryRecovery, restoreDraft, scheduleAutoSave, flush, discard };
}

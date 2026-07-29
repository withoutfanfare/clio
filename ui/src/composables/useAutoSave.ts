import { ref, onUnmounted } from "vue";
import * as api from "@/api/memory";
import type { Memory, MemoryPatch } from "@/api/types";

export function useAutoSave(delay = 2000) {
  const saving = ref(false);
  const dirty = ref(false);
  const saved = ref(false);
  const error = ref<string | null>(null);
  let timeout: ReturnType<typeof setTimeout>;
  let savedTimeout: ReturnType<typeof setTimeout>;
  let inFlight = false;
  let pendingReady = false;
  let pending: { memory: Memory; updates: MemoryPatch } | null = null;

  async function flush() {
    if (inFlight || !pendingReady || !pending) return;

    const next = pending;
    pending = null;
    pendingReady = false;
    inFlight = true;
    saving.value = true;
    dirty.value = false;

    try {
      const updated = await api.updateMemory(next.memory.id, {
        ...next.updates,
        expected_updated_at: next.memory.updated_at,
      });
      Object.assign(next.memory, updated);
      error.value = null;
      if (!pending) {
        saved.value = true;
        savedTimeout = setTimeout(() => {
          saved.value = false;
        }, 4000);
      }
    } catch (e) {
      error.value = String(e);
    } finally {
      inFlight = false;
      saving.value = false;
      if (pendingReady) void flush();
    }
  }

  function scheduleAutoSave(memory: Memory, updates: MemoryPatch) {
    clearTimeout(timeout);
    clearTimeout(savedTimeout);
    pending = {
      memory,
      updates:
        pending?.memory.id === memory.id
          ? { ...pending.updates, ...updates }
          : updates,
    };
    pendingReady = false;
    dirty.value = true;
    saved.value = false;
    error.value = null;

    timeout = setTimeout(() => {
      pendingReady = true;
      void flush();
    }, delay);
  }

  function cancel() {
    clearTimeout(timeout);
    clearTimeout(savedTimeout);
    pending = null;
    pendingReady = false;
    dirty.value = false;
  }

  onUnmounted(() => {
    cancel();
  });

  return { saving, dirty, saved, error, scheduleAutoSave, cancel };
}

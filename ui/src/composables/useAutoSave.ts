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

  function scheduleAutoSave(memory: Memory, updates: MemoryPatch) {
    clearTimeout(timeout);
    clearTimeout(savedTimeout);
    dirty.value = true;
    saved.value = false;
    saving.value = false;
    error.value = null;

    timeout = setTimeout(async () => {
      saving.value = true;
      dirty.value = false;
      try {
        const updated = await api.updateMemory(memory.id, {
          ...updates,
          expected_updated_at: memory.updated_at,
        });
        Object.assign(memory, updated);
        saved.value = true;
        error.value = null;
        // Clear "Saved" after 4 seconds so it doesn't linger forever
        savedTimeout = setTimeout(() => {
          saved.value = false;
        }, 4000);
      } catch (e) {
        error.value = String(e);
      } finally {
        saving.value = false;
      }
    }, delay);
  }

  function cancel() {
    clearTimeout(timeout);
    clearTimeout(savedTimeout);
    dirty.value = false;
  }

  onUnmounted(() => {
    cancel();
  });

  return { saving, dirty, saved, error, scheduleAutoSave, cancel };
}

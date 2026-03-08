import { ref } from "vue";
import * as api from "@/api/memory";
import type { Memory, RememberInput } from "@/api/types";

export function useAutoSave(delay = 2000) {
  const saving = ref(false);
  const dirty = ref(false);
  const saved = ref(false);
  const error = ref<string | null>(null);
  let timeout: ReturnType<typeof setTimeout>;
  let savedTimeout: ReturnType<typeof setTimeout>;

  function scheduleAutoSave(memory: Memory, updates: Partial<RememberInput>) {
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
        await api.updateMemory(memory.id, {
          namespace: updates.namespace ?? memory.namespace,
          kind: updates.kind ?? memory.kind,
          title: updates.title ?? memory.title ?? undefined,
          summary: updates.summary ?? memory.summary ?? undefined,
          content: updates.content ?? memory.content,
          tags: updates.tags ?? memory.tags,
          importance: updates.importance ?? memory.importance,
          source: memory.source ?? undefined,
          source_ref: memory.source_ref ?? undefined,
        });
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

  return { saving, dirty, saved, error, scheduleAutoSave, cancel };
}

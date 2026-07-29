<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { SButton } from "@stuntrocket/ui";
import * as api from "@/api/memory";
import type { CapturePreferences } from "@/api/types";
import { useMemoryStore } from "@/stores/memories";

const store = useMemoryStore();
const preferences = ref<CapturePreferences | null>(null);
const model = ref("");
const loading = ref(true);
const saving = ref(false);
const error = ref<string | null>(null);
const saved = ref(false);

const hasChanges = computed(
  () => !!preferences.value && model.value.trim() !== preferences.value.model,
);

watch(model, () => {
  if (hasChanges.value) saved.value = false;
});

async function loadPreferences() {
  loading.value = true;
  error.value = null;
  try {
    preferences.value = await api.capturePreferences();
    model.value = preferences.value.model;
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }
}

async function saveModel() {
  const nextModel = model.value.trim();
  if (!nextModel) {
    error.value = "Enter a model name.";
    return;
  }

  saving.value = true;
  saved.value = false;
  error.value = null;
  try {
    preferences.value = await api.setCaptureModel(nextModel);
    model.value = preferences.value.model;
    saved.value = true;
  } catch (e) {
    error.value = String(e);
  } finally {
    saving.value = false;
  }
}

onMounted(loadPreferences);
</script>

<template>
  <div class="settings-view">
    <header class="settings-header">
      <div>
        <h1>Settings</h1>
        <p>
          Changes apply to {{ store.isRemote ? "the shared Atlas backend" : "this local Clio database" }}.
        </p>
      </div>
    </header>

    <section class="settings-card" aria-labelledby="capture-heading">
      <div class="card-heading">
        <div>
          <h2 id="capture-heading">Capture model</h2>
          <p>Used to classify, distil and consolidate memories.</p>
        </div>
        <span v-if="preferences" class="status-pill" :class="{ disabled: !preferences.enabled }">
          {{ preferences.enabled ? "Capture on" : "Capture off" }}
        </span>
      </div>

      <div v-if="loading" class="settings-state" role="status">Loading capture settings…</div>

      <form v-else class="model-form" @submit.prevent="saveModel">
        <label for="capture-model">Model ID</label>
        <div class="model-row">
          <input
            id="capture-model"
            v-model="model"
            list="capture-model-options"
            type="text"
            autocomplete="off"
            spellcheck="false"
            placeholder="gpt-5.6-terra"
            :disabled="saving"
          />
          <datalist id="capture-model-options">
            <option value="gpt-4.1" />
            <option value="gpt-5.6-luna" />
            <option value="gpt-5.6-terra" />
          </datalist>
          <SButton
            type="submit"
            variant="primary"
            size="sm"
            :disabled="saving || !model.trim() || !hasChanges"
            :loading="saving"
          >
            {{ saving ? "Saving…" : "Save model" }}
          </SButton>
        </div>
        <p class="field-help">
          Choose a benchmarked model or enter another OpenAI-compatible model ID.
          Credentials and the API endpoint are preserved.
        </p>

        <dl v-if="preferences" class="capture-details">
          <div>
            <dt>Active model</dt>
            <dd>{{ preferences.model }}</dd>
          </div>
          <div>
            <dt>Review threshold</dt>
            <dd>{{ preferences.review_threshold ?? "Off" }}</dd>
          </div>
        </dl>

        <p v-if="saved" class="settings-success" role="status">
          Capture model updated. Other running clients reload it within 30 seconds.
        </p>
        <p v-if="error" class="settings-error" role="alert">{{ error }}</p>
      </form>
    </section>
  </div>
</template>

<style scoped>
.settings-view {
  max-width: 760px;
  margin: 0 auto;
  padding: var(--space-5) 0 var(--space-8);
}

.settings-header {
  margin-bottom: var(--space-5);
}

.settings-header h1 {
  margin: 0 0 var(--space-1);
  color: var(--color-text-primary);
  font-size: 24px;
  font-weight: 600;
}

.settings-header p,
.card-heading p,
.field-help {
  margin: 0;
  color: var(--color-text-secondary);
  font-size: 13px;
  line-height: 1.5;
}

.settings-card {
  padding: var(--space-5);
  background: var(--colour-surface-card);
  border: 1px solid var(--glass-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--glass-glow);
}

.card-heading {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-4);
  margin-bottom: var(--space-5);
}

.card-heading h2 {
  margin: 0 0 var(--space-1);
  color: var(--color-text-primary);
  font-size: 16px;
  font-weight: 600;
}

.status-pill {
  flex: 0 0 auto;
  padding: 3px 8px;
  color: var(--colour-success);
  background: var(--color-success-subtle);
  border-radius: 999px;
  font-size: 11px;
  font-weight: 600;
}

.status-pill.disabled {
  color: var(--color-text-secondary);
  background: var(--colour-surface-overlay);
}

.model-form label {
  display: block;
  margin-bottom: var(--space-2);
  color: var(--color-text-primary);
  font-size: 12px;
  font-weight: 600;
}

.model-row {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.model-row input {
  min-width: 0;
  flex: 1;
  padding: 9px 11px;
  color: var(--color-text-primary);
  background: var(--colour-surface-input);
  border: 1px solid var(--glass-border);
  border-radius: var(--radius-md);
  font: inherit;
  font-size: 13px;
}

.model-row input:focus {
  border-color: var(--colour-border-focus);
  outline: none;
  box-shadow: 0 0 0 3px var(--colour-accent-muted);
}

.field-help {
  margin-top: var(--space-2);
}

.capture-details {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-3);
  margin: var(--space-5) 0 0;
}

.capture-details div {
  padding: var(--space-3);
  background: var(--colour-surface-overlay);
  border-radius: var(--radius-md);
}

.capture-details dt {
  margin-bottom: 3px;
  color: var(--color-text-tertiary);
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: var(--tracking-caps);
}

.capture-details dd {
  margin: 0;
  color: var(--color-text-primary);
  font-size: 13px;
}

.settings-state,
.settings-success,
.settings-error {
  color: var(--color-text-secondary);
  font-size: 13px;
}

.settings-success,
.settings-error {
  margin: var(--space-4) 0 0;
}

.settings-success {
  color: var(--colour-success);
}

.settings-error {
  color: var(--colour-danger);
}

@media (max-width: 640px) {
  .model-row {
    align-items: stretch;
    flex-direction: column;
  }

  .capture-details {
    grid-template-columns: 1fr;
  }
}
</style>

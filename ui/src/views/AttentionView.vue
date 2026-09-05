<script setup lang="ts">
import { computed, onMounted, onUnmounted, watch, ref } from "vue";
import { SBadge, SButton, SCard, SHeading, SInput } from "@stuntrocket/ui";
import { invoke } from "@tauri-apps/api/core";
import { memoryTimestamp } from "@/utils/memoryPresentation";
import * as api from "@/api/memory";
import type {
  AttentionItem,
  AttentionOverview,
  CaptureQueueHealth,
  EligibleAttention,
} from "@/api/types";
import { useMemoryStore } from "@/stores/memories";
import { useRouter } from "vue-router";

const store = useMemoryStore();
const router = useRouter();

const overview = ref<AttentionOverview | null>(null);
const queueHealth = ref<CaptureQueueHealth | null>(null);
const queueHealthLoaded = ref(false);
const queueHealthError = ref<string | null>(null);
const queueLoading = ref(false);
const queueCheckedAt = ref<string | null>(null);
const copied = ref(false);
const actionError = ref<string | null>(null);
let request = 0;
let queueRequest = 0;
const loading = ref(true);
const error = ref<string | null>(null);
const busyId = ref<string | null>(null);
const snoozeFor = ref<string | null>(null);
const snoozeUntil = ref("");

const reasonLabels: Record<string, string> = {
  overdue: "Overdue",
  reminder_due: "Reminder due",
  project_session: "This project",
  dormant: "Dormant",
};

const eligibleIds = computed(
  () => new Set((overview.value?.eligible ?? []).map((item) => item.id)),
);

// Open/snoozed items that are not already shown in the eligible section.
const remainingOpen = computed(() =>
  (overview.value?.open ?? []).filter((item) => !eligibleIds.value.has(item.id)),
);

const titlesSupported = computed(() => overview.value?.memory_titles !== undefined);
function canViewEvidence(item: AttentionItem): boolean {
  return !!overview.value?.memory_titles && Object.prototype.hasOwnProperty.call(overview.value.memory_titles, item.memory_id);
}
function memoryTitle(item: AttentionItem): string {
  return overview.value?.memory_titles?.[item.memory_id] || "Evidence unavailable";
}
function countLabel(value: number | null | undefined): string {
  return value == null ? "Unknown" : value.toLocaleString("en-GB");
}
const queueWaiting = computed(() => {
  const health = queueHealth.value;
  return health?.pending == null || health.processing == null ? null : health.pending + health.processing;
});
const oldestPending = computed(() => {
  const health = queueHealth.value;
  if (!health || health.pending === null) return "Unknown";
  if (health.pending === 0) return "No pending entries";
  if (health.oldest_pending_age_secs === null) return "Unknown";
  const minutes = Math.floor(health.oldest_pending_age_secs / 60);
  return minutes < 60 ? `${minutes} min` : minutes < 1440 ? `${Math.floor(minutes / 60)} hr` : `${Math.floor(minutes / 1440)} days`;
});
const diagnostics = computed(() => JSON.stringify({
  scope: "local",
  checked_at: queueHealth.value?.checked_at ?? queueCheckedAt.value,
  available: queueHealth.value !== null,
  pending: queueHealth.value?.pending ?? null,
  processing: queueHealth.value?.processing ?? null,
  dead: queueHealth.value?.dead ?? null,
  oldest_pending_age_secs: queueHealth.value?.oldest_pending_age_secs ?? null,
  unavailable_buckets: queueHealth.value?.unavailable_buckets ?? [],
}, null, 2));

async function copyDiagnostics() {
  try {
    await invoke("cmd_copy_to_clipboard", { text: diagnostics.value });
    copied.value = true;
  } catch {
    actionError.value = "Could not copy diagnostics.";
  }
}

async function loadOverview() {
  const current = ++request;
  const namespace = store.selectedNamespace;
  loading.value = true;
  overview.value = null;
  error.value = null;
  actionError.value = null;
  snoozeFor.value = null;
  try {
    const result = await api.attentionOverview(namespace);
    if (current === request && namespace === store.selectedNamespace) overview.value = result;
  } catch (e) {
    if (current === request && namespace === store.selectedNamespace) error.value = String(e);
  } finally {
    if (current === request && namespace === store.selectedNamespace) loading.value = false;
  }
}

async function loadQueueHealth() {
  const current = ++queueRequest;
  queueLoading.value = true;
  queueHealthError.value = null;
  copied.value = false;
  try {
    const result = await api.captureQueueHealth();
    if (current === queueRequest) queueHealth.value = result;
  } catch (e) {
    if (current === queueRequest) {
      queueHealth.value = null;
      queueHealthError.value = String(e);
    }
  } finally {
    if (current === queueRequest) {
      queueHealthLoaded.value = true;
      queueLoading.value = false;
      queueCheckedAt.value = new Date().toISOString();
    }
  }
}

async function load() {
  await Promise.all([loadOverview(), loadQueueHealth()]);
}

// The row stays on screen until the server confirms its new state.
async function complete(item: AttentionItem) {
  if (busyId.value) return;
  actionError.value = null;
  busyId.value = item.id;
  try {
    await api.actionComplete({ id: item.id });
    store.pushToast("Marked as done", "info");
    await load();
  } catch (e) {
    actionError.value = `Could not complete this item: ${e}`;
  } finally {
    busyId.value = null;
  }
}

function openSnooze(item: AttentionItem) {
  if (busyId.value) return;
  snoozeFor.value = item.id;
  const tomorrow = new Date(Date.now() + 24 * 60 * 60 * 1000);
  snoozeUntil.value = tomorrow.toISOString().slice(0, 10);
}

async function confirmSnooze(item: AttentionItem) {
  if (busyId.value || !snoozeUntil.value) return;
  actionError.value = null;
  busyId.value = item.id;
  try {
    await api.actionSnooze({
      id: item.id,
      until: `${snoozeUntil.value}T09:00:00Z`,
    });
    store.pushToast(`Snoozed until ${snoozeUntil.value}`, "info");
    snoozeFor.value = null;
    await load();
  } catch (e) {
    actionError.value = `Could not snooze this item: ${e}`;
  } finally {
    busyId.value = null;
  }
}

async function cancel(item: AttentionItem) {
  if (busyId.value) return;
  actionError.value = null;
  busyId.value = item.id;
  try {
    await api.actionCancel({ id: item.id });
    store.pushToast("Cancelled", "info");
    await load();
  } catch (e) {
    actionError.value = `Could not cancel this item: ${e}`;
  } finally {
    busyId.value = null;
  }
}

async function viewEvidence(item: AttentionItem) {
  if (!canViewEvidence(item)) return;
  const current = request;
  const namespace = store.selectedNamespace;
  const isCurrent = () => current === request && namespace === store.selectedNamespace;
  const opened = await store.openDrawer(item.memory_id, { eligibleOnly: true, isCurrent });
  if (!opened && isCurrent()) actionError.value = "Evidence could not be opened. Refresh attention to check whether it is still available.";
}

function whyNow(item: EligibleAttention): string {
  switch (item.reason) {
    case "overdue":
      return item.due_at ? `was due ${item.due_at.slice(0, 10)}` : "overdue";
    case "reminder_due":
      return item.remind_at
        ? `reminder set for ${item.remind_at.slice(0, 10)}`
        : "reminder due";
    case "project_session":
      return "you asked to be reminded in this project";
    case "dormant":
      return `untouched since ${item.updated_at.slice(0, 10)}`;
    default:
      return item.reason;
  }
}

onMounted(load);
watch(() => store.selectedNamespace, () => void loadOverview());
onUnmounted(() => { request++; queueRequest++; });
</script>

<template>
  <div class="attention-view">
    <div class="attention-header">
      <SHeading :level="1">Needs attention</SHeading>
      <SButton variant="ghost" size="sm" :disabled="loading || queueLoading || !!busyId" @click="load">Refresh</SButton>
    </div>
    <p class="scope-label">{{ store.selectedNamespace || "All workspaces" }} · {{ store.connectionStatus?.label || "Configured backend" }}</p>
    <SCard variant="glass" class="capture-diagnostics">
      <div class="attention-header">
        <div>
          <strong>Capture queue on this Mac</strong>
          <p class="scope-label">Local diagnostics · separate from the connected memory backend</p>
        </div>
        <SButton variant="ghost" size="sm" :disabled="queueLoading" @click="loadQueueHealth">{{ queueLoading ? "Checking…" : "Refresh" }}</SButton>
      </div>
      <p v-if="!queueHealthLoaded">Checking local queue…</p>
      <p v-else-if="queueHealthError" role="alert">Local queue diagnostics could not be read. {{ queueHealthError }}</p>
      <p v-else-if="!queueHealth">No local capture queue is available.</p>
      <template v-else>
        <p>{{ countLabel(queueWaiting) }} waiting or processing · {{ countLabel(queueHealth.dead) }} retained failed entries</p>
        <details>
          <summary>Read-only diagnostics</summary>
          <dl class="queue-counts">
            <dt>Pending</dt><dd>{{ countLabel(queueHealth.pending) }}</dd>
            <dt>Processing</dt><dd>{{ countLabel(queueHealth.processing) }}</dd>
            <dt>Retained failed entries</dt><dd>{{ countLabel(queueHealth.dead) }}</dd>
            <dt>Oldest pending</dt><dd>{{ oldestPending }}</dd>
          </dl>
          <p v-if="queueHealth.unavailable_buckets?.length">Unknown counts: {{ queueHealth.unavailable_buckets.join(", ") }}. These entries could not be read.</p>
        </details>
      </template>
      <p v-if="queueHealthLoaded" class="scope-label">Checked {{ memoryTimestamp(queueHealth?.checked_at || queueCheckedAt || '') }}</p>
      <p class="operator-guidance">Share these diagnostics with the Clio operator for the separate capture recovery workflow (CLIO-OPS-008). Queue counts do not establish whether captures have already been recovered.</p>
      <SButton v-if="queueHealthLoaded" variant="ghost" size="sm" @click="copyDiagnostics">{{ copied ? "Diagnostics copied" : "Copy diagnostics" }}</SButton>
    </SCard>
    <p v-if="actionError" class="action-error" role="alert">{{ actionError }}</p>

    <SCard v-if="error" variant="glass" class="error-card" role="alert">
      <strong>Couldn't load attention data.</strong>
      <p>{{ error }}</p>
      <SButton size="sm" @click="load">Retry</SButton>
    </SCard>

    <template v-else-if="!loading && overview">
      <!-- System health strip -->
      <div class="health-row">
        <SCard variant="glass" class="health-card">
          <span class="health-value">{{ overview.review_pending }}</span>
          <span class="health-label">awaiting review · all backend workspaces</span>
          <SButton variant="ghost" size="sm" @click="router.push({ name: 'inbox' })">Review captures</SButton>
        </SCard>
        <SCard
          v-if="overview.consolidation_stale !== null"
          variant="glass"
          class="health-card"
        >
          <span
            class="health-value"
            :class="{ warn: overview.consolidation_stale }"
          >
            {{ overview.consolidation_stale ? "stale" : "fresh" }}
          </span>
          <span class="health-label">consolidated memory</span>
        </SCard>
      </div>

      <p v-if="!titlesSupported" class="scope-label" role="status">This backend does not provide eligible memory titles. Evidence links are unavailable until the backend is updated.</p>
      <!-- Eligible now -->
      <section aria-labelledby="eligible-heading">
        <SHeading :level="2" id="eligible-heading">Now</SHeading>
        <p v-if="overview.eligible.length === 0" class="empty">
          Nothing needs attention right now.
        </p>
        <SCard
          v-for="item in overview.eligible"
          :key="item.id"
          variant="glass"
          class="attention-row"
        >
          <div class="row-main">
            <div class="row-head">
              <SBadge>{{ reasonLabels[item.reason] ?? item.reason }}</SBadge>
              <SBadge v-if="item.status === 'snoozed'" variant="default"
                >snoozed</SBadge
              >
              <span v-if="item.owner" class="row-owner">{{ item.owner }}</span>
            </div>
            <button v-if="canViewEvidence(item)" class="row-title" @click="viewEvidence(item)">{{ memoryTitle(item) }}</button>
            <span v-else class="row-title unavailable">Evidence unavailable</span>
            <p class="row-why">why: {{ whyNow(item) }}</p>
            <p v-if="item.waiting_on" class="row-why">
              waiting on {{ item.waiting_on }}
            </p>
            <p v-if="item.completion_condition" class="row-why">
              done when: {{ item.completion_condition }}
            </p>
            <p class="row-meta">
              {{ item.namespace }} · captured {{ item.created_at.slice(0, 10) }}
              <template v-if="item.external_system && item.external_ref">
                · {{ item.external_system }}:{{ item.external_ref }}
              </template>
            </p>
          </div>
          <div class="row-actions">
            <template v-if="snoozeFor === item.id">
              <SInput v-model="snoozeUntil" type="date" aria-label="Snooze until" />
              <SButton
                size="sm"
                :disabled="!!busyId"
                @click="confirmSnooze(item)"
                >Snooze</SButton
              >
              <SButton size="sm" variant="ghost" :disabled="!!busyId" @click="snoozeFor = null"
                >Back</SButton
              >
            </template>
            <template v-else>
              <SButton
                size="sm"
                :disabled="!!busyId"
                @click="complete(item)"
                >Complete</SButton
              >
              <SButton
                size="sm"
                variant="ghost"
                :disabled="!!busyId"
                @click="openSnooze(item)"
                >Snooze</SButton
              >
              <SButton
                size="sm"
                variant="ghost"
                :disabled="!!busyId"
                @click="cancel(item)"
                >Cancel</SButton
              >
            </template>
          </div>
        </SCard>
      </section>

      <!-- Everything else open -->
      <section v-if="remainingOpen.length" aria-labelledby="open-heading">
        <SHeading :level="2" id="open-heading">Open</SHeading>
        <SCard
          v-for="item in remainingOpen"
          :key="item.id"
          variant="glass"
          class="attention-row"
        >
          <div class="row-main">
            <div class="row-head">
              <SBadge variant="default">{{ item.status }}</SBadge>
              <span v-if="item.owner" class="row-owner">{{ item.owner }}</span>
            </div>
            <button v-if="canViewEvidence(item)" class="row-title" @click="viewEvidence(item)">{{ memoryTitle(item) }}</button>
            <span v-else class="row-title unavailable">Evidence unavailable</span>
            <p v-if="item.waiting_on" class="row-why">
              waiting on {{ item.waiting_on }}
            </p>
            <p v-if="item.remind_at" class="row-why">
              wakes {{ item.remind_at.slice(0, 10) }}
            </p>
            <p class="row-meta">
              {{ item.namespace }} · captured {{ item.created_at.slice(0, 10) }}
            </p>
          </div>
          <div class="row-actions">
            <SButton
              size="sm"
              :disabled="!!busyId"
              @click="complete(item)"
              >Complete</SButton
            >
            <SButton
              size="sm"
              variant="ghost"
              :disabled="!!busyId"
              @click="cancel(item)"
              >Cancel</SButton
            >
          </div>
        </SCard>
      </section>
    </template>

    <p v-else-if="loading" class="empty">Loading…</p>
  </div>
</template>

<style scoped>
.attention-header { display: flex; justify-content: space-between; align-items: center; gap: 12px; }
.scope-label { font-size: 12px; color: var(--color-text-secondary); margin: 0; }
.capture-diagnostics { padding: 16px; display: flex; flex-direction: column; gap: 12px; }
.capture-diagnostics summary { cursor: pointer; }
.queue-counts { display: grid; grid-template-columns: max-content 1fr; gap: 4px 16px; margin-top: 12px; font-size: 13px; }
.queue-counts dd { margin: 0; }
.operator-guidance { font-size: 13px; color: var(--color-text-secondary); }
.action-error { color: var(--color-danger); font-size: 13px; }
.row-title.unavailable { cursor: default; text-decoration: none; color: var(--color-text-secondary); }

.attention-view {
  display: flex;
  flex-direction: column;
  gap: 20px;
  padding: 24px;
  max-width: 860px;
}

.health-row {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
}

.health-card {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 12px 16px;
  min-width: 140px;
}

.health-value {
  font-size: 20px;
  font-weight: 600;
}

.health-value.warn {
  color: var(--s-colour-danger, #d33);
}

.health-value.muted {
  opacity: 0.5;
}

.health-label {
  font-size: 12px;
  opacity: 0.7;
}

.attention-row {
  display: flex;
  justify-content: space-between;
  gap: 16px;
  padding: 14px 16px;
  margin-top: 10px;
}

.row-main {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}

.row-head {
  display: flex;
  align-items: center;
  gap: 8px;
}

.row-owner {
  font-size: 12px;
  opacity: 0.7;
}

.row-title {
  background: none;
  border: none;
  padding: 0;
  text-align: left;
  font: inherit;
  font-weight: 600;
  color: inherit;
  cursor: pointer;
  text-decoration: underline;
  text-underline-offset: 3px;
}

.row-why {
  font-size: 13px;
  margin: 0;
}

.row-meta {
  font-size: 12px;
  opacity: 0.6;
  margin: 0;
}

.row-actions {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  flex-shrink: 0;
}

.error-card {
  padding: 16px;
}

.empty {
  opacity: 0.7;
}
</style>

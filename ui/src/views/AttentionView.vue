<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { SBadge, SButton, SCard, SHeading, SInput } from "@stuntrocket/ui";
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

async function load() {
  loading.value = true;
  error.value = null;
  try {
    overview.value = await api.attentionOverview(store.selectedNamespace);
  } catch (e) {
    // A remote disconnect stays an error on screen — never a local fallback.
    error.value = String(e);
  } finally {
    loading.value = false;
  }
  try {
    queueHealth.value = await api.captureQueueHealth();
  } catch {
    queueHealth.value = null;
  } finally {
    queueHealthLoaded.value = true;
  }
}

// The row stays on screen until the server confirms its new state.
async function complete(item: AttentionItem) {
  busyId.value = item.id;
  try {
    await api.actionComplete({ id: item.id });
    store.pushToast("Marked as done", "info");
    await load();
  } catch (e) {
    store.pushToast(`Couldn't complete: ${e}`, "error");
  } finally {
    busyId.value = null;
  }
}

function openSnooze(item: AttentionItem) {
  snoozeFor.value = item.id;
  const tomorrow = new Date(Date.now() + 24 * 60 * 60 * 1000);
  snoozeUntil.value = tomorrow.toISOString().slice(0, 10);
}

async function confirmSnooze(item: AttentionItem) {
  if (!snoozeUntil.value) return;
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
    store.pushToast(`Couldn't snooze: ${e}`, "error");
  } finally {
    busyId.value = null;
  }
}

async function cancel(item: AttentionItem) {
  busyId.value = item.id;
  try {
    await api.actionCancel({ id: item.id });
    store.pushToast("Cancelled", "info");
    await load();
  } catch (e) {
    store.pushToast(`Couldn't cancel: ${e}`, "error");
  } finally {
    busyId.value = null;
  }
}

function viewEvidence(item: AttentionItem) {
  router.push({ name: "memory-detail", params: { id: item.memory_id } });
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
</script>

<template>
  <div class="attention-view">
    <SHeading :level="1">Needs attention</SHeading>

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
          <span class="health-label">awaiting review</span>
        </SCard>
        <SCard variant="glass" class="health-card">
          <template v-if="queueHealthLoaded && queueHealth">
            <span
              class="health-value"
              :class="{ warn: queueHealth.dead > 0 }"
              >{{ queueHealth.pending + queueHealth.processing
              }}<template v-if="queueHealth.dead > 0">
                +{{ queueHealth.dead }} failed</template
              ></span
            >
            <span class="health-label">capture queue</span>
          </template>
          <template v-else>
            <span class="health-value muted">—</span>
            <span class="health-label">capture queue unavailable</span>
          </template>
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
            <button class="row-title" @click="viewEvidence(item)">
              View memory {{ item.memory_id.slice(-8) }}
            </button>
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
                :disabled="busyId === item.id"
                @click="confirmSnooze(item)"
                >Snooze</SButton
              >
              <SButton size="sm" variant="ghost" @click="snoozeFor = null"
                >Back</SButton
              >
            </template>
            <template v-else>
              <SButton
                size="sm"
                :disabled="busyId === item.id"
                @click="complete(item)"
                >Complete</SButton
              >
              <SButton
                size="sm"
                variant="ghost"
                :disabled="busyId === item.id"
                @click="openSnooze(item)"
                >Snooze</SButton
              >
              <SButton
                size="sm"
                variant="ghost"
                :disabled="busyId === item.id"
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
            <button class="row-title" @click="viewEvidence(item)">
              View memory {{ item.memory_id.slice(-8) }}
            </button>
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
              :disabled="busyId === item.id"
              @click="complete(item)"
              >Complete</SButton
            >
            <SButton
              size="sm"
              variant="ghost"
              :disabled="busyId === item.id"
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

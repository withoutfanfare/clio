<script setup lang="ts">
import { onMounted } from "vue";
import { SCard, SBadge, STag, SSectionHeader, SHeading } from "@stuntrocket/ui";
import { useMemoryStore } from "@/stores/memories";

const store = useMemoryStore();

onMounted(() => {
  store.loadStats();
  store.loadActivity();
});
</script>

<template>
  <div class="stats-view">
    <SHeading :level="1">Statistics</SHeading>

    <div class="stats-grid" v-if="store.currentStats">
      <SCard variant="glass" class="stat-card">
        <span class="stat-value">{{ store.currentStats.total_memories }}</span>
        <span class="stat-label">Total</span>
      </SCard>
      <SCard variant="glass" class="stat-card">
        <span class="stat-value">{{ store.currentStats.active_memories }}</span>
        <span class="stat-label">Active</span>
      </SCard>
      <SCard variant="glass" class="stat-card">
        <span class="stat-value">{{ store.currentStats.archived_memories }}</span>
        <span class="stat-label">Archived</span>
      </SCard>
      <SCard variant="glass" class="stat-card">
        <span class="stat-value">{{ store.currentStats.embedding_coverage.toFixed(0) }}%</span>
        <span class="stat-label">Embedded</span>
      </SCard>
      <SCard variant="glass" class="stat-card">
        <span class="stat-value">{{ store.currentStats.total_links }}</span>
        <span class="stat-label">Links</span>
      </SCard>
      <SCard variant="glass" class="stat-card">
        <span class="stat-value">{{ store.currentStats.link_density.toFixed(2) }}</span>
        <span class="stat-label">Link Density</span>
      </SCard>
    </div>

    <div class="section" v-if="store.currentStats">
      <SSectionHeader title="By Namespace" />
      <div class="breakdown-list">
        <div
          v-for="[ns, count] in store.currentStats.by_namespace"
          :key="ns"
          class="breakdown-item"
        >
          <span class="breakdown-label">{{ ns }}</span>
          <span class="breakdown-value">{{ count }}</span>
        </div>
      </div>
    </div>

    <div class="section" v-if="store.currentStats">
      <SSectionHeader title="By Kind" />
      <div class="breakdown-list">
        <div
          v-for="[kind, count] in store.currentStats.by_kind"
          :key="kind"
          class="breakdown-item"
        >
          <span class="breakdown-label">{{ kind }}</span>
          <span class="breakdown-value">{{ count }}</span>
        </div>
      </div>
    </div>

    <div class="section" v-if="store.currentStats?.top_tags?.length">
      <SSectionHeader title="Top Tags" />
      <div class="tag-cloud">
        <STag v-for="[tag, count] in store.currentStats.top_tags" :key="tag">
          #{{ tag }} <span class="tag-count">{{ count }}</span>
        </STag>
      </div>
    </div>

    <div class="section" v-if="store.recentActivity.length">
      <SSectionHeader title="Recent Activity" />
      <div class="activity-list">
        <div
          v-for="entry in store.recentActivity"
          :key="entry.memory_id + entry.timestamp"
          class="activity-item"
        >
          <SBadge
            :variant="entry.action === 'created' ? 'success' : entry.action === 'updated' ? 'info' : 'error'"
          >
            {{ entry.action }}
          </SBadge>
          <span class="activity-title">
            {{ entry.title || entry.memory_id.slice(0, 8) }}
          </span>
          <div class="activity-trailing">
            <span class="activity-ns">{{ entry.namespace }}</span>
            <span class="activity-time">
              {{ new Date(entry.timestamp).toLocaleDateString("en-GB") }}
            </span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.stats-view {
  padding-bottom: 48px;
}

/* ── Stats Grid ── */
.stats-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: var(--space-3);
  margin-top: var(--space-6);
  margin-bottom: var(--space-8);
}

.stat-card {
  padding: var(--space-5);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-1);
}

.stat-value {
  font-size: 26px;
  font-weight: 700;
  color: var(--color-accent);
  font-variant-numeric: tabular-nums;
  line-height: 1.3;
}

.stat-label {
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--color-text-tertiary);
  line-height: 1.5;
}

/* ── Sections ── */
.section {
  margin-top: var(--space-8);
}

/* ── Breakdown ── */
.breakdown-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.breakdown-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: var(--space-3) var(--space-4);
  background: var(--colour-surface-card);
  border-radius: var(--radius-md);
}

.breakdown-label {
  font-size: 13px;
  color: var(--color-text-secondary);
  line-height: 1.5;
}

.breakdown-value {
  font-size: 13px;
  font-weight: 600;
  color: var(--color-accent);
  font-variant-numeric: tabular-nums;
}

/* ── Tag Cloud ── */
.tag-cloud {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.tag-count {
  font-size: 11px;
  color: var(--color-text-tertiary);
  font-variant-numeric: tabular-nums;
  margin-left: 2px;
}

/* ── Activity Feed ── */
.activity-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.activity-item {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
  background: var(--colour-surface-card);
  border-radius: var(--radius-md);
}

.activity-title {
  flex: 1;
  font-size: 13px;
  color: var(--color-text-primary);
  line-height: 1.5;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.activity-trailing {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex-shrink: 0;
}

.activity-ns {
  font-size: 11px;
  color: var(--color-text-tertiary);
}

.activity-time {
  font-size: 11px;
  color: var(--color-text-tertiary);
  font-variant-numeric: tabular-nums;
}
</style>

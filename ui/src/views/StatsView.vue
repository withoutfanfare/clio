<script setup lang="ts">
import { onMounted } from "vue";
import { useMemoryStore } from "@/stores/memories";

const store = useMemoryStore();

onMounted(() => {
  store.loadStats();
  store.loadActivity();
});
</script>

<template>
  <div class="stats-view">
    <h1 class="stats-title">Statistics</h1>

    <div class="stats-grid" v-if="store.currentStats">
      <div class="stat-card">
        <span class="stat-value">{{ store.currentStats.total_memories }}</span>
        <span class="stat-label">Total</span>
      </div>
      <div class="stat-card">
        <span class="stat-value">{{ store.currentStats.active_memories }}</span>
        <span class="stat-label">Active</span>
      </div>
      <div class="stat-card">
        <span class="stat-value">{{ store.currentStats.archived_memories }}</span>
        <span class="stat-label">Archived</span>
      </div>
      <div class="stat-card">
        <span class="stat-value">{{ store.currentStats.embedding_coverage.toFixed(0) }}%</span>
        <span class="stat-label">Embedded</span>
      </div>
      <div class="stat-card">
        <span class="stat-value">{{ store.currentStats.total_links }}</span>
        <span class="stat-label">Links</span>
      </div>
      <div class="stat-card">
        <span class="stat-value">{{ store.currentStats.link_density.toFixed(2) }}</span>
        <span class="stat-label">Link Density</span>
      </div>
    </div>

    <div class="section" v-if="store.currentStats">
      <h2 class="section-title">By Namespace</h2>
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
      <h2 class="section-title">By Kind</h2>
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
      <h2 class="section-title">Top Tags</h2>
      <div class="tag-cloud">
        <span
          v-for="[tag, count] in store.currentStats.top_tags"
          :key="tag"
          class="tag-item"
        >
          #{{ tag }} <span class="tag-count">{{ count }}</span>
        </span>
      </div>
    </div>

    <div class="section" v-if="store.recentActivity.length">
      <h2 class="section-title">Recent Activity</h2>
      <div class="activity-list">
        <div
          v-for="entry in store.recentActivity"
          :key="entry.memory_id + entry.timestamp"
          class="activity-item"
        >
          <span class="activity-badge" :class="entry.action">
            {{ entry.action }}
          </span>
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
  padding-bottom: var(--space-12);
}

/* ── Page Title ── */
.stats-title {
  font-size: var(--text-xl);
  font-weight: var(--font-semibold);
  letter-spacing: var(--tracking-tight);
  color: var(--colour-text);
  margin-bottom: var(--space-6);
  line-height: var(--leading-tight);
}

/* ── Stats Grid ── */
.stats-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: var(--space-3);
  margin-bottom: var(--space-8);
}

.stat-card {
  background: var(--colour-surface-card);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-sm);
  padding: var(--space-5);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-1);
}

.stat-value {
  font-size: var(--text-2xl);
  font-weight: var(--font-bold);
  color: var(--colour-accent);
  font-variant-numeric: tabular-nums;
  line-height: var(--leading-tight);
}

.stat-label {
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  text-transform: uppercase;
  letter-spacing: var(--tracking-caps);
  color: var(--colour-text-disabled);
  line-height: var(--leading-normal);
}

/* ── Sections ── */
.section {
  margin-top: var(--space-8);
}

.section-title {
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  text-transform: uppercase;
  letter-spacing: var(--tracking-caps);
  color: var(--colour-text-muted);
  margin-bottom: var(--space-3);
  line-height: var(--leading-normal);
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
  font-size: var(--text-sm);
  color: var(--colour-text-secondary);
  line-height: var(--leading-normal);
}

.breakdown-value {
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--colour-accent);
  font-variant-numeric: tabular-nums;
}

/* ── Tag Cloud ── */
.tag-cloud {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.tag-item {
  padding: var(--space-1) var(--space-3);
  border-radius: 99px;
  font-size: var(--text-sm);
  color: var(--colour-accent);
  background: var(--colour-accent-muted);
  line-height: var(--leading-normal);
}

.tag-count {
  font-size: var(--text-xs);
  color: var(--colour-text-disabled);
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

.activity-badge {
  font-size: var(--text-xs);
  padding: 2px var(--space-2);
  border-radius: var(--radius-sm);
  text-transform: uppercase;
  font-weight: var(--font-semibold);
  letter-spacing: var(--tracking-caps);
  flex-shrink: 0;
  line-height: var(--leading-normal);
}

.activity-badge.created {
  background: color-mix(in srgb, var(--colour-success) 15%, transparent);
  color: var(--colour-success);
}

.activity-badge.updated {
  background: color-mix(in srgb, var(--colour-info) 15%, transparent);
  color: var(--colour-info);
}

.activity-badge.archived {
  background: color-mix(in srgb, var(--colour-danger) 15%, transparent);
  color: var(--colour-danger);
}

.activity-title {
  flex: 1;
  font-size: var(--text-sm);
  color: var(--colour-text);
  line-height: var(--leading-normal);
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
  font-size: var(--text-xs);
  color: var(--colour-text-muted);
}

.activity-time {
  font-size: var(--text-xs);
  color: var(--colour-text-muted);
  font-variant-numeric: tabular-nums;
}
</style>

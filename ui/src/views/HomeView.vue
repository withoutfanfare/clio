<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, toRef, watch, type Ref } from "vue";
import { useRoute } from "vue-router";
import { SButton, SSelect, SFormField, SEmptyState, SSpinner, SBadge, SKbd } from "@stuntrocket/ui";
import ComposeArea from "@/components/ComposeArea.vue";
import DateGroup from "@/components/DateGroup.vue";
import MemoryPage from "@/components/MemoryPage.vue";
import { useMemoryStore } from "@/stores/memories";
import { useGroupedMemories } from "@/composables/useGroupedMemories";
import { useNamespaceColours } from "@/composables/useNamespaceColours";
import type { GroupBy } from "@/composables/useGroupedMemories";

const store = useMemoryStore();
const route = useRoute();
const { getColour } = useNamespaceColours();
const groupByRef = toRef(store, "groupBy") as Ref<GroupBy>;
const groups = useGroupedMemories(toRef(store, "unpinnedItems"), groupByRef);

const pinnedCollapsed = ref(false);

const filtersOpen = ref(false);
const tagInput = ref("");
const tagDropdownOpen = ref(false);
const tagInputEl = ref<HTMLInputElement | null>(null);

const availableTags = computed(() => {
  const stats = store.currentStats;
  if (!stats?.top_tags?.length) return [];
  return stats.top_tags.map(([tag]) => tag);
});

const filteredTagSuggestions = computed(() => {
  const query = tagInput.value.trim().toLowerCase();
  const active = new Set(store.filterTags);
  const tags = availableTags.value.filter((t) => !active.has(t));
  if (!query) return tags.slice(0, 20);
  return tags.filter((t) => t.includes(query)).slice(0, 20);
});

const kinds = ["note", "observation", "decision", "preference", "snippet", "knowledgebase"];
const sortOptions = [
  { value: "importance_desc", label: "Most important" },
  { value: "importance_asc", label: "Least important" },
  { value: "created_desc", label: "Newest created" },
  { value: "created_asc", label: "Oldest created" },
  { value: "updated_desc", label: "Recently updated" },
  { value: "updated_asc", label: "Oldest updated" },
];
const groupOptions = [
  { value: "importance", label: "Importance" },
  { value: "date", label: "Date" },
  { value: "kind", label: "Kind" },
  { value: "none", label: "None" },
];

function addTag() {
  const tag = tagInput.value.trim().toLowerCase();
  if (tag && !store.filterTags.includes(tag)) {
    store.setFilterTags([...store.filterTags, tag]);
  }
  tagInput.value = "";
  tagDropdownOpen.value = false;
}

function selectTag(tag: string) {
  if (!store.filterTags.includes(tag)) {
    store.setFilterTags([...store.filterTags, tag]);
  }
  tagInput.value = "";
  tagDropdownOpen.value = false;
}

function removeTag(tag: string) {
  store.setFilterTags(store.filterTags.filter((t) => t !== tag));
}

function onTagInputFocus() {
  tagDropdownOpen.value = true;
}

function onTagInputBlur() {
  // Delay to allow click on dropdown items.
  setTimeout(() => { tagDropdownOpen.value = false; }, 150);
}

function onVisibilityChange() {
  if (document.hidden) {
    store.pausePolling();
  } else {
    store.resumePolling(3000);
  }
}

function handleSortChange(value: string) {
  store.setSortBy(value);
}

function handleGroupChange(value: string) {
  store.setGroupBy(value);
}

function handleKindChange(value: string) {
  store.setFilterKind(value || null);
}

/** Namespace counts derived from stats. */
const namespaceCounts = computed(() => {
  const map = new Map<string, number>();
  if (store.currentStats?.by_namespace) {
    for (const [ns, count] of store.currentStats.by_namespace) {
      map.set(ns, count);
    }
  }
  return map;
});

function handleNamespaceSwitch(value: string) {
  store.setNamespace(value || null);
  store.loadRecent();
}

onMounted(() => {
  store.loadRecent();
  store.loadStats();
  store.startPolling(3000);
  document.addEventListener("visibilitychange", onVisibilityChange);
});

onUnmounted(() => {
  store.stopPolling();
  document.removeEventListener("visibilitychange", onVisibilityChange);
});

watch(
  () => route.params.id,
  (id) => {
    if (id && typeof id === "string") {
      store.openDrawer(id);
    }
  },
  { immediate: true },
);
</script>

<template>
  <div class="home-view">
    <ComposeArea />

    <div class="river-header" v-if="!store.loading">
      <div class="river-info">
        <span class="river-count">
          {{ store.total }} {{ store.total === 1 ? "memory" : "memories" }}
        </span>
        <div class="ns-switcher">
          <select
            class="ns-select"
            :value="store.selectedNamespace ?? ''"
            @change="handleNamespaceSwitch(($event.target as HTMLSelectElement).value)"
          >
            <option value="">All namespaces</option>
            <option
              v-for="ns in store.allNamespaces"
              :key="ns"
              :value="ns"
            >
              {{ ns }} ({{ namespaceCounts.get(ns) ?? 0 }})
            </option>
          </select>
        </div>
      </div>
      <div class="header-controls">
        <SButton
          variant="icon"
          size="sm"
          :class="{ active: filtersOpen, 'has-filters': store.hasActiveFilters }"
          @click="filtersOpen = !filtersOpen"
          title="Filter &amp; sort"
          aria-label="Filter and sort"
        >
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
            <path d="M2 3h12M4 8h8M6 13h4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
          <span v-if="store.hasActiveFilters" class="filter-badge" />
        </SButton>
        <div class="view-toggle">
          <SButton
            variant="icon"
            size="sm"
            :class="{ active: store.viewMode === 'list' }"
            @click="store.setViewMode('list')"
            title="List view"
            aria-label="List view"
          >
            <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
              <path d="M2 4h12M2 8h12M2 12h12" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
            </svg>
          </SButton>
          <SButton
            variant="icon"
            size="sm"
            :class="{ active: store.viewMode === 'grid' }"
            @click="store.setViewMode('grid')"
            title="Grid view"
            aria-label="Grid view"
          >
            <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
              <rect x="2" y="2" width="5" height="5" rx="1" stroke="currentColor" stroke-width="1.3"/>
              <rect x="9" y="2" width="5" height="5" rx="1" stroke="currentColor" stroke-width="1.3"/>
              <rect x="2" y="9" width="5" height="5" rx="1" stroke="currentColor" stroke-width="1.3"/>
              <rect x="9" y="9" width="5" height="5" rx="1" stroke="currentColor" stroke-width="1.3"/>
            </svg>
          </SButton>
        </div>
      </div>
    </div>

    <!-- Filter bar -->
    <Transition name="slide-down">
      <div v-if="filtersOpen" class="filter-bar">
        <div class="filter-row">
          <!-- Sort -->
          <SFormField label="Sort">
            <SSelect
              :model-value="store.sortBy"
              size="sm"
              @update:model-value="handleSortChange"
            >
              <option v-for="opt in sortOptions" :key="opt.value" :value="opt.value">
                {{ opt.label }}
              </option>
            </SSelect>
          </SFormField>

          <!-- Group -->
          <SFormField label="Group">
            <SSelect
              :model-value="store.groupBy"
              size="sm"
              @update:model-value="handleGroupChange"
            >
              <option v-for="opt in groupOptions" :key="opt.value" :value="opt.value">
                {{ opt.label }}
              </option>
            </SSelect>
          </SFormField>

          <!-- Kind -->
          <SFormField label="Kind">
            <SSelect
              :model-value="store.filterKind ?? ''"
              size="sm"
              @update:model-value="handleKindChange"
            >
              <option value="">All</option>
              <option v-for="k in kinds" :key="k" :value="k">{{ k }}</option>
            </SSelect>
          </SFormField>

          <!-- Importance -->
          <SFormField label="Importance">
            <div class="importance-pills">
              <button
                v-for="n in 5"
                :key="n"
                class="imp-pill"
                :class="{
                  active:
                    store.filterImportanceMin !== null &&
                    store.filterImportanceMax !== null &&
                    n >= store.filterImportanceMin &&
                    n <= store.filterImportanceMax,
                  solo:
                    store.filterImportanceMin === n && store.filterImportanceMax === n,
                }"
                @click="
                  store.filterImportanceMin === n && store.filterImportanceMax === n
                    ? store.setFilterImportance(null, null)
                    : store.setFilterImportance(n, n)
                "
                :title="`Importance ${n}`"
              >
                {{ n }}
              </button>
            </div>
          </SFormField>

          <!-- Tags -->
          <SFormField label="Tags" class="filter-group-tags">
            <div class="tag-input-wrapper">
              <input
                ref="tagInputEl"
                v-model="tagInput"
                class="filter-input"
                placeholder="Filter by tag..."
                @keydown.enter.prevent="addTag"
                @focus="onTagInputFocus"
                @blur="onTagInputBlur"
              />
              <div v-if="tagDropdownOpen && filteredTagSuggestions.length" class="tag-dropdown">
                <button
                  v-for="tag in filteredTagSuggestions"
                  :key="tag"
                  class="tag-option"
                  @mousedown.prevent="selectTag(tag)"
                >
                  <span class="tag-option-hash">#</span>{{ tag }}
                </button>
              </div>
            </div>
            <div class="active-tags" v-if="store.filterTags.length">
              <SBadge v-for="tag in store.filterTags" :key="tag" variant="accent">
                #{{ tag }}
                <button class="tag-remove" @click="removeTag(tag)" aria-label="Remove tag">&times;</button>
              </SBadge>
            </div>
          </SFormField>
        </div>

        <SButton
          v-if="store.hasActiveFilters"
          variant="ghost"
          size="sm"
          @click="store.clearFilters()"
          class="clear-filters-btn"
        >
          Clear filters
        </SButton>
      </div>
    </Transition>

    <div v-if="store.loading" class="river-loading">
      <SSpinner size="md" />
    </div>

    <template v-else>
      <!-- Pinned section -->
      <div v-if="store.pinnedItems.length" class="pinned-section">
        <button class="pinned-header" @click="pinnedCollapsed = !pinnedCollapsed">
          <svg
            width="10" height="10" viewBox="0 0 12 12" fill="none"
            class="pinned-chevron"
            :class="{ open: !pinnedCollapsed }"
          >
            <path d="M4 2l4 4-4 4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
          </svg>
          <svg width="12" height="12" viewBox="0 0 16 16" fill="none" class="pin-icon">
            <path d="M9.828 1.172a1 1 0 011.414 0l3.586 3.586a1 1 0 010 1.414L12 9l-1 4-4.5-1.5L3 15l.5-3.5L2 7l3-2.828 4.828-3z" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" fill="none"/>
          </svg>
          <span class="pinned-label">Pinned</span>
          <span class="pinned-count">{{ store.pinnedItems.length }}</span>
        </button>
        <Transition name="slide-down">
          <div v-if="!pinnedCollapsed" class="pinned-list" :class="store.viewMode === 'grid' ? 'pinned-grid' : ''">
            <MemoryPage
              v-for="item in store.pinnedItems"
              :key="item.id"
              :memory="item"
              :mode="store.viewMode"
              :focused="store.navigableItems.indexOf(item) === store.focusedIndex"
            />
          </div>
        </Transition>
      </div>

      <div class="river">
        <DateGroup
          v-for="group in groups"
          :key="group.label"
          :label="group.label"
          :mode="store.viewMode"
        >
          <MemoryPage
            v-for="item in group.items"
            :key="item.id"
            :memory="item"
            :mode="store.viewMode"
            :focused="store.navigableItems.indexOf(item) === store.focusedIndex"
          />
        </DateGroup>
      </div>
    </template>

    <div v-if="!store.loading && !store.items.length" class="river-empty">
      <template v-if="store.hasActiveFilters">
        <SEmptyState
          title="No memories match filters"
          description="Try adjusting your filters to see more results"
        >
          <template #action>
            <SButton variant="ghost" size="sm" @click="store.clearFilters()">Clear filters</SButton>
          </template>
        </SEmptyState>
      </template>
      <template v-else>
        <SEmptyState
          title="No memories yet"
        >
          <template #action>
            <span class="empty-hint">Press <SKbd>&#8984;N</SKbd> to create your first one</span>
          </template>
        </SEmptyState>
      </template>
    </div>

    <div v-if="store.error" class="river-error">{{ store.error }}</div>
  </div>
</template>

<style scoped>
.home-view {
  display: flex;
  flex-direction: column;
  padding-bottom: 64px;
  min-height: 0;
}

.river-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  margin-bottom: var(--space-4);
}

.river-info {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  font-weight: 400;
  color: var(--color-text-tertiary);
}

.river-count {
  font-variant-numeric: tabular-nums;
}

.river-namespace {
  color: var(--color-text-secondary);
}

.river-namespace strong {
  color: var(--color-accent);
  font-weight: 500;
}

/* ── Namespace Quick-Switch ── */
.ns-switcher {
  position: relative;
}

.ns-select {
  appearance: none;
  background: var(--color-surface-hover);
  border: 1px solid var(--glass-border);
  border-radius: var(--radius-sm);
  padding: 3px 24px 3px 8px;
  font-size: 12px;
  color: var(--color-text-secondary);
  cursor: pointer;
  transition: border-color 150ms;
  background-image: url("data:image/svg+xml,%3Csvg width='10' height='6' viewBox='0 0 10 6' fill='none' xmlns='http://www.w3.org/2000/svg'%3E%3Cpath d='M1 1l4 4 4-4' stroke='%239CA3AF' stroke-width='1.5' stroke-linecap='round' stroke-linejoin='round'/%3E%3C/svg%3E");
  background-repeat: no-repeat;
  background-position: right 6px center;
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.ns-select:hover {
  border-color: var(--color-accent);
}

.ns-select:focus {
  outline: 2px solid color-mix(in srgb, var(--color-accent) 55%, transparent);
  outline-offset: 1px;
}

/* ── Header Controls ── */
.header-controls {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.filter-badge {
  position: absolute;
  top: 3px;
  right: 3px;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--color-accent);
}

/* ── View Toggle ── */
.view-toggle {
  display: flex;
  background: var(--color-surface-hover);
  border-radius: var(--radius-md);
  padding: 2px;
  gap: 1px;
}

/* ── Filter Bar ── */
.filter-bar {
  background: var(--color-surface-hover);
  border: 1px solid var(--glass-border);
  border-radius: var(--radius-lg);
  padding: var(--space-3) var(--space-4);
  margin-bottom: var(--space-4);
}

.filter-row {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-4);
  align-items: flex-start;
}

.filter-group-tags {
  flex: 1;
  min-width: 120px;
}

.filter-input {
  background: var(--colour-surface-input);
  border: 1px solid var(--glass-border);
  border-radius: var(--radius-sm);
  padding: 4px 8px;
  font-size: 13px;
  color: var(--color-text-primary);
  width: 100%;
  min-width: 80px;
}

.filter-input::placeholder {
  color: var(--color-text-tertiary);
}

.filter-input:focus {
  outline: 2px solid color-mix(in srgb, var(--color-accent) 55%, transparent);
  outline-offset: 1px;
}

.tag-input-wrapper {
  position: relative;
}

.tag-dropdown {
  position: absolute;
  top: 100%;
  left: 0;
  right: 0;
  z-index: 10;
  margin-top: 2px;
  max-height: 180px;
  overflow-y: auto;
  background: var(--colour-surface-dropdown);
  border: 1px solid var(--glass-border);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-sheet);
  padding: 4px;
}

.tag-option {
  display: flex;
  align-items: center;
  gap: 2px;
  width: 100%;
  padding: 4px 8px;
  background: transparent;
  border: none;
  border-radius: var(--radius-sm);
  color: var(--color-text-secondary);
  font-size: 13px;
  cursor: pointer;
  text-align: left;
}

.tag-option:hover {
  background: var(--color-surface-hover);
  color: var(--color-text-primary);
}

.tag-option-hash {
  color: var(--color-accent);
  font-weight: 500;
}

.active-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-top: 4px;
}

.tag-remove {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 14px;
  height: 14px;
  padding: 0;
  background: transparent;
  border: none;
  color: var(--color-accent);
  cursor: pointer;
  font-size: 12px;
  line-height: 1;
  border-radius: 50%;
}

.tag-remove:hover {
  background: var(--color-accent-subtle);
}

/* ── Importance Pills ── */
.importance-pills {
  display: flex;
  gap: 2px;
}

.imp-pill {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  padding: 0;
  background: var(--colour-surface-input);
  border: 1px solid var(--glass-border);
  border-radius: var(--radius-sm);
  color: var(--color-text-tertiary);
  font-size: 11px;
  font-weight: 600;
  cursor: pointer;
  transition: all 120ms;
}

.imp-pill:hover {
  color: var(--color-text-secondary);
  border-color: var(--glass-border-hover);
}

.imp-pill.active,
.imp-pill.solo {
  color: var(--color-text-primary);
  border-color: var(--color-accent);
  background: var(--color-accent-subtle);
}

.clear-filters-btn {
  margin-top: var(--space-3);
}

/* ── Filter Transition ── */
.slide-down-enter-active {
  transition: all 150ms ease-out;
}
.slide-down-leave-active {
  transition: all 100ms ease-in;
}
.slide-down-enter-from,
.slide-down-leave-to {
  opacity: 0;
  transform: translateY(-8px);
}

/* ── Loading ── */
.river-loading {
  padding: 64px 0;
  display: flex;
  justify-content: center;
}

/* ── Pinned Section ── */
.pinned-section {
  margin-bottom: var(--space-4);
}

.pinned-header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  background: none;
  border: none;
  color: var(--color-text-tertiary);
  cursor: pointer;
  transition: color 150ms;
  padding: var(--space-1) 0;
  margin-bottom: var(--space-3);
}

.pinned-header:hover {
  color: var(--color-text-primary);
}

.pinned-chevron {
  transition: transform 200ms cubic-bezier(0.4, 0, 0.2, 1);
}

.pinned-chevron.open {
  transform: rotate(90deg);
}

.pin-icon {
  color: var(--color-accent);
}

.pinned-label {
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
}

.pinned-count {
  font-size: 11px;
  color: var(--color-text-tertiary);
  font-variant-numeric: tabular-nums;
}

.pinned-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.pinned-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: var(--space-3);
}

.river {
  /* Cards flow naturally; .content-area is the scroll container.
     No nested scroll/height cap here, so the list fills to the bottom. */
  display: flex;
  flex-direction: column;
}

.river-empty {
  padding: 80px 0;
  text-align: center;
}

.empty-hint {
  font-size: 13px;
  color: var(--color-text-tertiary);
}

.river-error {
  padding: var(--space-3) var(--space-4);
  background: var(--color-danger-subtle);
  border: 1px solid color-mix(in srgb, var(--color-danger) 20%, transparent);
  border-radius: var(--radius-md);
  color: var(--color-danger);
  font-size: 13px;
  margin-top: var(--space-4);
}
</style>

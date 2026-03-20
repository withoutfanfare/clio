<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, toRef, watch, type Ref } from "vue";
import { useRoute } from "vue-router";
import ComposeArea from "@/components/ComposeArea.vue";
import DateGroup from "@/components/DateGroup.vue";
import MemoryPage from "@/components/MemoryPage.vue";
import { useMemoryStore } from "@/stores/memories";
import { useGroupedMemories } from "@/composables/useGroupedMemories";
import type { GroupBy } from "@/composables/useGroupedMemories";

const store = useMemoryStore();
const route = useRoute();
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
        <span v-if="store.activeNamespace" class="river-namespace">
          in <strong>{{ store.activeNamespace }}</strong>
        </span>
      </div>
      <div class="header-controls">
        <button
          class="control-btn"
          :class="{ active: filtersOpen, 'has-filters': store.hasActiveFilters }"
          @click="filtersOpen = !filtersOpen"
          title="Filter &amp; sort"
          aria-label="Filter and sort"
        >
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
            <path d="M2 3h12M4 8h8M6 13h4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
          <span v-if="store.hasActiveFilters" class="filter-badge" />
        </button>
        <div class="view-toggle">
          <button
            class="toggle-btn"
            :class="{ active: store.viewMode === 'list' }"
            @click="store.setViewMode('list')"
            title="List view"
            aria-label="List view"
          >
            <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
              <path d="M2 4h12M2 8h12M2 12h12" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
            </svg>
          </button>
          <button
            class="toggle-btn"
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
          </button>
        </div>
      </div>
    </div>

    <!-- Filter bar -->
    <Transition name="slide-down">
      <div v-if="filtersOpen" class="filter-bar">
        <div class="filter-row">
          <!-- Sort -->
          <div class="filter-group">
            <label class="filter-label">Sort</label>
            <select
              class="filter-select"
              :value="store.sortBy"
              @change="store.setSortBy(($event.target as HTMLSelectElement).value)"
            >
              <option v-for="opt in sortOptions" :key="opt.value" :value="opt.value">
                {{ opt.label }}
              </option>
            </select>
          </div>

          <!-- Group -->
          <div class="filter-group">
            <label class="filter-label">Group</label>
            <select
              class="filter-select"
              :value="store.groupBy"
              @change="store.setGroupBy(($event.target as HTMLSelectElement).value)"
            >
              <option v-for="opt in groupOptions" :key="opt.value" :value="opt.value">
                {{ opt.label }}
              </option>
            </select>
          </div>

          <!-- Kind -->
          <div class="filter-group">
            <label class="filter-label">Kind</label>
            <select
              class="filter-select"
              :value="store.filterKind ?? ''"
              @change="store.setFilterKind(($event.target as HTMLSelectElement).value || null)"
            >
              <option value="">All</option>
              <option v-for="k in kinds" :key="k" :value="k">{{ k }}</option>
            </select>
          </div>

          <!-- Importance -->
          <div class="filter-group">
            <label class="filter-label">Importance</label>
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
          </div>

          <!-- Tags -->
          <div class="filter-group filter-group-tags">
            <label class="filter-label">Tags</label>
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
              <span v-for="tag in store.filterTags" :key="tag" class="active-tag">
                #{{ tag }}
                <button class="tag-remove" @click="removeTag(tag)" aria-label="Remove tag">&times;</button>
              </span>
            </div>
          </div>
        </div>

        <button
          v-if="store.hasActiveFilters"
          class="clear-filters-btn"
          @click="store.clearFilters()"
        >
          Clear filters
        </button>
      </div>
    </Transition>

    <div v-if="store.loading" class="river-loading">
      <div class="loading-dots">
        <span /><span /><span />
      </div>
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
              :focused="store.items.indexOf(item) === store.focusedIndex"
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
            :focused="store.items.indexOf(item) === store.focusedIndex"
          />
        </DateGroup>
      </div>
    </template>

    <div v-if="!store.loading && !store.items.length" class="river-empty">
      <template v-if="store.hasActiveFilters">
        <p class="empty-title">No memories match filters</p>
        <p class="empty-hint">
          <button class="clear-link" @click="store.clearFilters()">Clear filters</button>
          to see all memories
        </p>
      </template>
      <template v-else>
        <p class="empty-title">No memories yet</p>
        <p class="empty-hint">Press <kbd>&#8984;N</kbd> to create your first one</p>
      </template>
    </div>

    <div v-if="store.error" class="river-error">{{ store.error }}</div>
  </div>
</template>

<style scoped>
.home-view {
  padding-bottom: 64px;
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
  font-size: var(--text-sm);
  font-weight: var(--font-normal);
  color: var(--colour-text-muted);
}

.river-count {
  font-variant-numeric: tabular-nums;
}

.river-namespace {
  color: var(--colour-text-secondary);
}

.river-namespace strong {
  color: var(--colour-accent);
  font-weight: var(--font-medium);
}

/* ── Header Controls ── */
.header-controls {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.control-btn {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 26px;
  padding: 0;
  background: transparent;
  border: none;
  border-radius: var(--radius-sm);
  color: var(--colour-text-disabled);
  cursor: pointer;
  transition: color 150ms, background 150ms;
}

.control-btn:hover {
  color: var(--colour-text-muted);
}

.control-btn.active {
  color: var(--colour-text);
}

.filter-badge {
  position: absolute;
  top: 3px;
  right: 3px;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--colour-accent);
}

/* ── View Toggle ── */
.view-toggle {
  display: flex;
  background: var(--colour-surface-overlay);
  border-radius: var(--radius-md);
  padding: 2px;
  gap: 1px;
}

.toggle-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 26px;
  padding: 0;
  background: transparent;
  border: none;
  border-radius: var(--radius-sm);
  color: var(--colour-text-disabled);
  cursor: pointer;
  transition: color 150ms, background 150ms;
}

.toggle-btn:hover {
  color: var(--colour-text-muted);
}

.toggle-btn.active {
  color: var(--colour-text);
  background: var(--colour-surface-overlay);
  box-shadow: var(--shadow-sm);
}

/* ── Filter Bar ── */
.filter-bar {
  background: var(--colour-surface-overlay);
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

.filter-group {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}

.filter-group-tags {
  flex: 1;
  min-width: 120px;
}

.filter-label {
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  text-transform: uppercase;
  letter-spacing: var(--tracking-caps);
  color: var(--colour-text-disabled);
}

.filter-select {
  appearance: none;
  background: var(--colour-surface-input);
  border: 1px solid var(--glass-border);
  border-radius: var(--radius-sm);
  padding: 4px 24px 4px 8px;
  font-size: var(--text-sm);
  color: var(--colour-text);
  cursor: pointer;
  background-image: url("data:image/svg+xml,%3Csvg width='10' height='6' viewBox='0 0 10 6' fill='none' xmlns='http://www.w3.org/2000/svg'%3E%3Cpath d='M1 1l4 4 4-4' stroke='%2378736e' stroke-width='1.5' stroke-linecap='round' stroke-linejoin='round'/%3E%3C/svg%3E");
  background-repeat: no-repeat;
  background-position: right 8px center;
}

.filter-select:focus {
  outline: 2px solid var(--colour-border-focus);
  outline-offset: 1px;
}

.filter-input {
  background: var(--colour-surface-input);
  border: 1px solid var(--glass-border);
  border-radius: var(--radius-sm);
  padding: 4px 8px;
  font-size: var(--text-sm);
  color: var(--colour-text);
  width: 100%;
  min-width: 80px;
}

.filter-input::placeholder {
  color: var(--colour-text-disabled);
}

.filter-input:focus {
  outline: 2px solid var(--colour-border-focus);
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
  box-shadow: var(--shadow-overlay);
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
  color: var(--colour-text-secondary);
  font-size: var(--text-sm);
  cursor: pointer;
  text-align: left;
}

.tag-option:hover {
  background: var(--colour-surface-overlay);
  color: var(--colour-text);
}

.tag-option-hash {
  color: var(--colour-accent);
  font-weight: var(--font-medium);
}

.active-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-top: 4px;
}

.active-tag {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  padding: 1px 6px;
  border-radius: 99px;
  background: var(--colour-accent-muted);
  color: var(--colour-accent);
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
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
  color: var(--colour-accent);
  cursor: pointer;
  font-size: 12px;
  line-height: 1;
  border-radius: 50%;
}

.tag-remove:hover {
  background: var(--colour-accent-muted);
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
  color: var(--colour-text-disabled);
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  cursor: pointer;
  transition: all 120ms;
}

.imp-pill:hover {
  color: var(--colour-text-muted);
  border-color: var(--glass-border-hover);
}

.imp-pill.active,
.imp-pill.solo {
  color: var(--colour-text);
  border-color: var(--colour-accent);
  background: var(--colour-accent-muted);
}

.clear-filters-btn {
  display: inline-flex;
  align-items: center;
  margin-top: var(--space-3);
  padding: 4px 10px;
  background: transparent;
  border: 1px solid var(--glass-border);
  border-radius: var(--radius-sm);
  color: var(--colour-text-muted);
  font-size: var(--text-xs);
  cursor: pointer;
  transition: color 120ms, border-color 120ms;
}

.clear-filters-btn:hover {
  color: var(--colour-text);
  border-color: var(--glass-border-hover);
}

.clear-link {
  background: none;
  border: none;
  color: var(--colour-accent);
  cursor: pointer;
  font-size: inherit;
  text-decoration: underline;
  padding: 0;
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

.loading-dots {
  display: flex;
  gap: 5px;
}

.loading-dots span {
  width: 4px;
  height: 4px;
  border-radius: 50%;
  background: var(--colour-text-disabled);
  animation: loading-pulse 1.2s ease-in-out infinite;
}

.loading-dots span:nth-child(2) {
  animation-delay: 0.15s;
}

.loading-dots span:nth-child(3) {
  animation-delay: 0.3s;
}

@keyframes loading-pulse {
  0%, 60%, 100% { opacity: 0.3; }
  30% { opacity: 0.8; }
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
  color: var(--colour-text-muted);
  cursor: pointer;
  transition: color 150ms;
  padding: var(--space-1) 0;
  margin-bottom: var(--space-3);
}

.pinned-header:hover {
  color: var(--colour-text);
}

.pinned-chevron {
  transition: transform 200ms cubic-bezier(0.4, 0, 0.2, 1);
}

.pinned-chevron.open {
  transform: rotate(90deg);
}

.pin-icon {
  color: var(--colour-accent);
}

.pinned-label {
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  text-transform: uppercase;
  letter-spacing: var(--tracking-caps);
}

.pinned-count {
  font-size: var(--text-xs);
  color: var(--colour-text-disabled);
  font-variant-numeric: tabular-nums;
}

.pinned-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.pinned-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: var(--space-2);
}

.river {
  display: flex;
  flex-direction: column;
}

.river-empty {
  padding: 80px 0;
  text-align: center;
}

.empty-title {
  font-size: var(--text-lg);
  color: var(--colour-text-secondary);
  margin-bottom: var(--space-2);
}

.empty-hint {
  font-size: var(--text-sm);
  color: var(--colour-text-disabled);
}

.empty-hint kbd {
  font-size: var(--text-xs);
  padding: 2px 6px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--colour-border);
  color: var(--colour-text-muted);
  font-family: inherit;
}

.river-error {
  padding: var(--space-3) var(--space-4);
  background: color-mix(in srgb, var(--colour-danger) 8%, transparent);
  border: 1px solid color-mix(in srgb, var(--colour-danger) 20%, transparent);
  border-radius: var(--radius-md);
  color: var(--colour-danger);
  font-size: var(--text-sm);
  margin-top: var(--space-4);
}

</style>

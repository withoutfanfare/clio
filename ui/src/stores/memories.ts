import { defineStore } from "pinia";
import { ref, computed } from "vue";
import * as api from "@/api/memory";
import type {
  Memory,
  RecallItem,
  RecallResult,
  MemoryStats,
  RecentEntry,
} from "@/api/types";

export const useMemoryStore = defineStore("memories", () => {
  // Memory list
  const items = ref<RecallItem[]>([]);
  const total = ref(0);
  const loading = ref(false);
  const error = ref<string | null>(null);

  // Namespaces
  const selectedNamespace = ref<string | null>(null);
  const allNamespaces = ref<string[]>([]);

  // Stats
  const currentStats = ref<MemoryStats | null>(null);
  const recentActivity = ref<RecentEntry[]>([]);

  // Drawer state
  const drawerMemory = ref<Memory | null>(null);
  const drawerOpen = ref(false);

  // Filters (persisted to localStorage)
  const filterKind = ref<string | null>(
    localStorage.getItem("clio-filter-kind") || null,
  );
  const filterImportanceMin = ref<number | null>(
    localStorage.getItem("clio-filter-imp-min")
      ? Number(localStorage.getItem("clio-filter-imp-min"))
      : null,
  );
  const filterImportanceMax = ref<number | null>(
    localStorage.getItem("clio-filter-imp-max")
      ? Number(localStorage.getItem("clio-filter-imp-max"))
      : null,
  );
  const filterTags = ref<string[]>(
    localStorage.getItem("clio-filter-tags")
      ? JSON.parse(localStorage.getItem("clio-filter-tags")!)
      : [],
  );
  const sortBy = ref<string>(
    localStorage.getItem("clio-sort-by") || "importance_desc",
  );
  const groupBy = ref<string>(
    localStorage.getItem("clio-group-by") || "importance",
  );

  const hasActiveFilters = computed(
    () =>
      filterKind.value !== null ||
      filterImportanceMin.value !== null ||
      filterImportanceMax.value !== null ||
      filterTags.value.length > 0,
  );

  function setFilterKind(kind: string | null) {
    filterKind.value = kind;
    if (kind) localStorage.setItem("clio-filter-kind", kind);
    else localStorage.removeItem("clio-filter-kind");
    loadRecent(true);
  }

  function setFilterImportance(min: number | null, max: number | null) {
    filterImportanceMin.value = min;
    filterImportanceMax.value = max;
    if (min !== null) localStorage.setItem("clio-filter-imp-min", String(min));
    else localStorage.removeItem("clio-filter-imp-min");
    if (max !== null) localStorage.setItem("clio-filter-imp-max", String(max));
    else localStorage.removeItem("clio-filter-imp-max");
    loadRecent(true);
  }

  function setFilterTags(tags: string[]) {
    filterTags.value = tags;
    if (tags.length) localStorage.setItem("clio-filter-tags", JSON.stringify(tags));
    else localStorage.removeItem("clio-filter-tags");
    loadRecent(true);
  }

  function setSortBy(sort: string) {
    sortBy.value = sort;
    localStorage.setItem("clio-sort-by", sort);
    loadRecent(true);
  }

  function setGroupBy(group: string) {
    groupBy.value = group;
    localStorage.setItem("clio-group-by", group);
  }

  function clearFilters() {
    filterKind.value = null;
    filterImportanceMin.value = null;
    filterImportanceMax.value = null;
    filterTags.value = [];
    sortBy.value = "importance_desc";
    groupBy.value = "importance";
    localStorage.removeItem("clio-filter-kind");
    localStorage.removeItem("clio-filter-imp-min");
    localStorage.removeItem("clio-filter-imp-max");
    localStorage.removeItem("clio-filter-tags");
    localStorage.setItem("clio-sort-by", "importance_desc");
    localStorage.setItem("clio-group-by", "importance");
    loadRecent(true);
  }

  // View mode (persisted to localStorage)
  const viewMode = ref<"list" | "grid">(
    (localStorage.getItem("clio-view-mode") as "list" | "grid") || "list",
  );

  function setViewMode(mode: "list" | "grid") {
    viewMode.value = mode;
    localStorage.setItem("clio-view-mode", mode);
  }

  // Pinned memories (persisted to localStorage, max 25)
  const MAX_PINS = 25;
  const pinnedIds = ref<string[]>(
    localStorage.getItem("clio-pinned-ids")
      ? JSON.parse(localStorage.getItem("clio-pinned-ids")!)
      : [],
  );

  const pinnedItems = computed(() =>
    pinnedIds.value
      .map((id) => items.value.find((m) => m.id === id))
      .filter((m): m is RecallItem => m !== undefined),
  );

  const unpinnedItems = computed(() =>
    items.value.filter((m) => !pinnedIds.value.includes(m.id)),
  );

  const pinnedCount = computed(() => pinnedIds.value.length);

  function isPinned(memoryId: string): boolean {
    return pinnedIds.value.includes(memoryId);
  }

  function togglePin(memoryId: string) {
    if (isPinned(memoryId)) {
      pinnedIds.value = pinnedIds.value.filter((id) => id !== memoryId);
    } else {
      if (pinnedIds.value.length >= MAX_PINS) return;
      pinnedIds.value = [...pinnedIds.value, memoryId];
    }
    localStorage.setItem("clio-pinned-ids", JSON.stringify(pinnedIds.value));
  }

  // Focused memory index for keyboard navigation
  const focusedIndex = ref(-1);

  // Shortcut help overlay
  const shortcutHelpOpen = ref(false);

  // Compose state
  const composeOpen = ref(false);

  // Bulk selection
  const selectedIds = ref<Set<string>>(new Set());
  const selectionMode = ref(false);

  const selectedCount = computed(() => selectedIds.value.size);

  function toggleSelection(memoryId: string, shiftKey = false) {
    const newSet = new Set(selectedIds.value);
    if (newSet.has(memoryId)) {
      newSet.delete(memoryId);
    } else {
      newSet.add(memoryId);
    }
    selectedIds.value = newSet;
    selectionMode.value = newSet.size > 0;
  }

  function selectRange(fromIndex: number, toIndex: number) {
    const start = Math.min(fromIndex, toIndex);
    const end = Math.max(fromIndex, toIndex);
    const newSet = new Set(selectedIds.value);
    for (let i = start; i <= end; i++) {
      if (items.value[i]) {
        newSet.add(items.value[i].id);
      }
    }
    selectedIds.value = newSet;
    selectionMode.value = newSet.size > 0;
  }

  function clearSelection() {
    selectedIds.value = new Set();
    selectionMode.value = false;
  }

  function isSelected(memoryId: string): boolean {
    return selectedIds.value.has(memoryId);
  }

  // Notifications for new memories from external sources
  const notifications = ref<Array<{ id: string; title: string | null; namespace: string; source: string | null; timestamp: number }>>([]);
  const notificationsEnabled = ref(true);
  let lastKnownIds = new Set<string>();
  let notificationsInitialised = false;

  function initNotificationTracking() {
    lastKnownIds = new Set(items.value.map((m) => m.id));
    notificationsInitialised = true;
  }

  function checkForNewMemories(newItems: RecallItem[]) {
    if (!notificationsInitialised || !notificationsEnabled.value) return;
    for (const item of newItems) {
      if (!lastKnownIds.has(item.id)) {
        // Only notify for externally created memories (not desktop source)
        if (item.source !== "desktop") {
          notifications.value.push({
            id: item.id,
            title: item.title,
            namespace: item.namespace,
            source: item.source,
            timestamp: Date.now(),
          });
          // Keep only last 10 notifications
          if (notifications.value.length > 10) {
            notifications.value = notifications.value.slice(-10);
          }
        }
        lastKnownIds.add(item.id);
      }
    }
  }

  function dismissNotification(memoryId: string) {
    notifications.value = notifications.value.filter((n) => n.id !== memoryId);
  }

  function dismissAllNotifications() {
    notifications.value = [];
  }

  // Search result cache (session-scoped)
  interface CacheEntry {
    result: RecallResult;
    timestamp: number;
  }
  const searchCache = ref(new Map<string, CacheEntry>());
  const SEARCH_CACHE_MAX = 20;
  let cacheVersion = 0;

  function getCachedSearch(key: string): RecallResult | null {
    const entry = searchCache.value.get(key);
    if (!entry) return null;
    return entry.result;
  }

  function setCachedSearch(key: string, result: RecallResult) {
    const newCache = new Map<string, CacheEntry>(searchCache.value);
    newCache.set(key, { result, timestamp: Date.now() });
    // Evict oldest if over limit
    if (newCache.size > SEARCH_CACHE_MAX) {
      let oldest: string | null = null;
      let oldestTime = Infinity;
      for (const [k, v] of newCache) {
        if (v.timestamp < oldestTime) {
          oldestTime = v.timestamp;
          oldest = k;
        }
      }
      if (oldest) newCache.delete(oldest);
    }
    searchCache.value = newCache;
  }

  function invalidateSearchCache() {
    searchCache.value = new Map<string, CacheEntry>();
    cacheVersion++;
  }

  // Quick-create last-used defaults (persisted to localStorage)
  const quickCreateLastNamespace = ref(
    localStorage.getItem("clio-qc-namespace") || "global",
  );
  const quickCreateLastKind = ref(
    localStorage.getItem("clio-qc-kind") || "note",
  );

  function setQuickCreateDefaults(namespace: string, kind: string) {
    quickCreateLastNamespace.value = namespace;
    quickCreateLastKind.value = kind;
    localStorage.setItem("clio-qc-namespace", namespace);
    localStorage.setItem("clio-qc-kind", kind);
  }

  // Side panel (always visible by default)
  const sidePanelOpen = ref(true);

  // Live polling
  let pollInterval: ReturnType<typeof setInterval> | null = null;
  let pollPaused = false;

  // Command palette
  const paletteOpen = ref(false);
  const paletteQuery = ref("");
  const paletteResults = ref<RecallItem[]>([]);
  const paletteSemanticResults = ref<RecallItem[]>([]);
  const paletteLoading = ref(false);

  const activeNamespace = computed(() => selectedNamespace.value);

  async function fetchNamespaces() {
    try {
      allNamespaces.value = await api.namespaces();
    } catch (e) {
      error.value = String(e);
    }
  }

  async function searchMemories(query: string) {
    loading.value = true;
    error.value = null;
    try {
      const cacheKey = JSON.stringify({
        q: query || null,
        ns: selectedNamespace.value,
        v: cacheVersion,
      });
      const cached = getCachedSearch(cacheKey);
      if (cached) {
        items.value = cached.items;
        total.value = cached.total;
        return;
      }
      const result = await api.recall({
        query: query || undefined,
        namespace: selectedNamespace.value ?? undefined,
      });
      items.value = result.items;
      total.value = result.total;
      setCachedSearch(cacheKey, result);
    } catch (e) {
      error.value = String(e);
    } finally {
      loading.value = false;
    }
  }

  function fingerprint(list: RecallItem[]): string {
    return list.map((i) => `${i.id}:${i.updated_at}`).join("|");
  }

  async function loadRecent(silent = false) {
    if (!silent) {
      loading.value = true;
    }
    error.value = null;
    try {
      const result = await api.recent({
        namespace: selectedNamespace.value ?? undefined,
        kind: filterKind.value ?? undefined,
        tags: filterTags.value.length ? filterTags.value : undefined,
        importance_min: filterImportanceMin.value ?? undefined,
        importance_max: filterImportanceMax.value ?? undefined,
        sort_by: sortBy.value || undefined,
        limit: 50,
      });
      // Only update if data actually changed to avoid unnecessary re-renders.
      if (fingerprint(result.items) !== fingerprint(items.value)) {
        // Check for new memories (notifications)
        checkForNewMemories(result.items);
        items.value = result.items;
        // Initialise notification tracking on first load
        if (!notificationsInitialised) {
          initNotificationTracking();
        }
      }
      total.value = result.total;
    } catch (e) {
      if (!silent) {
        error.value = String(e);
      }
    } finally {
      if (!silent) {
        loading.value = false;
      }
    }
  }

  async function loadStats() {
    try {
      currentStats.value = await api.stats(
        selectedNamespace.value ?? undefined,
      );
    } catch (e) {
      error.value = String(e);
    }
  }

  async function loadActivity() {
    try {
      recentActivity.value = await api.activity({
        namespace: selectedNamespace.value ?? undefined,
        limit: 20,
      });
    } catch (e) {
      error.value = String(e);
    }
  }

  function setNamespace(ns: string | null) {
    selectedNamespace.value = ns;
  }

  // Drawer
  async function openDrawer(memoryId: string) {
    try {
      drawerMemory.value = await api.getMemory(memoryId);
      drawerOpen.value = true;
    } catch (e) {
      error.value = String(e);
    }
  }

  function closeDrawer() {
    drawerOpen.value = false;
    drawerMemory.value = null;
  }

  // Compose
  function toggleCompose() {
    composeOpen.value = !composeOpen.value;
  }

  async function captureMemory(text: string, namespace?: string) {
    try {
      await api.capture({ text, namespace });
      composeOpen.value = false;
      invalidateSearchCache();
      await loadRecent();
    } catch {
      // Capture unavailable — fall back to simple remember
      await api.remember({
        content: text,
        namespace: namespace ?? undefined,
        source: "desktop",
      });
      composeOpen.value = false;
      invalidateSearchCache();
      await loadRecent();
    }
  }

  async function quickCreate(params: {
    content: string;
    namespace?: string;
    kind?: string;
    tags?: string[];
    title?: string;
    importance?: number;
  }) {
    await api.remember({
      content: params.content,
      namespace: params.namespace || "global",
      kind: params.kind || "note",
      tags: params.tags,
      title: params.title,
      importance: params.importance || 3,
      source: "desktop",
    });
    if (params.namespace) {
      setQuickCreateDefaults(params.namespace, params.kind || "note");
    }
    invalidateSearchCache();
    await loadRecent();
    await fetchNamespaces();
  }

  // Palette search
  async function paletteSearch(query: string) {
    paletteQuery.value = query;
    if (!query.trim()) {
      paletteResults.value = [];
      paletteSemanticResults.value = [];
      return;
    }

    paletteLoading.value = true;
    try {
      const ftsResult = await api.recall({
        query,
        namespace: selectedNamespace.value ?? undefined,
        limit: 10,
      });
      paletteResults.value = ftsResult.items;
    } catch (e) {
      error.value = String(e);
    } finally {
      paletteLoading.value = false;
    }
  }

  async function paletteSemanticSearch(query: string) {
    if (!query.trim()) {
      paletteSemanticResults.value = [];
      return;
    }
    try {
      const result = await api.search({
        query,
        namespace: selectedNamespace.value ?? undefined,
        limit: 5,
      });
      paletteSemanticResults.value = result.items;
    } catch {
      // Semantic search may not be available
    }
  }

  function startPolling(intervalMs = 3000) {
    stopPolling();
    pollInterval = setInterval(() => {
      loadRecent(true);
    }, intervalMs);
  }

  function stopPolling() {
    if (pollInterval) {
      clearInterval(pollInterval);
      pollInterval = null;
    }
  }

  function pausePolling() {
    pollPaused = true;
    stopPolling();
  }

  function resumePolling(intervalMs = 3000) {
    if (pollPaused) {
      pollPaused = false;
      loadRecent(true);
      startPolling(intervalMs);
    }
  }

  function closePalette() {
    paletteOpen.value = false;
    paletteQuery.value = "";
    paletteResults.value = [];
    paletteSemanticResults.value = [];
  }

  return {
    items,
    total,
    loading,
    error,
    selectedNamespace,
    allNamespaces,
    currentStats,
    recentActivity,
    drawerMemory,
    drawerOpen,
    composeOpen,
    viewMode,
    setViewMode,
    filterKind,
    filterImportanceMin,
    filterImportanceMax,
    filterTags,
    sortBy,
    groupBy,
    hasActiveFilters,
    setFilterKind,
    setFilterImportance,
    setFilterTags,
    setSortBy,
    setGroupBy,
    clearFilters,
    sidePanelOpen,
    paletteOpen,
    paletteQuery,
    paletteResults,
    paletteSemanticResults,
    paletteLoading,
    activeNamespace,
    fetchNamespaces,
    searchMemories,
    loadRecent,
    loadStats,
    loadActivity,
    setNamespace,
    openDrawer,
    closeDrawer,
    toggleCompose,
    captureMemory,
    paletteSearch,
    paletteSemanticSearch,
    closePalette,
    startPolling,
    stopPolling,
    pausePolling,
    resumePolling,
    pinnedIds,
    pinnedItems,
    unpinnedItems,
    pinnedCount,
    isPinned,
    togglePin,
    focusedIndex,
    shortcutHelpOpen,
    // Bulk selection
    selectedIds,
    selectionMode,
    selectedCount,
    toggleSelection,
    selectRange,
    clearSelection,
    isSelected,
    // Notifications
    notifications,
    notificationsEnabled,
    dismissNotification,
    dismissAllNotifications,
    // Search cache
    invalidateSearchCache,
    // Quick create
    quickCreate,
    quickCreateLastNamespace,
    quickCreateLastKind,
    setQuickCreateDefaults,
  };
});

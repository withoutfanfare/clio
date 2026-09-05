import { defineStore } from "pinia";
import { ref, computed } from "vue";
import * as api from "@/api/memory";
import { memoryKinds } from "@/utils/memoryKinds";
import { groupMemories, type GroupBy } from "@/composables/useGroupedMemories";
import type {
  Memory,
  RecallItem,
  RecallResult,
  MemoryStats,
  RecentEntry,
  ConnectionStatus,
  NamespaceInfo,
} from "@/api/types";

export const useMemoryStore = defineStore("memories", () => {
  // Memory list
  const items = ref<RecallItem[]>([]);
  const total = ref(0);
  const pageSize = 50;
  const collection = ref<"active" | "archive">("active");
  const loadingMore = ref(false);
  let requestedDepth = pageSize;
  let pageEnd = 0;
  let listRequest = 0;
  let listScope = "";
  let listInFlight = false;
  const canLoadMore = computed(() => items.value.length < total.value);
  const loading = ref(false);
  const error = ref<string | null>(null);
  const connectionStatus = ref<ConnectionStatus | null>(null);
  const connectionStatusError = ref<string | null>(null);
  const isRemote = computed(
    () => connectionStatus.value?.backend === "remote",
  );

  // Namespaces
  const selectedNamespace = ref<string | null>(null);
  const allNamespaces = ref<string[]>([]);
  const namespaceDetails = ref<NamespaceInfo[]>([]);
  const namespaceDetailsError = ref<string | null>(null);

  // Stats
  const currentStats = ref<MemoryStats | null>(null);
  const statsError = ref<string | null>(null);
  const recentActivity = ref<RecentEntry[]>([]);
  const availableKinds = computed(() => memoryKinds(
    currentStats.value?.by_kind.map(([kind]) => kind) ?? [],
    items.value.map(item => item.kind),
    filterKind.value ? [filterKind.value] : [],
  ));

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
    resetBrowsing();
    void loadRecent(true);
  }

  function setFilterImportance(min: number | null, max: number | null) {
    filterImportanceMin.value = min;
    filterImportanceMax.value = max;
    if (min !== null) localStorage.setItem("clio-filter-imp-min", String(min));
    else localStorage.removeItem("clio-filter-imp-min");
    if (max !== null) localStorage.setItem("clio-filter-imp-max", String(max));
    else localStorage.removeItem("clio-filter-imp-max");
    resetBrowsing();
    void loadRecent(true);
  }

  function setFilterTags(tags: string[]) {
    filterTags.value = tags;
    if (tags.length) localStorage.setItem("clio-filter-tags", JSON.stringify(tags));
    else localStorage.removeItem("clio-filter-tags");
    resetBrowsing();
    void loadRecent(true);
  }

  function setSortBy(sort: string) {
    sortBy.value = sort;
    localStorage.setItem("clio-sort-by", sort);
    resetBrowsing();
    void loadRecent(true);
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
    localStorage.removeItem("clio-filter-kind");
    localStorage.removeItem("clio-filter-imp-min");
    localStorage.removeItem("clio-filter-imp-max");
    localStorage.removeItem("clio-filter-tags");
    resetBrowsing();
    void loadRecent(true);
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
  function readPinnedIds(): string[] {
    try {
      const stored: unknown = JSON.parse(localStorage.getItem("clio-pinned-ids") || "[]");
      return Array.isArray(stored)
        ? [...new Set(stored.filter((id): id is string => typeof id === "string" && !!id))].slice(0, MAX_PINS)
        : [];
    } catch {
      return [];
    }
  }
  const pinnedIds = ref<string[]>(readPinnedIds());

  const pinnedRecords = ref(new Map<string, RecallItem>());
  const pinError = ref<string | null>(null);
  const pinNow = ref(Date.now());
  let pinsRefreshedAt = 0;
  let pinsInFlight: Promise<void> | null = null;

  function matchesCollection(memory: RecallItem) {
    return (!selectedNamespace.value || memory.namespace === selectedNamespace.value)
      && (!!memory.archived_at === (collection.value === "archive"))
      && (!memory.valid_until || new Date(memory.valid_until).getTime() > pinNow.value)
      && (!filterKind.value || memory.kind === filterKind.value)
      && (filterImportanceMin.value === null || memory.importance >= filterImportanceMin.value)
      && (filterImportanceMax.value === null || memory.importance <= filterImportanceMax.value)
      && (!filterTags.value.length || filterTags.value.every(tag => memory.tags.includes(tag)));
  }

  const pinnedItems = computed(() => pinnedIds.value
    .map(id => items.value.find(item => item.id === id) ?? pinnedRecords.value.get(id))
    .filter((item): item is RecallItem => !!item && matchesCollection(item)));

  const unpinnedItems = computed(() => {
    const visiblePins = new Set(pinnedItems.value.map(item => item.id));
    return items.value.filter(item => !visiblePins.has(item.id));
  });
  const loadedCount = computed(() => new Set([...items.value, ...pinnedItems.value].map(item => item.id)).size);

  async function refreshPins(force = false) {
    if (pinsInFlight) return pinsInFlight;
    if (!force && Date.now() - pinsRefreshedAt < 60_000) return;
    const ids = pinnedIds.value.slice(0, MAX_PINS);
    if (!ids.length) return;
    pinsRefreshedAt = Date.now();
    pinsInFlight = (async () => {
      const results = await Promise.allSettled(ids.map(id => api.getMemory(id)));
      const records = new Map(pinnedRecords.value);
      let failed = 0;
      results.forEach((result, index) => {
        const id = ids[index];
        if (!pinnedIds.value.includes(id)) return;
        if (result.status === "fulfilled" && result.value) {
          records.set(id, { ...result.value, rank: null, linked_from: null });
        } else {
          records.delete(id);
          failed++;
        }
      });
      pinnedRecords.value = records;
      pinNow.value = Date.now();
      pinError.value = failed ? `${failed} pinned ${failed === 1 ? "memory could" : "memories could"} not be loaded. Pin preferences are kept.` : null;
    })().finally(() => { pinsInFlight = null; });
    return pinsInFlight;
  }

  const pinnedCount = computed(() => pinnedIds.value.length);

  // Whether the home view's Pinned section is collapsed. Held here (not in the
  // view) so navigableItems can skip pinned cards that aren't rendered.
  const pinnedCollapsed = ref(false);

  // Flattened list in the exact order cards are rendered on the home view:
  // pinned first (unless collapsed), then unpinned in their grouped/sorted
  // render order. Keyboard navigation and the focus highlight both index into
  // this single list.
  const navigableItems = computed(() => [
    ...(pinnedCollapsed.value ? [] : pinnedItems.value),
    ...groupMemories(unpinnedItems.value, groupBy.value as GroupBy).flatMap(
      (g) => g.items,
    ),
  ]);

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
    const memory = items.value.find(item => item.id === memoryId) ?? (drawerMemory.value?.id === memoryId ? drawerMemory.value : null);
    if (memory && isPinned(memoryId)) {
      pinnedRecords.value = new Map(pinnedRecords.value).set(memoryId, { ...memory, rank: null, linked_from: null });
    } else if (isPinned(memoryId)) {
      void refreshPins(true);
    }
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
    const ordered = navigableItems.value;
    const newSet = new Set(selectedIds.value);
    for (let i = start; i <= end; i++) {
      if (ordered[i]) {
        newSet.add(ordered[i].id);
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

  // ── Action toasts (transient feedback, optional undo) ──
  interface ActionToast {
    id: string;
    message: string;
    variant: "success" | "error" | "info";
    action?: { label: string; run: () => void };
  }
  const toasts = ref<ActionToast[]>([]);
  let toastSeq = 0;

  function dismissToast(id: string) {
    toasts.value = toasts.value.filter((t) => t.id !== id);
  }

  function pushToast(
    message: string,
    variant: ActionToast["variant"] = "info",
    action?: ActionToast["action"],
  ) {
    const id = `toast-${Date.now()}-${toastSeq++}`;
    toasts.value.push({ id, message, variant, action });
    // Keep the stack short so a burst can't grow off-screen.
    if (toasts.value.length > 4) {
      toasts.value = toasts.value.slice(-4);
    }
    setTimeout(() => dismissToast(id), action ? 6000 : 4000);
  }

  function runToastAction(id: string) {
    const toast = toasts.value.find((t) => t.id === id);
    toast?.action?.run();
    dismissToast(id);
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
    pinsRefreshedAt = 0;
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
    try {
      localStorage.setItem("clio-qc-namespace", namespace);
      localStorage.setItem("clio-qc-kind", kind);
    } catch {
      pushToast("Saved successfully; last-used defaults could not be stored on this Mac", "info");
    }
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

  async function loadConnectionStatus() {
    try {
      connectionStatus.value = await api.connectionStatus();
      connectionStatusError.value = null;
    } catch (e) {
      connectionStatusError.value = String(e);
    }
  }

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

  function browsingScope() {
    return JSON.stringify([selectedNamespace.value, collection.value, filterKind.value,
      filterTags.value, filterImportanceMin.value, filterImportanceMax.value, sortBy.value]);
  }

  function resetBrowsing() {
    listRequest++;
    listInFlight = false;
    requestedDepth = pageSize;
    pageEnd = 0;
    items.value = [];
    total.value = 0;
    error.value = null;
    loadingMore.value = false;
    focusedIndex.value = -1;
    listScope = browsingScope();
  }

  async function setCollection(value: "active" | "archive") {
    if (collection.value === value) return;
    collection.value = value;
    resetBrowsing();
    return loadRecent();
  }

  async function loadMore() {
    if (loadingMore.value || !canLoadMore.value) return;
    requestedDepth = Math.max(pageEnd, items.value.length) + pageSize;
    loadingMore.value = true;
    // A deliberate request supersedes an older background refresh.
    await loadRecent();
  }

  async function loadRecent(silent = false) {
    const scope = browsingScope();
    if (scope !== listScope) resetBrowsing();
    if (silent && listInFlight) return;
    const request = ++listRequest;
    listInFlight = true;
    if (!silent && !items.value.length) loading.value = true;
    const params = {
      namespace: selectedNamespace.value ?? undefined,
      kind: filterKind.value ?? undefined,
      tags: filterTags.value.length ? [...filterTags.value] : undefined,
      importance_min: filterImportanceMin.value ?? undefined,
      importance_max: filterImportanceMax.value ?? undefined,
      sort_by: sortBy.value || "importance_desc",
      archived_only: collection.value === "archive",
      limit: pageSize,
    };
    const depth = requestedDepth;
    const current = () => request === listRequest && scope === browsingScope();
    try {
      const loaded = new Map<string, RecallItem>();
      let offset = 0;
      let resultTotal = 0;
      do {
        const result = await api.recent({ ...params, offset });
        if (!current()) return;
        if (params.archived_only && result.archived_only !== true) {
          throw new Error("This backend has not confirmed Archive support. Archive browsing is unavailable.");
        }
        if (params.archived_only && result.items.some(item => !item.archived_at)) {
          throw new Error("This backend does not support the Archive collection: it returned active memories.");
        }
        resultTotal = result.total;
        const previousSize = loaded.size;
        result.items.forEach(item => loaded.set(item.id, item));
        if ((!result.items.length || loaded.size === previousSize) && offset < resultTotal) {
          throw new Error("The backend could not provide the next page. Loaded memories have been kept.");
        }
        offset += result.items.length;
      } while (offset < depth && offset < resultTotal);
      if (!current()) return;
      const nextItems = [...loaded.values()];
      if (collection.value === "active") checkForNewMemories(nextItems.slice(0, pageSize));
      items.value = nextItems;
      total.value = resultTotal;
      pageEnd = offset;
      pinNow.value = Date.now();
      error.value = null;
      if (focusedIndex.value >= navigableItems.value.length) focusedIndex.value = navigableItems.value.length - 1;
      if (!notificationsInitialised) initNotificationTracking();
      void refreshPins();
    } catch (e) {
      if (current()) error.value = String(e);
    } finally {
      if (current()) {
        loading.value = false;
        loadingMore.value = false;
        listInFlight = false;
      }
    }
  }

  let statsRequest = 0;
  async function loadStats() {
    const request = ++statsRequest;
    const namespace = selectedNamespace.value;
    statsError.value = null;
    currentStats.value = null;
    try {
      const stats = await api.stats(namespace ?? undefined);
      if (namespace && stats.namespace !== namespace) throw new Error("The Clio backend needs updating before it can confirm project statistics.");
      if (request === statsRequest && namespace === selectedNamespace.value) currentStats.value = stats;
    } catch (e) {
      if (request === statsRequest && namespace === selectedNamespace.value) statsError.value = String(e);
    }
  }

  async function loadNamespaceDetails() {
    try {
      namespaceDetails.value = await api.namespaceDetails();
      namespaceDetailsError.value = null;
    } catch (e) {
      namespaceDetailsError.value = String(e);
    }
  }

  let activityRequest = 0;
  async function loadActivity() {
    const request = ++activityRequest;
    const namespace = selectedNamespace.value;
    recentActivity.value = [];
    try {
      const activity = await api.activity({
        namespace: namespace ?? undefined,
        limit: 20,
      });
      if (request === activityRequest && namespace === selectedNamespace.value) recentActivity.value = activity;
    } catch (e) {
      if (request === activityRequest && namespace === selectedNamespace.value) statsError.value = String(e);
    }
  }

  function setNamespace(ns: string | null) {
    selectedNamespace.value = ns;
    resetBrowsing();
    currentStats.value = null;
    // Reset notification tracking so the new namespace's items aren't treated as "new"
    notificationsInitialised = false;
    lastKnownIds.clear();
  }

  // All editor exits share the component's save-or-retain guard.
  let drawerCloseGuard: ((nextMemoryId?: string) => Promise<boolean>) | null = null;
  let composeCloseGuard: (() => boolean | Promise<boolean>) | null = null;
  let drawerRequest = 0;

  function setDrawerCloseGuard(guard: typeof drawerCloseGuard) {
    drawerCloseGuard = guard;
  }

  function setComposeCloseGuard(guard: typeof composeCloseGuard) {
    composeCloseGuard = guard;
  }

  async function openDrawer(memoryId: string, options: { eligibleOnly?: boolean; namespace?: string; isCurrent?: () => boolean } = {}) {
    const request = ++drawerRequest;
    const isCurrent = () => request === drawerRequest && (options.isCurrent?.() ?? true);
    if (!isCurrent()) return false;
    if (composeOpen.value && !(await closeCompose())) return false;
    if (!isCurrent()) return false;
    if (drawerCloseGuard && !(await drawerCloseGuard(memoryId))) return false;
    if (!isCurrent()) return false;
    try {
      const currentMemory = drawerMemory.value?.id === memoryId ? drawerMemory.value : null;
      const versionBeforeFetch = currentMemory?.updated_at;
      let memory = await api.getMemory(memoryId);
      if (!isCurrent()) return false;
      if (options.eligibleOnly && (memory.archived_at || (memory.valid_until && !(new Date(memory.valid_until).getTime() > Date.now())))) {
        error.value = "Supporting memory is archived, expired or unavailable.";
        return false;
      }
      // A memory moved since the overview loaded must not open under the previous workspace.
      if (options.namespace !== undefined && memory.namespace !== options.namespace) {
        error.value = "Supporting memory has moved to another workspace.";
        return false;
      }
      if (composeOpen.value && !(await closeCompose())) return false;
      if (!isCurrent()) return false;
      // Include any edits entered while the next memory was loading.
      if (drawerOpen.value && drawerCloseGuard && !(await drawerCloseGuard())) return false;
      if (!isCurrent()) return false;
      // A confirmed save during this fetch is newer than its captured response.
      if (currentMemory && drawerMemory.value === currentMemory && currentMemory.updated_at !== versionBeforeFetch) memory = currentMemory;
      drawerMemory.value = memory;
      drawerOpen.value = true;
      return true;
    } catch (e) {
      if (isCurrent()) error.value = String(e);
      return false;
    }
  }

  async function closeDrawer() {
    const request = ++drawerRequest;
    if (drawerOpen.value && drawerCloseGuard && !(await drawerCloseGuard())) return false;
    if (request !== drawerRequest) return false;
    drawerOpen.value = false;
    drawerMemory.value = null;
    return true;
  }

  async function closeCompose() {
    if (composeOpen.value && composeCloseGuard && !(await composeCloseGuard())) return false;
    composeOpen.value = false;
    return true;
  }

  async function toggleCompose() {
    if (composeOpen.value) return closeCompose();
    ++drawerRequest;
    if (drawerOpen.value && !(await closeDrawer())) return false;
    composeOpen.value = true;
    return true;
  }

  async function captureMemory(text: string, namespace?: string) {
    try {
      const result = await api.capture({ text, namespace });
      if (namespace) setQuickCreateDefaults(namespace, quickCreateLastKind.value);
      if (result.outcome === "Queued") {
        pushToast("Capture queued for review", "info");
        return result;
      }
      invalidateSearchCache();
      await loadRecent();
      return result;
    } catch (error) {
      pushToast(
        "Capture failed; check recent memories before retrying",
        "error",
      );
      throw error;
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

  // ── Archive / delete (centralised so feedback and undo are consistent) ──
  async function archiveMemory(id: string) {
    try {
      await api.archive(id);
      invalidateSearchCache();
      await loadRecent();
      pushToast("Memory archived", "success", {
        label: "Undo",
        run: () => unarchiveMemory(id),
      });
    } catch {
      pushToast("Couldn't archive memory", "error");
    }
  }

  async function unarchiveMemory(id: string) {
    try {
      await api.unarchive(id);
      invalidateSearchCache();
      await loadRecent();
    } catch {
      pushToast("Couldn't unarchive memory", "error");
    }
  }

  async function deleteMemory(id: string): Promise<boolean> {
    try {
      await api.deleteMemory(id);
      invalidateSearchCache();
      await loadRecent();
      pushToast("Memory deleted", "info");
      return true;
    } catch {
      pushToast("Couldn't delete memory", "error");
      return false;
    }
  }

  // Palette search stays active-only even when browsing the Archive collection.
  let paletteRequest = 0;
  let semanticRequest = 0;
  const paletteError = ref<string | null>(null);
  async function paletteSearch(query: string) {
    const request = ++paletteRequest;
    const namespace = selectedNamespace.value;
    paletteQuery.value = query;
    paletteError.value = null;
    if (!query.trim()) {
      paletteResults.value = [];
      paletteSemanticResults.value = [];
      paletteLoading.value = false;
      return;
    }
    paletteLoading.value = true;
    try {
      const result = await api.recall({ query, namespace: namespace ?? undefined, include_archived: false, limit: 10 });
      if (request === paletteRequest && namespace === selectedNamespace.value) paletteResults.value = result.items;
    } catch (e) {
      if (request === paletteRequest && namespace === selectedNamespace.value) paletteError.value = String(e);
    } finally {
      if (request === paletteRequest && namespace === selectedNamespace.value) paletteLoading.value = false;
    }
  }

  async function paletteSemanticSearch(query: string) {
    const request = ++semanticRequest;
    const namespace = selectedNamespace.value;
    if (!query.trim()) {
      paletteSemanticResults.value = [];
      return;
    }
    try {
      const result = await api.search({ query, namespace: namespace ?? undefined, include_archived: false, limit: 5 });
      if (request === semanticRequest && namespace === selectedNamespace.value && query === paletteQuery.value) paletteSemanticResults.value = result.items;
    } catch {
      if (request === semanticRequest) paletteSemanticResults.value = [];
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
    paletteRequest++;
    semanticRequest++;
    paletteLoading.value = false;
    paletteOpen.value = false;
    paletteQuery.value = "";
    paletteResults.value = [];
    paletteSemanticResults.value = [];
  }

  return {
    items,
    total,
    loadedCount,
    canLoadMore,
    loadingMore,
    collection,
    setCollection,
    loadMore,
    loading,
    error,
    connectionStatus,
    connectionStatusError,
    isRemote,
    selectedNamespace,
    allNamespaces,
    namespaceDetails,
    namespaceDetailsError,
    loadNamespaceDetails,
    availableKinds,
    currentStats,
    statsError,
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
    paletteError,
    activeNamespace,
    loadConnectionStatus,
    fetchNamespaces,
    searchMemories,
    loadRecent,
    loadStats,
    loadActivity,
    setNamespace,
    openDrawer,
    closeDrawer,
    closeCompose,
    setDrawerCloseGuard,
    setComposeCloseGuard,
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
    pinError,
    refreshPins,
    unpinnedItems,
    navigableItems,
    pinnedCount,
    pinnedCollapsed,
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
    // Action toasts
    toasts,
    pushToast,
    dismissToast,
    runToastAction,
    // Archive / delete
    archiveMemory,
    unarchiveMemory,
    deleteMemory,
    // Search cache
    invalidateSearchCache,
    // Quick create
    quickCreate,
    quickCreateLastNamespace,
    quickCreateLastKind,
    setQuickCreateDefaults,
  };
});

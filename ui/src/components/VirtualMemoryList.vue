<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from "vue";
import MemoryPage from "@/components/MemoryPage.vue";
import { SSectionHeader } from "@stuntrocket/ui";
import type { MemoryGroup } from "@/composables/useGroupedMemories";
import type { RecallItem } from "@/api/types";
import { useMemoryStore } from "@/stores/memories";

const props = defineProps<{
  groups: MemoryGroup[];
  mode: "list" | "grid";
}>();

const store = useMemoryStore();

// Row heights (px) — kept consistent for smooth virtualisation
const LIST_CARD_HEIGHT = 120;
const GRID_CARD_HEIGHT = 180;
const GROUP_HEADER_HEIGHT = 48;
const GAP = 12;
const GRID_COLUMNS = 3;
const BUFFER = 5;

interface VirtualRow {
  type: "header" | "item" | "grid-row";
  label?: string;
  items?: RecallItem[];
  item?: RecallItem;
  height: number;
  offset: number;
}

const containerRef = ref<HTMLElement | null>(null);
const scrollTop = ref(0);
const containerHeight = ref(600);

/** Flatten groups into virtual rows with pre-computed offsets. */
const virtualRows = computed<VirtualRow[]>(() => {
  const rows: VirtualRow[] = [];
  let offset = 0;

  for (const group of props.groups) {
    // Group header
    rows.push({
      type: "header",
      label: group.label,
      height: GROUP_HEADER_HEIGHT,
      offset,
    });
    offset += GROUP_HEADER_HEIGHT;

    if (props.mode === "grid") {
      // Chunk items into rows of GRID_COLUMNS
      for (let i = 0; i < group.items.length; i += GRID_COLUMNS) {
        const chunk = group.items.slice(i, i + GRID_COLUMNS);
        const h = GRID_CARD_HEIGHT + GAP;
        rows.push({
          type: "grid-row",
          items: chunk,
          height: h,
          offset,
        });
        offset += h;
      }
    } else {
      for (const item of group.items) {
        const h = LIST_CARD_HEIGHT + GAP;
        rows.push({
          type: "item",
          item,
          height: h,
          offset,
        });
        offset += h;
      }
    }

    // Extra spacing between groups
    offset += GAP;
  }

  return rows;
});

const totalHeight = computed(() => {
  const rows = virtualRows.value;
  if (!rows.length) return 0;
  const last = rows[rows.length - 1];
  return last.offset + last.height + GAP;
});

/** Determine which rows are visible. */
const visibleRows = computed(() => {
  const rows = virtualRows.value;
  if (!rows.length) return [];

  const top = scrollTop.value;
  const bottom = top + containerHeight.value;

  // Binary search for first visible row
  let startIdx = 0;
  let lo = 0;
  let hi = rows.length - 1;
  while (lo <= hi) {
    const mid = (lo + hi) >>> 1;
    if (rows[mid].offset + rows[mid].height < top) {
      lo = mid + 1;
    } else {
      hi = mid - 1;
      startIdx = mid;
    }
  }

  // Apply buffer
  startIdx = Math.max(0, startIdx - BUFFER);

  // Find end index
  let endIdx = startIdx;
  for (let i = startIdx; i < rows.length; i++) {
    endIdx = i;
    if (rows[i].offset > bottom) {
      break;
    }
  }
  endIdx = Math.min(rows.length - 1, endIdx + BUFFER);

  return rows.slice(startIdx, endIdx + 1);
});

function onScroll() {
  if (containerRef.value) {
    scrollTop.value = containerRef.value.scrollTop;
  }
}

function updateContainerHeight() {
  if (containerRef.value) {
    containerHeight.value = containerRef.value.clientHeight;
  }
}

let resizeObserver: ResizeObserver | null = null;

onMounted(() => {
  updateContainerHeight();
  if (containerRef.value) {
    resizeObserver = new ResizeObserver(() => updateContainerHeight());
    resizeObserver.observe(containerRef.value);
  }
});

onUnmounted(() => {
  resizeObserver?.disconnect();
});

// Reset scroll position when groups change (filter/sort/namespace switch)
watch(
  () => props.groups,
  () => {
    if (containerRef.value) {
      containerRef.value.scrollTop = 0;
      scrollTop.value = 0;
    }
  },
);

// Auto-scroll to focused item on keyboard navigation
watch(
  () => store.focusedIndex,
  (idx) => {
    if (idx >= 0) {
      scrollToIndex(idx);
    }
  },
);

/** Expose scrollToIndex for keyboard navigation integration. */
function scrollToIndex(flatIndex: number) {
  // Find the row containing the item at the given flat index
  let current = 0;
  for (const row of virtualRows.value) {
    if (row.type === "item" && row.item) {
      if (current === flatIndex) {
        containerRef.value?.scrollTo({
          top: Math.max(0, row.offset - containerHeight.value / 3),
          behavior: "smooth",
        });
        return;
      }
      current++;
    } else if (row.type === "grid-row" && row.items) {
      for (const _ of row.items) {
        if (current === flatIndex) {
          containerRef.value?.scrollTo({
            top: Math.max(0, row.offset - containerHeight.value / 3),
            behavior: "smooth",
          });
          return;
        }
        current++;
      }
    }
  }
}

defineExpose({ scrollToIndex });
</script>

<template>
  <div ref="containerRef" class="virtual-list" @scroll.passive="onScroll">
    <div class="virtual-spacer" :style="{ height: totalHeight + 'px' }">
      <div
        v-for="(row, i) in visibleRows"
        :key="row.type === 'header' ? `h-${row.label}` : row.type === 'item' ? `i-${row.item!.id}` : `g-${row.offset}`"
        class="virtual-row"
        :style="{ transform: `translateY(${row.offset}px)`, height: row.height + 'px' }"
      >
        <!-- Group header -->
        <SSectionHeader v-if="row.type === 'header'" :title="row.label!" />

        <!-- List mode card -->
        <MemoryPage
          v-else-if="row.type === 'item'"
          :memory="row.item!"
          :mode="mode"
          :focused="store.unpinnedItems.indexOf(row.item!) === store.focusedIndex"
        />

        <!-- Grid mode row -->
        <div v-else-if="row.type === 'grid-row'" class="grid-row">
          <MemoryPage
            v-for="gridItem in row.items"
            :key="gridItem.id"
            :memory="gridItem"
            :mode="mode"
            :focused="store.unpinnedItems.indexOf(gridItem) === store.focusedIndex"
          />
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.virtual-list {
  flex: 1;
  overflow-y: auto;
  min-height: 0;
}

.virtual-spacer {
  position: relative;
  width: 100%;
}

.virtual-row {
  position: absolute;
  left: 0;
  right: 0;
  will-change: transform;
}

.grid-row {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: var(--space-3);
}
</style>

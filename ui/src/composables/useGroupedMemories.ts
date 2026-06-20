import { computed, type Ref } from "vue";
import type { RecallItem } from "@/api/types";

export type GroupBy = "importance" | "date" | "kind" | "none";

export interface MemoryGroup {
  label: string;
  items: RecallItem[];
}

function startOfDay(d: Date): number {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

function dateLabel(date: Date, now: Date): string {
  const today = startOfDay(now);
  const target = startOfDay(date);
  const diff = today - target;
  const oneDay = 86_400_000;

  if (diff < oneDay) return "Today";
  if (diff < 2 * oneDay) return "Yesterday";
  if (diff < 7 * oneDay) return "This Week";
  if (diff < 30 * oneDay) return "This Month";
  return date.toLocaleDateString("en-GB", { month: "long", year: "numeric" });
}

const importanceLabels: Record<number, string> = {
  5: "Critical",
  4: "High",
  3: "Normal",
  2: "Low",
  1: "Minimal",
};

function groupByDate(items: RecallItem[]): MemoryGroup[] {
  const now = new Date();
  const groups = new Map<string, RecallItem[]>();
  const order: string[] = [];

  for (const item of items) {
    const date = new Date(item.updated_at);
    const label = dateLabel(date, now);
    if (!groups.has(label)) {
      groups.set(label, []);
      order.push(label);
    }
    groups.get(label)!.push(item);
  }

  return order.map((label) => ({ label, items: groups.get(label)! }));
}

function groupByImportance(items: RecallItem[]): MemoryGroup[] {
  const buckets = new Map<number, RecallItem[]>();

  for (const item of items) {
    const imp = item.importance ?? 3;
    if (!buckets.has(imp)) {
      buckets.set(imp, []);
    }
    buckets.get(imp)!.push(item);
  }

  // Sort buckets by importance descending (5 → 1).
  const sorted = [...buckets.entries()].sort((a, b) => b[0] - a[0]);

  return sorted.map(([imp, group]) => ({
    label: importanceLabels[imp] ?? `Importance ${imp}`,
    items: group,
  }));
}

function groupByKind(items: RecallItem[]): MemoryGroup[] {
  const groups = new Map<string, RecallItem[]>();
  const order: string[] = [];

  for (const item of items) {
    const kind = item.kind || "note";
    if (!groups.has(kind)) {
      groups.set(kind, []);
      order.push(kind);
    }
    groups.get(kind)!.push(item);
  }

  // Sort alphabetically.
  order.sort();

  return order.map((kind) => ({
    label: kind.charAt(0).toUpperCase() + kind.slice(1),
    items: groups.get(kind)!,
  }));
}

function groupNone(items: RecallItem[]): MemoryGroup[] {
  if (!items.length) return [];
  return [{ label: "All memories", items }];
}

/** Pure grouping — shared by the composable and the store's navigation order. */
export function groupMemories(items: RecallItem[], groupBy: GroupBy): MemoryGroup[] {
  switch (groupBy) {
    case "importance":
      return groupByImportance(items);
    case "date":
      return groupByDate(items);
    case "kind":
      return groupByKind(items);
    case "none":
      return groupNone(items);
    default:
      return groupByImportance(items);
  }
}

export function useGroupedMemories(
  items: Ref<RecallItem[]>,
  groupBy: Ref<GroupBy>,
) {
  return computed<MemoryGroup[]>(() => groupMemories(items.value, groupBy.value));
}

import { ref, computed } from "vue";

/**
 * Curated palette of 12 colours that work in both light and dark modes.
 * Each entry is [dot colour, subtle background for badges].
 */
const PALETTE = [
  "#8B5CF6", // violet (Clio accent)
  "#3B82F6", // blue
  "#06B6D4", // cyan
  "#10B981", // emerald
  "#F59E0B", // amber
  "#EF4444", // red
  "#EC4899", // pink
  "#F97316", // orange
  "#14B8A6", // teal
  "#6366F1", // indigo
  "#A855F7", // purple
  "#84CC16", // lime
] as const;

/** Simple DJB2 hash for deterministic colour assignment. */
function djb2(str: string): number {
  let hash = 5381;
  for (let i = 0; i < str.length; i++) {
    hash = ((hash << 5) + hash + str.charCodeAt(i)) >>> 0;
  }
  return hash;
}

// Module-level singleton state (shared across all consumers)
const customColours = ref<Record<string, string>>(
  localStorage.getItem("clio-namespace-colours")
    ? JSON.parse(localStorage.getItem("clio-namespace-colours")!)
    : {},
);

function persistColours() {
  localStorage.setItem(
    "clio-namespace-colours",
    JSON.stringify(customColours.value),
  );
}

export function useNamespaceColours() {
  function getColour(namespace: string): string {
    if (customColours.value[namespace]) {
      return customColours.value[namespace];
    }
    // Deterministic assignment from name hash
    const index = djb2(namespace) % PALETTE.length;
    return PALETTE[index];
  }

  function setColour(namespace: string, colour: string) {
    customColours.value = { ...customColours.value, [namespace]: colour };
    persistColours();
  }

  function removeColour(namespace: string) {
    const next = { ...customColours.value };
    delete next[namespace];
    customColours.value = next;
    persistColours();
  }

  return {
    palette: PALETTE,
    customColours: computed(() => customColours.value),
    getColour,
    setColour,
    removeColour,
  };
}

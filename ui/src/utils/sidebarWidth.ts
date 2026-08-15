export const DEFAULT_SIDEBAR_WIDTH = 220;
export const MIN_SIDEBAR_WIDTH = 180;
export const MAX_SIDEBAR_WIDTH = 420;

export function normaliseSidebarWidth(value: string | number | null | undefined): number {
  const parsed = value == null || (typeof value === "string" && value.trim() === "")
    ? Number.NaN
    : Number(value);

  if (!Number.isFinite(parsed)) {
    return DEFAULT_SIDEBAR_WIDTH;
  }

  return Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, Math.round(parsed)));
}

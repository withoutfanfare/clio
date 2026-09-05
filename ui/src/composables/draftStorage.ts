// Keep one bounded recovery draft per editor; successful saves and explicit discard remove it.
const maximumDraftLength = 1_000_000;

export type DraftRead<T> =
  | { status: "missing" }
  | { status: "ready"; draft: T }
  | { status: "error"; reason: "unreadable" | "oversized" | "unsupported" | "corrupt" };

export function readDraft<T>(key: string, valid: (value: unknown) => value is T): DraftRead<T> {
  let raw: string | null;
  try { raw = localStorage.getItem(key); }
  catch { return { status: "error", reason: "unreadable" }; }
  if (raw === null) return { status: "missing" };
  if (raw.length > maximumDraftLength) return { status: "error", reason: "oversized" };
  try {
    const stored = JSON.parse(raw);
    if (!stored || typeof stored !== "object") return { status: "error", reason: "corrupt" };
    if (stored.version !== 1) return { status: "error", reason: "unsupported" };
    return valid(stored.draft) ? { status: "ready", draft: stored.draft } : { status: "error", reason: "corrupt" };
  } catch {
    return { status: "error", reason: "corrupt" };
  }
}

export function writeDraft(key: string, draft: unknown) {
  const raw = JSON.stringify({ version: 1, draft });
  if (raw.length > maximumDraftLength) throw new Error("Draft exceeds recovery storage limit");
  localStorage.setItem(key, raw);
}

export function removeDraft(key: string) {
  localStorage.removeItem(key);
}

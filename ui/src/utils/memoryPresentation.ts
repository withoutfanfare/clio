export function memoryDate(iso: string, now = new Date()): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "Unknown date";
  return date.toLocaleDateString("en-GB", {
    day: "numeric", month: "short",
    ...(date.getFullYear() !== now.getFullYear() ? { year: "numeric" as const } : {}),
  });
}

export function memoryTimestamp(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleString("en-GB", { dateStyle: "full", timeStyle: "long" });
}

export function memoryExcerpt(content: string, query: string, length = 180): string {
  const text = content.replace(/\s+/g, " ").trim();
  const terms = query.toLocaleLowerCase().split(/\s+/).filter(Boolean);
  const lower = text.toLocaleLowerCase();
  const matches = terms.map(term => lower.indexOf(term)).filter(index => index >= 0);
  const match = matches.length ? Math.min(...matches) : 0;
  const start = Math.max(0, match - 45);
  const excerpt = text.slice(start, start + length);
  return `${start > 0 ? "…" : ""}${excerpt}${start + length < text.length ? "…" : ""}`;
}

import { invoke } from "@tauri-apps/api/core";

interface MemoryLike {
  id: string;
  title?: string | null;
  content: string;
  kind: string;
  namespace: string;
  tags: string[];
  importance: number;
  created_at: string;
  updated_at: string;
}

export function toMarkdown(m: MemoryLike): string {
  const lines: string[] = [];

  if (m.title) {
    lines.push(`# ${m.title}`, "");
  }

  lines.push(m.content, "");

  lines.push("---", "");
  lines.push(`- **Kind:** ${m.kind}`);
  lines.push(`- **Namespace:** ${m.namespace}`);
  lines.push(`- **Importance:** ${m.importance}/5`);
  if (m.tags.length) {
    lines.push(`- **Tags:** ${m.tags.map((t) => `#${t}`).join(" ")}`);
  }
  lines.push(`- **Created:** ${new Date(m.created_at).toLocaleString("en-GB")}`);
  lines.push(`- **Updated:** ${new Date(m.updated_at).toLocaleString("en-GB")}`);
  lines.push(`- **ID:** ${m.id}`);

  return lines.join("\n");
}

export async function copyToClipboard(m: MemoryLike): Promise<boolean> {
  const md = toMarkdown(m);
  try {
    await invoke("cmd_copy_to_clipboard", { text: md });
    return true;
  } catch {
    // Fallback to browser clipboard API.
    try {
      await navigator.clipboard.writeText(md);
      return true;
    } catch {
      return false;
    }
  }
}

export function downloadMarkdown(m: MemoryLike): void {
  const md = toMarkdown(m);
  const slug = (m.title || m.id)
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "")
    .slice(0, 60);
  const blob = new Blob([md], { type: "text/markdown;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `${slug}.md`;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
}

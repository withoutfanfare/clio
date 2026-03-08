import { onMounted, onUnmounted } from "vue";

export interface KeyboardShortcuts {
  onCompose?: () => void;
  onSearch?: () => void;
  onEscape?: () => void;
}

export function useKeyboard(shortcuts: KeyboardShortcuts) {
  function handler(e: KeyboardEvent) {
    const meta = e.metaKey || e.ctrlKey;
    const target = e.target as HTMLElement;
    const isInput =
      target.tagName === "INPUT" ||
      target.tagName === "TEXTAREA" ||
      target.isContentEditable;

    if (meta && e.key === "n") {
      e.preventDefault();
      shortcuts.onCompose?.();
      return;
    }

    if (meta && e.key === "k") {
      e.preventDefault();
      shortcuts.onSearch?.();
      return;
    }

    if (e.key === "Escape") {
      shortcuts.onEscape?.();
      return;
    }
  }

  onMounted(() => window.addEventListener("keydown", handler));
  onUnmounted(() => window.removeEventListener("keydown", handler));
}

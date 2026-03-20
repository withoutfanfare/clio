import { onMounted, onUnmounted } from "vue";

export interface KeyboardShortcuts {
  onCompose?: () => void;
  onSearch?: () => void;
  onEscape?: () => void;
  onNavigateDown?: () => void;
  onNavigateUp?: () => void;
  onOpenFocused?: () => void;
  onArchiveFocused?: () => void;
  onToggleHelp?: () => void;
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

    if (meta && e.key === "d") {
      e.preventDefault();
      shortcuts.onArchiveFocused?.();
      return;
    }

    if (meta && e.key === "/") {
      e.preventDefault();
      shortcuts.onToggleHelp?.();
      return;
    }

    if (e.key === "Escape") {
      shortcuts.onEscape?.();
      return;
    }

    // j/k navigation — only when not in an input
    if (!isInput) {
      if (e.key === "j") {
        e.preventDefault();
        shortcuts.onNavigateDown?.();
        return;
      }

      if (e.key === "k") {
        e.preventDefault();
        shortcuts.onNavigateUp?.();
        return;
      }

      if (e.key === "Enter") {
        e.preventDefault();
        shortcuts.onOpenFocused?.();
        return;
      }
    }
  }

  onMounted(() => window.addEventListener("keydown", handler));
  onUnmounted(() => window.removeEventListener("keydown", handler));
}

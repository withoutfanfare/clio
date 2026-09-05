import { onMounted, onUnmounted } from "vue";

export interface KeyboardShortcuts {
  isModalOpen?: () => boolean;
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
    if (e.defaultPrevented || e.isComposing) return;
    const target = e.target as HTMLElement | null;
    const isInput = !!target && (
      ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName) ||
      target.isContentEditable || !!target.closest?.("form, [contenteditable='true']")
    );
    const isControl = isInput || !!target && (
      ["BUTTON", "A", "SUMMARY"].includes(target.tagName) ||
      !!target.closest?.("button, a, [role='button'], [role='combobox'], [role='menuitem']")
    );
    // Dialogues own Escape and form shortcuts, including while focus moves.
    if (target?.closest?.("dialog[open]")) return;
    if (shortcuts.isModalOpen?.() || target?.closest?.("[aria-modal='true']")) {
      if (e.key === "Escape") shortcuts.onEscape?.();
      return;
    }
    if (isInput) return;
    const meta = e.metaKey || e.ctrlKey;

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
    if (!isControl && !meta && !e.altKey) {
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

import { ref } from "vue";
import { normaliseSidebarWidth } from "../utils/sidebarWidth.ts";

const SIDEBAR_WIDTH_STORAGE_KEY = "clio-sidebar-width";
const KEYBOARD_RESIZE_STEP = 10;

type SidebarWidthStorage = Pick<Storage, "getItem" | "setItem">;

export function useSidebarResize(storage?: SidebarWidthStorage) {
  let availableStorage = storage;
  let storedWidth: string | null = null;

  if (!availableStorage && typeof window !== "undefined") {
    try {
      availableStorage = window.localStorage;
    } catch {
      // Storage is an enhancement; resizing must still work when it is unavailable.
    }
  }

  try {
    storedWidth = availableStorage?.getItem(SIDEBAR_WIDTH_STORAGE_KEY) ?? null;
  } catch {
    // Storage is an enhancement; resizing must still work when it is unavailable.
  }

  const width = ref(normaliseSidebarWidth(storedWidth));
  const isResizing = ref(false);
  let dragStartX = 0;
  let dragStartWidth = width.value;

  function persistWidth() {
    try {
      availableStorage?.setItem(SIDEBAR_WIDTH_STORAGE_KEY, String(width.value));
    } catch {
      // Keep the current in-memory width when persistence is unavailable.
    }
  }

  function startResize(event: PointerEvent) {
    if (event.button !== 0) return;

    const handle = event.currentTarget as HTMLElement | null;
    if (!handle?.setPointerCapture) return;

    event.preventDefault();
    handle.setPointerCapture(event.pointerId);
    dragStartX = event.clientX;
    dragStartWidth = width.value;
    isResizing.value = true;
  }

  function resize(event: PointerEvent) {
    if (!isResizing.value) return;

    event.preventDefault();
    width.value = normaliseSidebarWidth(dragStartWidth + event.clientX - dragStartX);
  }

  function finishResize() {
    if (!isResizing.value) return;

    isResizing.value = false;
    persistWidth();
  }

  function stopResize(event: PointerEvent) {
    if (!isResizing.value) return;

    finishResize();
    const handle = event.currentTarget as HTMLElement | null;
    if (handle?.hasPointerCapture?.(event.pointerId)) {
      handle.releasePointerCapture(event.pointerId);
    }
  }

  function stopResizeAfterCaptureLoss() {
    finishResize();
  }

  function resizeWithKeyboard(event: KeyboardEvent) {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;

    event.preventDefault();
    const delta = event.key === "ArrowLeft" ? -KEYBOARD_RESIZE_STEP : KEYBOARD_RESIZE_STEP;
    width.value = normaliseSidebarWidth(width.value + delta);
    persistWidth();
  }

  return {
    width,
    isResizing,
    startResize,
    resize,
    stopResize,
    stopResizeAfterCaptureLoss,
    resizeWithKeyboard,
  };
}

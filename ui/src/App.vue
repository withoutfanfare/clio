<script setup lang="ts">
import { onMounted } from "vue";
import { SAmbientBlobs } from "@stuntrocket/ui";
import AppBar from "./components/AppBar.vue";
import SidePanel from "./components/SidePanel.vue";
import MemoryDrawer from "./components/MemoryDrawer.vue";
import CommandPalette from "./components/CommandPalette.vue";
import ShortcutHelp from "./components/ShortcutHelp.vue";
import QuickCreate from "./components/QuickCreate.vue";
import BulkActionBar from "./components/BulkActionBar.vue";
import NotificationToast from "./components/NotificationToast.vue";
import { useMemoryStore } from "@/stores/memories";
import { useKeyboard } from "@/composables/useKeyboard";

const store = useMemoryStore();

function navigateDown() {
  if (store.drawerOpen || store.paletteOpen) return;
  const max = store.navigableItems.length - 1;
  if (max < 0) return;
  store.focusedIndex = Math.min(store.focusedIndex + 1, max);
}

function navigateUp() {
  if (store.drawerOpen || store.paletteOpen) return;
  store.focusedIndex = Math.max(store.focusedIndex - 1, 0);
}

function openFocused() {
  if (store.drawerOpen || store.paletteOpen) return;
  const item = store.navigableItems[store.focusedIndex];
  if (item) store.openDrawer(item.id);
}

async function archiveFocused() {
  if (store.drawerOpen) {
    // Archive the drawer memory
    if (!store.drawerMemory) return;
    const { id, archived_at } = store.drawerMemory;
    store.closeDrawer();
    if (archived_at) {
      await store.unarchiveMemory(id);
    } else {
      await store.archiveMemory(id);
    }
    return;
  }
  // Archive the focused list item
  const item = store.navigableItems[store.focusedIndex];
  if (!item) return;
  if (item.archived_at) {
    await store.unarchiveMemory(item.id);
  } else {
    await store.archiveMemory(item.id);
  }
}

useKeyboard({
  onCompose: () => store.toggleCompose(),
  onSearch: () => (store.paletteOpen = !store.paletteOpen),
  onEscape: () => {
    if (store.shortcutHelpOpen) {
      store.shortcutHelpOpen = false;
    } else if (store.paletteOpen) {
      store.closePalette();
    } else if (store.drawerOpen) {
      store.closeDrawer();
    } else if (store.selectionMode) {
      store.clearSelection();
    } else {
      store.focusedIndex = -1;
    }
  },
  onNavigateDown: navigateDown,
  onNavigateUp: navigateUp,
  onOpenFocused: openFocused,
  onArchiveFocused: archiveFocused,
  onToggleHelp: () => {
    store.shortcutHelpOpen = !store.shortcutHelpOpen;
  },
});

onMounted(() => {
  store.fetchNamespaces();
});
</script>

<template>
  <div class="app-shell">
    <SAmbientBlobs color1="#7C3AED" color2="#8B5CF6" color3="#06B6D4" />

    <div class="app-body">
      <SidePanel />

      <div class="content-area">
        <AppBar />
        <main class="main-content">
          <div class="content-column">
            <router-view v-slot="{ Component }">
              <Transition name="page" mode="out-in">
                <component :is="Component" />
              </Transition>
            </router-view>
          </div>
        </main>
      </div>
    </div>

    <MemoryDrawer />
    <CommandPalette />
    <ShortcutHelp />
    <QuickCreate />
    <BulkActionBar />
    <NotificationToast />
  </div>
</template>

<!-- All global styles are now in src/style.css and @stuntrocket/ui -->

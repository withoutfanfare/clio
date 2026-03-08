<script setup lang="ts">
import { onMounted } from "vue";
import AppBar from "./components/AppBar.vue";
import SidePanel from "./components/SidePanel.vue";
import MemoryDrawer from "./components/MemoryDrawer.vue";
import CommandPalette from "./components/CommandPalette.vue";
import { useMemoryStore } from "@/stores/memories";
import { useKeyboard } from "@/composables/useKeyboard";

const store = useMemoryStore();

useKeyboard({
  onCompose: () => store.toggleCompose(),
  onSearch: () => (store.paletteOpen = !store.paletteOpen),
  onEscape: () => {
    if (store.paletteOpen) {
      store.closePalette();
    } else if (store.drawerOpen) {
      store.closeDrawer();
    }
  },
});

onMounted(() => {
  store.fetchNamespaces();
});
</script>

<template>
  <div class="app-shell">
    <div class="ambient-bg" aria-hidden="true">
      <div class="ambient-blob ambient-blob-1" />
      <div class="ambient-blob ambient-blob-2" />
      <div class="ambient-blob ambient-blob-3" />
      <div class="ambient-blob ambient-blob-4" />
      <div class="ambient-blob ambient-blob-5" />
    </div>

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
  </div>
</template>

<style>
@import url('https://fonts.googleapis.com/css2?family=Poppins:ital,wght@0,300;0,400;0,500;0,600;0,700;1,400&display=swap');
:root {
  /* ── Warm Neutral Greys ── */
  --grey-50: #fafaf9;
  --grey-100: #f5f4f2;
  --grey-200: #e7e5e3;
  --grey-300: #d6d3d0;
  --grey-400: #a8a4a0;
  --grey-500: #78736e;
  --grey-600: #57524d;
  --grey-700: #3d3936;
  --grey-800: #292624;
  --grey-850: #201e1c;
  --grey-900: #1a1816;
  --grey-950: #121110;

  /* ── Accent ── */
  --violet-400: #a78bfa;
  --violet-500: #8b5cf6;
  --violet-600: #7c3aed;
  --teal-400: #2dd4bf;
  --teal-500: #14b8a6;

  /* ── Typography: Type Scale ── */
  --text-xs: 11px;
  --text-sm: 13px;
  --text-base: 15px;
  --text-lg: 17px;
  --text-xl: 20px;
  --text-2xl: 26px;
  --text-3xl: 32px;

  /* ── Typography: Line Height ── */
  --leading-tight: 1.3;
  --leading-normal: 1.5;
  --leading-relaxed: 1.65;

  /* ── Typography: Font Weight ── */
  --font-normal: 400;
  --font-medium: 500;
  --font-semibold: 600;
  --font-bold: 700;

  /* ── Typography: Letter Spacing ── */
  --tracking-tight: -0.01em;
  --tracking-normal: 0;
  --tracking-wide: 0.02em;
  --tracking-caps: 0.06em;

  /* ── Spacing ── */
  --space-1: 4px;
  --space-2: 8px;
  --space-3: 12px;
  --space-4: 16px;
  --space-5: 20px;
  --space-6: 24px;
  --space-8: 32px;
  --space-10: 40px;
  --space-12: 48px;

  /* ── Semantic Surfaces ── */
  --colour-bg: #0c0b0a;
  --colour-surface: #111010;
  --colour-surface-panel: rgba(14, 13, 12, 0.75);
  --colour-surface-overlay: rgba(255, 255, 255, 0.04);
  --colour-surface-card: rgba(14, 13, 12, 0.6);
  --colour-surface-input: rgba(0, 0, 0, 0.4);
  --colour-surface-dropdown: rgba(10, 9, 8, 0.92);

  /* ── Glass recipe tokens ── */
  --glass-blur: blur(24px) saturate(1.5);
  --glass-border: rgba(255, 255, 255, 0.08);
  --glass-border-hover: rgba(255, 255, 255, 0.13);
  --glass-glow: inset 0 1px 0 0 rgba(255, 255, 255, 0.06),
                inset 0 0 0 0.5px rgba(255, 255, 255, 0.04);
  --glass-glow-strong: inset 0 1px 0 0 rgba(255, 255, 255, 0.08),
                       inset 0 0 20px rgba(139, 92, 246, 0.03);

  /* ── Text ── */
  --colour-text: var(--grey-100);
  --colour-text-secondary: var(--grey-400);
  --colour-text-muted: var(--grey-500);
  --colour-text-disabled: var(--grey-600);

  /* ── Accent Semantic ── */
  --colour-accent: var(--violet-500);
  --colour-accent-hover: var(--violet-400);
  --colour-accent-muted: color-mix(in srgb, var(--violet-500) 20%, transparent);

  /* ── Status ── */
  --colour-success: #4ade80;
  --colour-warning: #fbbf24;
  --colour-danger: #f87171;
  --colour-info: #60a5fa;

  /* ── Borders ── */
  --colour-border: var(--glass-border);
  --colour-border-hover: var(--glass-border-hover);
  --colour-border-focus: var(--violet-500);

  /* ── Shadows ── */
  --shadow-sm: 0 1px 2px 0 rgba(0, 0, 0, 0.5);
  --shadow-card: 0 2px 6px -1px rgba(0, 0, 0, 0.5);
  --shadow-panel: 0 8px 24px -4px rgba(0, 0, 0, 0.6),
                  0 2px 8px -2px rgba(0, 0, 0, 0.4);
  --shadow-overlay: 0 16px 48px -8px rgba(0, 0, 0, 0.7),
                    0 6px 16px -4px rgba(0, 0, 0, 0.5);
  --shadow-focus: 0 0 0 3px var(--colour-accent-muted);

  /* ── Layout ── */
  --content-width: 720px;
  --appbar-height: 44px;
  --radius-sm: 6px;
  --radius-md: 8px;
  --radius-lg: 12px;
  --radius-xl: 16px;
}

* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

html {
  font-size: var(--text-base);
  line-height: var(--leading-normal);
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
}

body {
  font-family: "Poppins", system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
  background: var(--colour-bg);
  color: var(--colour-text);
  height: 100vh;
  overflow: hidden;
  letter-spacing: var(--tracking-normal);
}

::selection {
  background: var(--colour-accent-muted);
  color: var(--colour-text);
}

.app-shell {
  height: 100vh;
  display: flex;
  flex-direction: column;
  position: relative;
  overflow: hidden;
}

.app-body {
  display: flex;
  flex: 1;
  min-height: 0;
  padding: var(--space-3);
  gap: var(--space-3);
}

.content-area {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  overflow-y: auto;
  border-radius: var(--radius-xl);
  background: var(--colour-surface-card);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  border: 1px solid var(--glass-border);
  box-shadow: var(--glass-glow);
}

/* ── Ambient Background ──
   Static coloured blobs give the glass something to diffuse.
   Without these, backdrop-filter on a flat dark bg produces flat results.
   The blobs are NOT animated — just soft static washes of colour.
   ──────────────────────── */
.ambient-bg {
  position: fixed;
  inset: 0;
  z-index: 0;
  overflow: hidden;
  pointer-events: none;
}

.ambient-blob {
  position: absolute;
  border-radius: 50%;
  filter: blur(200px);
}

.ambient-blob-1 {
  width: 650px;
  height: 650px;
  background: #7c3aed;
  opacity: 0.12;
  top: -280px;
  left: -180px;
}

.ambient-blob-2 {
  width: 550px;
  height: 550px;
  background: #2563eb;
  opacity: 0.09;
  bottom: -220px;
  right: -160px;
}

.ambient-blob-3 {
  width: 480px;
  height: 480px;
  background: #8b5cf6;
  opacity: 0.07;
  top: 35%;
  left: 50%;
  transform: translateX(-50%);
}

.ambient-blob-4 {
  width: 400px;
  height: 400px;
  background: #06b6d4;
  opacity: 0.06;
  top: 10%;
  right: 5%;
}

.ambient-blob-5 {
  width: 350px;
  height: 350px;
  background: #a855f7;
  opacity: 0.08;
  bottom: 15%;
  left: 10%;
}

/* ── Main Content ── */
.main-content {
  flex: 1;
  padding: 0 var(--space-6) var(--space-6);
  position: relative;
  z-index: 1;
  min-width: 0;
}

.content-column {
  width: 100%;
}

/* ── Scrollbar ── */
::-webkit-scrollbar {
  width: 5px;
  height: 5px;
}
::-webkit-scrollbar-track {
  background: transparent;
}
::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.08);
  border-radius: 99px;
}
::-webkit-scrollbar-thumb:hover {
  background: rgba(255, 255, 255, 0.14);
}

/* ── Glass Utility ──
   Dark frosted glass: the background is translucent near-black,
   backdrop-filter blurs the ambient blobs behind it, and
   the inner glow (box-shadow inset) gives that faint luminous edge.
   ──────────────────── */
.glass {
  background: var(--colour-surface-card);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  border: 1px solid var(--glass-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-card), var(--glass-glow);
  transition: border-color 150ms cubic-bezier(0.4, 0, 0.2, 1),
              box-shadow 150ms cubic-bezier(0.4, 0, 0.2, 1);
}

.glass:hover {
  border-color: var(--glass-border-hover);
  box-shadow: var(--shadow-card), var(--glass-glow-strong);
}

/* ── Page Transitions ── */
.page-enter-active {
  transition: opacity 180ms ease, transform 180ms ease;
}
.page-leave-active {
  transition: opacity 120ms ease;
}
.page-enter-from {
  opacity: 0.6;
  transform: translateY(4px);
}
.page-leave-to {
  opacity: 0;
}

/* ── Generic Transitions ── */
.fade-enter-active,
.fade-leave-active {
  transition: opacity 120ms ease;
}
.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}

.slide-right-enter-active,
.slide-right-leave-active {
  transition: transform 200ms cubic-bezier(0.4, 0, 0.2, 1);
}
.slide-right-enter-from,
.slide-right-leave-to {
  transform: translateX(100%);
}

.slide-left-enter-active,
.slide-left-leave-active {
  transition: transform 200ms cubic-bezier(0.4, 0, 0.2, 1);
}
.slide-left-enter-from,
.slide-left-leave-to {
  transform: translateX(-100%);
}

.scale-enter-active {
  transition: all 80ms ease-out;
}
.scale-leave-active {
  transition: all 60ms ease-in;
}
.scale-enter-from,
.scale-leave-to {
  opacity: 0;
  transform: scale(0.97);
}

/* ── Reduced Motion ── */
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.01ms !important;
  }
}
</style>

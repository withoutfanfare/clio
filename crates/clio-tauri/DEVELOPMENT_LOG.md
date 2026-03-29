# Clio Tauri — Development Log

## Cycle: 2026-03-29 21:00
- App: Clio Tauri
- Items completed:
  - [Feature] Add memory deduplication detection with merge suggestions (P2/M) — New `deduplication` module in clio-core with two-strategy duplicate detection: exact content matching via SQL GROUP BY and FTS5 near-duplicate detection using keyword extraction, BM25 scoring, and word-level Jaccard similarity verification. Union-find clustering groups similar pairs. Three new Tauri commands: `cmd_find_duplicates` (scan), `cmd_preview_merge` (preview), `cmd_merge_memories` (execute). Merge operation combines tags, preserves highest confidence/importance, transfers all incoming/outgoing links to the kept memory, and archives merged-away memories. Frontend: deduplication section in ToolsView with scan button, cluster display (exact/similar badges, memory cards with title/namespace/kind/tags/date), merge preview panel showing resulting memory state, and one-click merge with post-merge re-scan. 8 unit tests covering word cleaning, significance filtering, Jaccard similarity, FTS query building, and union-find root finding.
- Items attempted but failed: none
- Branch: feature/memory-deduplication
- Tests passing: yes (cargo test 78 lib + 34 integration, cargo check clean, cargo clippy clean excluding pre-existing warnings, vue-tsc clean excluding pre-existing baseUrl deprecation, vite build clean)
- Build status: pending
- Notes: All business logic in clio-core per architecture rules. Near-duplicate detection extracts up to 10 significant words from title + first 200 chars of content, queries FTS5 with OR-joined quoted terms, then verifies with word-level Jaccard similarity (threshold 0.3) to reduce false positives. Clusters limited to 50 max. Merge archives rather than deletes per Clio's "archive means hidden, not deleted" rule. Self-links prevented during link transfer. Pre-existing uncommitted changes on develop (docs, README, MemoryDrawer, MemoryPage) were not included in the feature commit.

## Cycle: 2026-03-25 22:00
- App: Clio Tauri
- Items completed:
  - [Performance] Memory list virtualisation (P2/S) — VirtualMemoryList component with binary-search-based visible row detection, pre-computed offsets for group headers, list cards, and grid rows. Renders only visible items plus buffer of 5 above/below. Auto-scrolls on keyboard navigation (j/k) via focusedIndex watcher. Resets scroll position on filter/sort/namespace changes. ResizeObserver tracks container dimensions. Supports both list and grid modes with configurable row heights.
  - [UX/UI] Namespace colour coding (P3/S) — useNamespaceColours composable with 12-colour curated palette, DJB2 hash for deterministic colour assignment, localStorage persistence for custom colour overrides. Left border stripe on MemoryPage cards via CSS custom property (--ns-colour). Coloured dot indicator next to namespace name in card meta. Coloured dots replacing folder icons in SidePanel namespace list.
  - [UX/UI] Namespace quick-switch dropdown (P3/S) — Native select element in the memory list header alongside the count and filter controls. Lists all namespaces with memory count from stats data. "All namespaces" default option. Selecting a namespace calls setNamespace and loadRecent for immediate filtering. Styled to match glass design system.
- Items attempted but failed: none
- Branch: feature/virtualisation-namespace-colours-quickswitch
- Tests passing: yes (cargo check clean, cargo clippy clean excluding pre-existing clio-core warnings, vue-tsc clean excluding pre-existing baseUrl deprecation, vite build clean)
- Build status: success (Clio.app + Clio_0.1.0_aarch64.dmg bundled, copied to ~/Desktop/TauriBuilds/clio/Clio-2026-03-25-2226.app)
- Notes: All three items are frontend-only changes — no Rust backend modifications needed. New files: VirtualMemoryList.vue, useNamespaceColours.ts. Modified files: HomeView.vue (virtual list integration, namespace dropdown, layout updates), MemoryPage.vue (namespace colour border stripe and dot), SidePanel.vue (namespace colour dots). The virtualisation uses absolute positioning with translateY transforms for smooth 60fps scrolling, avoiding DOM thrashing.

## Cycle: 2026-03-20 (bulk roadmap implementation)
- **App:** Clio Tauri
- **Items completed:**
  - [UX/UI] Bulk memory operations (P2/M) — Cmd-click and Shift-click multi-select in memory list; floating BulkActionBar component with archive, delete, add tag, remove tag; clio-core repository gets archive_bulk, delete_bulk, add_tag_bulk, remove_tag_bulk functions; selection state in Pinia store with clearSelection on Escape
  - [Feature] Memory export and import (P3/M) — JSONL export/import via clio-core export module exposed through cmd_export_memories and cmd_import_memories Tauri commands; ToolsView with file download and file-picker import
  - [Quality] Memory integrity checks (P2/S) — New clio-core integrity module (integrity.rs) with check() and fix() functions; detects broken links, orphaned links, duplicate content, empty content, tag mismatches; auto-fix for broken links, orphaned links, and tag sync; ToolsView UI with run/fix buttons and issue cards
  - [Distribution] Database backup and restore (P3/S) — New clio-core backup module (backup.rs) with timestamped backup, integrity-checked restore, retention management; ToolsView UI with backup list, create, and restore-with-confirmation
  - [Feature] Real-time notifications (P3/S) — NotificationToast component with toast stack; polling delta detection in loadRecent(); external memory arrivals (non-desktop source) trigger toast; click navigates to memory; dismiss individual or all
  - [Feature] Namespace management (P2/S) — NamespacesView with create, rename, merge, delete operations; new namespace_details Tauri command returning memory count and last activity per namespace; clio-core repository gets rename_namespace and delete_empty_namespace functions; sidebar navigation link added
  - [Feature] Inline editing with revisions (P2/S) — MemoryDrawer already had auto-save editing; added localStorage-based revision tracking with saveRevision on content change; expandable revision history list in drawer details panel
  - [UX/UI] Quick-create dialog (P2/S) — QuickCreate modal triggered by Cmd+N; fields: content, title, kind, namespace, tags, importance; calls store.quickCreate which invokes clio-core remember with source="desktop"; remembers last namespace/kind in localStorage
  - [Performance] Search result caching (P2/S) — Session-scoped Map cache in Pinia store (max 20 entries, LRU eviction); versioned cache keys; invalidated on all write operations (create, edit, archive, delete, bulk ops)
  - [Quality] Source attribution (P2/S) — Source field already in schema; quick-create sets source="desktop"; source badge displayed on memory cards (meta-source class); source value shown in drawer details panel
- **Items attempted but failed:** none
- **Tests passing:** yes (cargo check clean, cargo clippy clean [pre-existing warnings only], vue-tsc clean)
- **Build status:** pending
- **Notes:** Largest single cycle — 10 roadmap items completed. Architecture rule maintained throughout: all business logic in clio-core (2 new modules: integrity.rs, backup.rs; repository.rs extended with bulk operations, namespace management). Tauri adapter adds 16 new commands. Frontend additions: 3 new views (NamespacesView, ToolsView), 3 new components (QuickCreate, BulkActionBar, NotificationToast), extended MemoryPage (selection, source badge), MemoryDrawer (revisions, source display), SidePanel (navigation links), Pinia store (bulk selection, notifications, search cache, quick-create). Two routes added to Vue Router.

## Cycle: 2026-03-20 23:30
- **App:** Clio Tauri
- **Items completed:**
  - [Foundation] Integrate @stuntrocket/ui shared component library and design tokens (P1/M) — Installed @stuntrocket/ui from local Verdaccio registry, configured Tailwind CSS v4 with @tailwindcss/vite plugin, replaced bespoke theme with @stuntrocket/ui tokens.css import, loaded Poppins via Google Fonts, added violet accent override for Clio identity, migrated all components to use shared UI primitives (SButton, SBadge, SCard, SInput, SSidebarLink, SCommandPalette, SDropdownMenu, SFormField, SAmbientBlobs, SSpinner, SEmptyState, SHeading, SSectionHeader, STag, SKbd)
- **Items attempted but failed:** none
- **Branch:** feature/scooda-design-tokens
- **Tests passing:** yes (cargo check clean, cargo clippy clean, vue-tsc clean)
- **Build status:** pending
- **Notes:** Significant refactoring — net removal of 655 lines as inline styles replaced by shared component library classes. Clio uses a violet accent (#8B5CF6) override on top of the @stuntrocket/ui base palette to maintain its distinct identity within the portfolio. Glass morphism tokens (surface-card, surface-panel, surface-overlay) are Clio-specific additions layered on top of the shared token system. Dark mode preserved via .dark class on html element.

## Cycle: 2026-03-20 23:00
- **App:** Clio Tauri
- **Items completed:**
  - [Performance] Lazy-load memory detail panel content (P2/S) — LinkList section now collapsible with on-demand data fetching; links and suggestions load only when expanded; state resets on memory change
  - [Feature] Add memory pinning for frequently accessed entries (P2/S) — Pin/unpin toggle on memory cards and drawer; collapsible "Pinned" section at top of list view; pin count badge in sidebar; localStorage persistence; max 25 pins
  - [UX/UI] Add keyboard shortcuts for common memory operations (P2/S) — j/k list navigation with visual focus indicator, Enter to open, Cmd+D to archive, Cmd+/ for shortcut help overlay; all shortcuts documented in a modal help view
- **Items attempted but failed:** none
- **Branch:** feature/lazy-load-pinning-shortcuts
- **Tests passing:** yes (cargo check clean, cargo clippy clean, vue-tsc clean)
- **Build status:** pending
- **Notes:** First autonomous development cycle for Clio Tauri. Pinned state uses localStorage rather than database metadata to avoid touching clio-core — meets the UI persistence requirement without schema changes. Keyboard navigation uses a focusedIndex tracked in the Pinia store, with visual focus ring matching the existing focus-visible style.

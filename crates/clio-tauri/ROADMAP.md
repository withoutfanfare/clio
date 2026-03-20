# Clio Roadmap

Local-first AI memory backbone — desktop UI for managing persistent, searchable memory across AI tools.

## Pending

### [UX/UI] Add bulk memory operations (select, archive, delete, tag)
- **Priority:** P2 (important)
- **Size:** M (1-3hrs)
- **Added:** 2026-03-19
- **Status:** completed
- **Completed:** 2026-03-20
- **Description:** As the memory database grows, users need to perform bulk operations — selecting multiple memories to archive, delete, or tag in one action. Currently each memory must be managed individually, which becomes tedious when curating hundreds of entries. A checkbox-based selection model with a floating action bar would dramatically improve memory hygiene workflows.
- **Acceptance criteria:**
  - Shift-click and Cmd-click multi-select in memory list views
  - Floating action bar appears when 2+ memories selected, offering: archive, delete, add tag, remove tag
  - Bulk operations show confirmation dialog with count of affected memories
  - Operations execute as a single database transaction for consistency
  - Selection state clears after successful operation

### [Feature] Add memory export and import (JSON/Markdown)
- **Priority:** P3 (nice-to-have)
- **Size:** M (1-3hrs)
- **Added:** 2026-03-19
- **Status:** completed
- **Completed:** 2026-03-20
- **Description:** Users should be able to export their memory database (or a filtered subset) to JSON or Markdown for backup, sharing with teammates, or migrating between machines. Import would allow restoring from backup or ingesting curated memory sets. This supports Clio's local-first philosophy by giving users full data portability.
- **Acceptance criteria:**
  - Export all memories or a filtered/selected subset to JSON or Markdown format
  - JSON export preserves all metadata (tags, links, confidence, timestamps, namespace)
  - Markdown export produces human-readable files grouped by namespace
  - Import from JSON with duplicate detection (skip or overwrite by content hash)
  - Progress indicator for large exports/imports (1000+ memories)

### [Performance] Lazy-load memory detail panel content
- **Priority:** P2 (important)
- **Size:** S (< 1hr)
- **Added:** 2026-03-19
- **Status:** completed
- **Completed:** 2026-03-20
- **Description:** Recent optimisation work improved polling and search performance, but the memory detail/drawer panel still fetches full content (including links and activity history) eagerly when any memory is selected. Deferring link and activity data until the user expands those sections would reduce unnecessary database queries and improve perceived responsiveness, especially with large memory sets.
- **Acceptance criteria:**
  - Memory drawer loads title, content, tags, and metadata immediately on selection
  - Links and activity history load on-demand when their respective sections are expanded
  - No visible delay for the initial panel render (< 100ms)
  - Previously expanded sections remember their state within a session

### [Quality] Add memory integrity checks and orphan detection
- **Priority:** P2 (important)
- **Size:** S (< 1hr)
- **Added:** 2026-03-19
- **Status:** completed
- **Completed:** 2026-03-20
- **Description:** As the memory database grows through automated captures and manual entries, data integrity issues can accumulate silently — broken links pointing to deleted memories, orphaned memories without a namespace, and metadata inconsistencies (e.g. tags referencing non-existent tag IDs). A periodic integrity check that surfaces these issues would help users maintain a healthy memory database.
- **Acceptance criteria:**
  - Integrity check detectable from the UI (settings or tools menu)
  - Reports: broken links, orphaned memories, duplicate content hashes, invalid metadata
  - Each issue includes a suggested fix (delete, re-link, reassign namespace)
  - Batch-fix option for low-risk issues (e.g. remove broken links)
  - Check completes within 5 seconds for databases up to 10,000 memories

### [UX/UI] Add keyboard shortcuts for common memory operations
- **Priority:** P2 (important)
- **Size:** S (< 1hr)
- **Added:** 2026-03-19
- **Status:** completed
- **Completed:** 2026-03-20
- **Description:** Power users managing memories frequently need quick access to core operations without reaching for the mouse. Keyboard shortcuts for search (Cmd+F), quick recall (Cmd+R), archive selected (Cmd+D), and navigation (j/k through memory list) would match the keyboard-driven workflow expectations set by other developer tools.
- **Acceptance criteria:**
  - Cmd+F focuses the search input
  - Cmd+R opens the quick recall dialog
  - Cmd+D archives the currently selected memory (with confirmation)
  - j/k navigates up/down through the memory list
  - Enter opens the selected memory in the detail panel
  - Shortcuts documented in a help view or tooltip overlay (Cmd+/)

### [Innovation] Add memory usage analytics and insights dashboard
- **Priority:** P3 (nice-to-have)
- **Size:** M (1-3hrs)
- **Added:** 2026-03-19
- **Status:** pending
- **Description:** Users have no visibility into how their memory database is being used — which memories are recalled most, which namespaces are growing fastest, or which memories have gone stale. An analytics dashboard showing access patterns, growth trends, and staleness indicators would help users curate their memory more effectively and understand which memories are actually providing value.
- **Acceptance criteria:**
  - Dashboard view showing: total memories, memories by namespace (bar chart), growth trend (last 30 days)
  - Top 10 most-accessed memories with recall count and last access date
  - Stale memory list: memories not recalled in 30+ days, sorted by staleness
  - Namespace health: memories per namespace with average confidence score
  - Dashboard data computed from existing access tracking and memory metadata

### [Distribution] Add one-command database backup and restore
- **Priority:** P3 (nice-to-have)
- **Size:** S (< 1hr)
- **Added:** 2026-03-19
- **Status:** completed
- **Completed:** 2026-03-20
- **Description:** Clio's local-first philosophy means users own their data, but there is no structured backup mechanism. If the SQLite database file is corrupted or a machine is replaced, all memories are lost. A backup command (accessible from the Tauri menu bar and via the CLI) that copies the database to a timestamped archive, and a restore command that validates and replaces from a backup, would give users confidence in Clio's durability promise.
- **Acceptance criteria:**
  - Backup command creates a timestamped copy of the SQLite database file (e.g. clio-backup-2026-03-19T14-30.db)
  - Backup destination configurable (default: alongside the database file)
  - Restore command validates backup integrity (SQLite pragma integrity_check) before replacing
  - Accessible from Tauri menu bar (File → Backup / File → Restore) and via clio-cli
  - Backup count limited by configurable retention (default: keep last 5)

### [Feature] Add memory deduplication detection with merge suggestions
- **Priority:** P2 (important)
- **Size:** M (1-3hrs)
- **Added:** 2026-03-20
- **Status:** pending
- **Description:** As memories accumulate from multiple sources (CLI captures, MCP tool calls, manual entries), content overlap becomes inevitable. The integrity check item detects duplicate content hashes, but near-duplicates — the same concept captured at different times with slightly different wording — require fuzzy matching. A deduplication view that surfaces clusters of similar memories and lets users merge them (combining metadata, preserving the highest-confidence version, reconciling tags) would keep the memory database clean and improve recall quality.
- **Acceptance criteria:**
  - Deduplication scan using content similarity scoring (FTS5 similarity or embedding cosine distance)
  - Results grouped into clusters of similar memories with similarity score
  - Merge action: combines tags, preserves highest confidence, keeps most recent content, maintains all links
  - Merge preview showing what the merged memory will look like before confirming
  - Scan accessible from settings/tools menu and completable within 10 seconds for 5,000 memories

### [Feature] Add real-time new memory notifications from external sources
- **Priority:** P3 (nice-to-have)
- **Size:** S (< 1hr)
- **Added:** 2026-03-20
- **Status:** completed
- **Completed:** 2026-03-20
- **Description:** The desktop UI relies on manual navigation or periodic polling to discover memories created by external tools (CLI `clio remember`, MCP `memory_remember` calls, automated captures) during active sessions. A lightweight notification system that detects new memory arrivals and displays a toast — showing title, namespace, and source — would make the desktop app a live awareness hub for cross-tool memory activity. This is especially valuable during Claude Code sessions where Clio hooks may capture multiple memories in quick succession.
- **Acceptance criteria:**
  - New memories detected within 5 seconds of creation (leveraging existing polling infrastructure)
  - Toast notification showing memory title, namespace, and source tool
  - Notification click navigates to the new memory in the detail panel
  - Memory list auto-refreshes to include new arrivals without manual interaction
  - Notifications suppressible per namespace or globally via settings
  - No duplicate notifications for memories created through the desktop UI itself

### [Feature] Add namespace management view with create, rename, and merge operations
- **Priority:** P2 (important)
- **Size:** S (< 1hr)
- **Added:** 2026-03-19
- **Status:** completed
- **Completed:** 2026-03-20
- **Description:** Namespaces are a core organisational concept in Clio, but there is no dedicated UI for managing them beyond viewing the namespace list. Users cannot rename a namespace (requiring manual re-assignment of all memories), merge two namespaces that have become redundant, or create a new namespace before memories are assigned to it. A namespace management view would make this first-class organisational concept properly manageable.
- **Acceptance criteria:**
  - Dedicated namespace management view accessible from settings or main navigation
  - Create new namespace with name and optional description
  - Rename namespace (updates all associated memories in a single transaction)
  - Merge two namespaces (moves all memories from source to target, preserving all metadata)
  - Delete empty namespace (blocked if memories still assigned, with count displayed)
  - Namespace list shows memory count and last activity date per namespace

### [Feature] Add memory pinning for frequently accessed entries
- **Priority:** P2 (important)
- **Size:** S (< 1hr)
- **Added:** 2026-03-19
- **Status:** completed
- **Completed:** 2026-03-20
- **Description:** Power users return to certain key memories repeatedly — architecture decisions, project contexts, critical constraints. Currently, finding these requires a search or scroll through the list every time. Pinning memories to a persistent "Pinned" section at the top of the list view would provide instant access to the most important memories without searching, similar to pinned messages in Slack or pinned tabs in browsers. This is especially valuable as the memory database grows beyond a few hundred entries.
- **Acceptance criteria:**
  - Pin/unpin toggle on each memory card and in the detail panel
  - Pinned memories displayed in a collapsible "Pinned" section at the top of the list view
  - Pinned state persisted in the database (new column or metadata field)
  - Pin count badge visible in the sidebar navigation
  - Pinned memories excluded from staleness calculations (they are intentionally kept accessible)
  - Maximum pin limit (e.g. 25) to prevent the section from becoming a second list

### [Feature] Add inline memory content editing with revision tracking
- **Priority:** P2 (important)
- **Size:** S (< 1hr)
- **Added:** 2026-03-21
- **Status:** completed
- **Completed:** 2026-03-20
- **Description:** Memories are currently created via CLI or MCP tools and can be archived from the desktop UI, but their content cannot be edited in place. As understanding evolves — decisions are revised, observations refined, constraints clarified — the original memory text becomes outdated. Allowing inline editing from the detail panel with a simple revision history (previous versions stored as metadata) would keep important memories current without requiring delete-and-recreate workflows.
- **Acceptance criteria:**
  - Edit button on the memory detail panel enables inline content editing
  - Previous content version stored as a revision record with timestamp
  - Revision history viewable from the detail panel (expandable list of previous versions)
  - Edit triggers an update through clio-core (not a direct database write)
  - Tags, namespace, and metadata editable alongside content
  - Edit operation updates the memory's modified timestamp without changing the created timestamp

### [UX/UI] Add memory quick-create dialog from the desktop UI
- **Priority:** P2 (important)
- **Size:** S (< 1hr)
- **Added:** 2026-03-20
- **Status:** completed
- **Completed:** 2026-03-20
- **Description:** Memories are currently created exclusively via CLI (`clio remember`) or MCP tool calls (`memory_remember`) from external AI sessions. The desktop UI supports viewing, searching, pinning, and archiving memories but has no way to create one directly. When a user has a thought, decision, or observation while browsing existing memories, they must switch to a terminal to capture it. A quick-create dialog (Cmd+N) with namespace selector, tag presets, and kind picker would make the desktop UI a complete memory management environment rather than a read-only viewer.
- **Acceptance criteria:**
  - Cmd+N opens a quick-create dialog with fields: content (textarea), namespace (dropdown from existing namespaces), tags (tag input with autocomplete), kind (dropdown: decision, constraint, observation, summary)
  - Create action calls clio-core's remember function (not a direct database write)
  - New memory appears in the list view immediately after creation
  - Dialog remembers last-used namespace and kind for rapid successive entries
  - Validation: content required, namespace defaults to "default" if not selected
  - Dialog dismissible via Escape without creating a memory

### [Performance] Add search result caching with session-scoped invalidation
- **Priority:** P2 (important)
- **Size:** S (< 1hr)
- **Added:** 2026-03-21
- **Status:** completed
- **Completed:** 2026-03-20
- **Description:** The completed lazy-load optimisation improved panel rendering, but search queries still hit the database on every keystroke (after debounce). Developers frequently search for the same terms repeatedly during a session — caching recent search results in memory and invalidating only when the underlying data changes (new memory created, memory edited, memory archived) would make repeated searches instant and reduce unnecessary database load during intensive memory management sessions.
- **Acceptance criteria:**
  - Recent search results cached in memory (last 20 unique queries)
  - Cache returns results instantly for repeated queries (< 5ms)
  - Cache invalidated when any memory is created, edited, archived, or deleted
  - Cache scoped to the session (cleared on app restart, not persisted)
  - Cache hit/miss not visible to the user (transparent optimisation)
  - No increase in memory usage beyond 2MB for the cache

### [Quality] Add memory source attribution tracking across capture tools
- **Priority:** P2 (important)
- **Size:** S (< 1hr)
- **Added:** 2026-03-20
- **Status:** completed
- **Completed:** 2026-03-20
- **Description:** Memories are created by multiple tools — CLI (`clio remember`), MCP (`memory_remember`), desktop UI (once the quick-create dialog ships), and automated hooks — but there is no structured attribution showing which tool created each memory. During memory hygiene sessions, users need to understand capture patterns: are most memories coming from automated hooks (potentially low-quality) or deliberate CLI captures (likely higher quality)? Source attribution would support filtering, quality assessment, and help users tune their capture pipeline by identifying which tools produce the most valuable memories.
- **Acceptance criteria:**
  - Memory metadata includes a `source` field populated at creation time (values: cli, mcp, desktop, hook, import)
  - Source indicator displayed on memory cards in the list view (subtle icon or badge)
  - Source filter available in the list view (show only memories from a specific tool)
  - Source data included in the analytics dashboard namespace breakdown
  - Existing memories without source attribution display "unknown" (not blank)
  - No schema migration required if source field already exists in clio-core; otherwise add migration

## Design System Adoption

These items implement the Scooda design system (derived from the Dalil app styleguide) to achieve premium visual uniformity across all Tauri applications. Items are ordered by dependency — foundation must complete before migration, migration before polish.

### [Foundation] Integrate @stuntrocket/ui shared component library and design tokens
- **Priority:** P1 (critical)
- **Size:** M (1-3hrs)
- **Added:** 2026-03-19
- **Status:** completed
- **Completed:** 2026-03-20
- **Description:** Clio's Vue 3 + Pinia frontend already uses Tailwind CSS, but with its own token set and component styles. Adopting the Scooda design system requires installing @stuntrocket/ui from the local Verdaccio registry, replacing the current Tailwind @theme block with Scooda's shared tokens (colours, typography, spacing, shadows), and loading Poppins as the primary font. This foundational step ensures the colour palette, type scale, and spacing grid match the Dalil reference exactly.
- **Acceptance criteria:**
  - .npmrc configured with @stuntrocket:registry=http://localhost:4873
  - @stuntrocket/ui installed as a dependency
  - Existing Tailwind @theme block replaced with Scooda tokens.css import
  - Poppins font loaded as primary sans font
  - Colour palette matches Dalil: surface #FFFFFF/#171717, accent #2563EB/#60A5FA, etc.
  - Typography scale matches: body 15px, H1 1.95-2.15rem, labels 14px
  - Dark mode toggle continues working with Scooda token values

### [UI Migration] Replace bespoke components with @stuntrocket/ui shared components
- **Priority:** P1 (critical)
- **Size:** XL (8hrs+)
- **Added:** 2026-03-19
- **Status:** pending
- **Description:** Systematically replace all locally-defined Vue UI components with @stuntrocket/ui equivalents. This covers the five command module views (memory, search, stats, namespaces, clipboard) and all shared UI primitives. Every button, input, card, badge, modal, toast, sidebar link, and navigation element must use the shared library. The goal is zero bespoke UI primitives — all visual elements sourced from @stuntrocket/ui.
- **Acceptance criteria:**
  - All buttons use @stuntrocket/ui Button (primary, secondary, icon variants)
  - All form controls use @stuntrocket/ui Input, Select, Textarea
  - All cards use @stuntrocket/ui Card variants (content header, sidebar, list)
  - All badges and tags use @stuntrocket/ui Badge, Tag, Pill
  - All modals and overlays use @stuntrocket/ui Modal, CommandPalette, SlideOver patterns
  - All toasts use @stuntrocket/ui Toast with correct status colours
  - Memory list views use @stuntrocket/ui list patterns with hover states
  - Sidebar navigation uses @stuntrocket/ui sidebar link pattern (13px, accent/8 active)
  - No locally-defined UI primitive components remain
  - Dark mode renders correctly with all shared components

### [Polish] Achieve full Scooda styleguide visual conformance
- **Priority:** P2 (important)
- **Size:** L (3-8hrs)
- **Added:** 2026-03-19
- **Status:** pending
- **Description:** After component migration, apply the remaining Scooda styleguide specifications: ambient background blobs, custom accent-tinted scrollbars, micro-animations on all interactive elements, macOS native titlebar integration, correct z-index layering, and accessibility compliance. Visual QA against the Dalil reference to verify the apps are visually indistinguishable in their shared UI patterns.
- **Acceptance criteria:**
  - Ambient background blobs with accent/violet/cyan colours and 20-30s drift animations
  - Custom scrollbars with accent-tinted thumb styling
  - Micro-animations: button press 80ms, link hover 100-130ms, modal 200ms, panel slide 250ms
  - macOS titlebar with drag region and 78px traffic light padding
  - Z-index scale matches styleguide (blobs:1, sidebar:10, topbar:50, modals:100, toasts:200)
  - prefers-reduced-motion respected throughout
  - Focus ring on all interactive elements (2px solid accent/55%, 2px offset)
  - Visual side-by-side comparison with Dalil passes review

## Archived

### [UX/UI] Add memory timeline view with chronological visualisation
- **Priority:** P3 (nice-to-have)
- **Size:** M (1-3hrs)
- **Added:** 2026-03-19
- **Archived:** 2026-03-20
- **Reason:** Visualisation feature with limited practical impact compared to other pending items. The list view with filtering and the analytics dashboard cover the primary discovery use cases. Can be revisited once the core memory management workflow (quick-create, inline editing, deduplication) is complete.

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

### [Feature] Add memory relationship graph visualisation showing inter-memory link topology
- **Priority:** P3 (nice-to-have)
- **Size:** M (1-3hrs)
- **Added:** 2026-03-22
- **Status:** pending
- **Description:** Clio's link system creates relationships between memories (auto-links, manual links), but there is no visual way to explore the resulting graph. Users can see individual memory links in the detail panel, but cannot grasp the overall topology — which memories are highly connected hubs, which clusters exist, and where link chains lead. A force-directed graph visualisation showing memories as nodes and links as edges would make the relationship structure explorable and help users discover unexpected connections, identify isolated memories that should be linked, and understand how their knowledge is structured.
- **Acceptance criteria:**
  - Graph view accessible from the main navigation alongside the existing list and search views
  - Memories rendered as nodes (sized by link count), links rendered as edges (coloured by link type)
  - Namespace filtering: show only memories and links within selected namespaces
  - Click on a node opens the memory detail panel; click on an edge shows the link relationship
  - Graph layout computed client-side using a force-directed algorithm (e.g. d3-force or similar lightweight library)
  - Performance acceptable for graphs of up to 500 nodes (beyond that, filter by namespace first)
  - Zoom, pan, and node drag interactions for exploring dense graphs

### [UX/UI] Add memory template system for structured knowledge capture with predefined fields
- **Priority:** P3 (nice-to-have)
- **Size:** S (< 1hr)
- **Added:** 2026-03-22
- **Status:** pending
- **Description:** Different kinds of memories benefit from different structures — architectural decisions need "Context / Decision / Consequences" fields, debugging observations need "Symptom / Root Cause / Fix" fields, and constraints need "Rule / Reason / Scope" fields. Currently all memories are freeform text, meaning the structure depends entirely on the user's discipline at capture time. Predefined templates per memory kind would guide users toward structured, consistently formatted entries that are easier to search and recall. The quick-create dialog (completed) already has a kind selector — extending it with kind-specific template content that pre-populates the text area would improve memory quality across all capture sources without changing the underlying data model.
- **Acceptance criteria:**
  - Template content pre-populated when a kind is selected in the quick-create dialog (editable, not locked)
  - Templates defined for at least: decision (Context / Options / Decision / Consequences), observation (What happened / Why / Impact), constraint (Rule / Reason / Scope), summary (Scope / Key points / Open questions)
  - Templates stored as configuration (not database records) — editable via settings
  - Template sections rendered as markdown headers in the pre-populated content
  - "Blank" option always available alongside templates for freeform capture
  - Templates do not affect existing memories or the recall/search workflow

### [Feature] Add memory expiry dates with automatic archival for time-bounded knowledge
- **Priority:** P3 (nice-to-have)
- **Size:** S (< 1hr)
- **Added:** 2026-03-22
- **Status:** pending
- **Description:** Some memories have a natural shelf life — sprint goals expire at sprint end, temporary constraints lift after a migration completes, project deadlines become irrelevant after delivery. Currently all memories persist indefinitely unless manually archived, meaning outdated information silently pollutes recall results. An optional expiry date on memories that triggers automatic archival would keep the active memory set current without requiring manual curation, reducing noise in recall and search results. The staleness indicators in the analytics dashboard (pending) would complement this by identifying memories that should have had expiry dates but didn't.
- **Acceptance criteria:**
  - Optional expiry date field available when creating or editing a memory (quick-create dialog, inline editor, CLI, MCP)
  - Expired memories automatically archived on the next app launch or periodic check (every 30 minutes while running)
  - Expiry check runs against all non-archived memories; expired ones archived with reason "expired"
  - Expired-but-not-yet-archived memories visually distinguished in the list view (dimmed, with "expires soon" or "expired" badge)
  - Expiry date editable after creation (extend, remove, or shorten)
  - Bulk set expiry available for multiple selected memories

### [UX/UI] Add memory content markdown preview with formatted rendering in detail panel
- **Priority:** P3 (nice-to-have)
- **Size:** S (< 1hr)
- **Added:** 2026-03-22
- **Status:** pending
- **Description:** The memory detail panel displays content as plain text, but many memories — especially structured captures like decisions (Context / Options / Decision / Consequences), observations, and summaries — use markdown formatting with headers, bullet lists, code blocks, and links. Rendering markdown in the detail panel would significantly improve readability and make structured memory kinds visually scannable. The template system item (pending) proposes structured templates for capture; markdown rendering would make those templates visually meaningful when reviewing memories. The inline editing feature (completed) would toggle between rendered and edit modes.
- **Acceptance criteria:**
  - Memory detail panel renders content as formatted markdown (headers, bold, italic, bullet lists, code blocks, links)
  - Toggle between rendered view and raw text/edit mode (clicking "Edit" switches to textarea, "Preview" switches back)
  - Code blocks rendered with syntax highlighting (matching the app's existing code styling if available)
  - External links open in the default browser (not within the app)
  - Markdown rendering does not affect search, recall, or content storage (display-only transformation)
  - Rendering completes within 50ms for memories up to 5000 characters

### [UX/UI] Add namespace colour coding for visual differentiation in memory lists
- **Priority:** P3 (nice-to-have)
- **Size:** S (< 1hr)
- **Added:** 2026-03-22
- **Status:** completed
- **Completed:** 2026-03-25
- **Description:** When browsing memories across namespaces, visual distinction relies entirely on text labels — the namespace name displayed on each memory card. In a list of dozens or hundreds of memories spanning multiple namespaces, text labels require active reading to identify which namespace a memory belongs to. Assigning a colour to each namespace (from a predefined palette or user-selected) and displaying it as a coloured dot or sidebar stripe on memory cards would enable instant visual parsing of namespace distribution in any list view, complementing the existing namespace management view (completed) with a display-level improvement.
- **Acceptance criteria:**
  - Each namespace assignable a colour from a curated palette (8-12 colours that work in both light and dark modes)
  - Colour indicator displayed on memory cards in the list view (coloured dot, left border stripe, or namespace badge background)
  - Colour assignment managed in the namespace management view (existing, completed)
  - Default colours auto-assigned to namespaces without explicit assignment (deterministic from namespace name hash)
  - Colour visible in all list contexts: main memory list, search results, pinned section, stale memory list
  - Colour does not affect any functional behaviour (filtering, sorting, recall)

### [Quality] Add memory content quality scoring identifying vague or low-detail entries that need enrichment
- **Priority:** P3 (nice-to-have)
- **Size:** S (< 1hr)
- **Added:** 2026-03-23
- **Status:** pending
- **Description:** Memories captured hastily — during rapid CLI sessions or automated hook captures — often contain vague, single-sentence entries that provide little value on recall ("fixed the auth bug", "decided to use Redis", "migration issue"). The source attribution feature (completed) identifies where memories come from, and the deduplication item (pending) catches content overlap, but neither assesses whether a memory's content is detailed enough to be useful when recalled months later. A quality score based on content length, structural markers (headings, bullet lists, specific terms), and specificity heuristics would surface low-quality entries that need enrichment, helping users maintain a high-value memory database rather than a pile of cryptic notes.
- **Acceptance criteria:**
  - Quality score computed per memory based on: content length, presence of structured formatting (headings, lists), specificity indicators (proper nouns, code references, URLs), and absence of vague phrases ("the thing", "that issue", "some problem")
  - Score displayed as a subtle indicator on memory cards in the list view (e.g. thin/medium/rich or a 1-3 star indicator)
  - "Needs enrichment" filter available in the list view showing low-quality memories sorted by importance (high-confidence but low-quality = highest priority for enrichment)
  - Quality scoring does not affect search ranking or recall results (display-only assessment)
  - Scoring runs at creation time and on edit, stored as metadata (no repeated computation)
  - Quality criteria configurable in settings (minimum content length, required structural elements)

### [Feature] Add memory context builder for assembling curated knowledge briefs in the desktop UI
- **Priority:** P2 (important)
- **Size:** M (1-3hrs)
- **Added:** 2026-03-23
- **Status:** pending
- **Description:** The MCP `memory_context` tool builds context programmatically for AI agent consumption, but the desktop UI has no way for users to manually curate and refine context outputs. When preparing for a meeting, onboarding a colleague, or documenting a decision trail, users need to select specific memories, arrange them in a logical order, add bridging narrative, and export the result as a formatted brief. A drag-and-drop context builder in the desktop UI — with memory search integration, ordering controls, section headings, and export to Markdown/clipboard — would make Clio valuable for deliberate knowledge transfer beyond automated recall, filling the gap between raw memory storage and polished knowledge communication.
- **Acceptance criteria:**
  - Context builder view accessible from the main navigation alongside list, search, and graph views
  - Users can add memories to the builder via drag-and-drop from the memory list or search results
  - Memories reorderable within the builder via drag-and-drop
  - Section headings and bridging text insertable between memories
  - Export as Markdown (to clipboard or file) with memory content, metadata, and user-added narrative
  - Builder state auto-saved per session (resume where you left off)
  - Maximum 50 memories per brief to keep exports focused and manageable

### [Feature] Add memory revision diff view showing content changes between editing versions in the detail panel
- **Priority:** P3 (nice-to-have)
- **Size:** S (< 1hr)
- **Added:** 2026-03-24
- **Status:** pending
- **Description:** The inline editing feature (completed) stores revision history when memory content is modified, and users can view previous versions as an expandable list in the detail panel. However, there is no way to see what actually changed between two revisions — users must visually compare the full text of each version. A diff view showing additions, removals, and modifications between any two revisions (or between the current version and any previous revision) would make the revision history actionable rather than merely archival, helping users understand how their knowledge evolved and verify that edits preserved important details.
- **Acceptance criteria:**
  - "Compare" action available between any two revisions in the revision history list
  - Diff displayed as a unified or side-by-side view with additions (green) and removals (red)
  - Default comparison: current version vs the immediately previous revision
  - Diff view rendered inline within the detail panel (not a separate modal or page)
  - Diff computation runs client-side from stored revision text (no additional backend calls)
  - Diff view dismissible to return to the standard content view

### [Performance] Add memory list virtualisation for efficient rendering of large memory databases
- **Priority:** P2 (important)
- **Size:** S (< 1hr)
- **Added:** 2026-03-24
- **Status:** completed
- **Completed:** 2026-03-25
- **Description:** As the memory database grows beyond 500 entries through automated captures, MCP tool calls, and manual creation, the memory list view renders all cards with their indicators (source badges, namespace colour coding, pinned states, quality indicators) — causing perceptible rendering latency during scrolling and filter/sort operations. The lazy-load detail panel optimisation (completed) and search result caching (completed) address panel and query performance, but the list rendering itself has no optimisation. Virtualising the memory list — rendering only visible cards plus a buffer — would keep the list responsive at any database size, mirroring the virtual scrolling already implemented in Sentinel and planned for Quirk.
- **Acceptance criteria:**
  - Memory list uses virtual scrolling, rendering only visible cards plus a buffer of 5 above and below
  - Smooth scrolling at 60fps with databases containing 1000+ memories
  - Card height consistent; no visual jumping during scroll
  - Keyboard navigation (j/k from existing shortcuts) works correctly with virtualised rows
  - Filter, sort, and namespace quick-switch operations update the virtualised list without scroll position reset
  - Memory usage stays flat as database size grows beyond 500 entries

### [UX/UI] Add namespace quick-switch dropdown in the memory list header for rapid context filtering
- **Priority:** P3 (nice-to-have)
- **Size:** S (< 1hr)
- **Added:** 2026-03-24
- **Status:** completed
- **Completed:** 2026-03-25
- **Description:** When browsing memories, focusing on a single namespace — the most common browsing pattern during active project work — requires navigating to the namespace management view or manually applying namespace filters. A persistent dropdown in the memory list header that immediately filters to the selected namespace, with "All namespaces" as the default, would make namespace-scoped browsing a single click. This complements the namespace colour coding item (pending) with a functional navigation shortcut, and the namespace management view (completed) with an in-context filtering control that doesn't require leaving the memory list.
- **Acceptance criteria:**
  - Namespace dropdown displayed in the memory list header alongside existing filter/sort controls
  - Dropdown lists all namespaces with memory count per namespace (sourced from existing data)
  - "All namespaces" option at the top (default selection, current behaviour)
  - Selecting a namespace immediately filters the memory list to that namespace only
  - Selected namespace persisted for the session (reset on app restart)
  - Dropdown updates when namespaces are created, renamed, or merged (reactive to existing namespace management)

### [Distribution] Add Tauri auto-updater with release notes display for seamless version delivery
- **Priority:** P2 (important)
- **Size:** M (1-3hrs)
- **Added:** 2026-03-24
- **Status:** pending
- **Description:** The Clio desktop UI has no update mechanism — users must manually discover, download, and replace the application binary to get new versions. As memory management features mature (deduplication, graph visualisation, context builder) and the underlying clio-core evolves, delivering fixes and improvements seamlessly becomes critical. Tauri's built-in updater plugin with a release notes panel would ensure users always run the latest version without manual intervention, matching the auto-updater items already planned across the rest of the portfolio (Grove, Fuse, Amber, Drift, Sentinel). Clio is the only desktop app in the portfolio without this planned.
- **Acceptance criteria:**
  - Tauri updater plugin configured with update endpoint and code signing
  - Update check on app launch with non-intrusive notification banner (not modal)
  - Release notes displayed in a panel before the user confirms installation
  - "Install now" and "Remind me later" options; deferred updates install on next launch
  - Current version and last update check timestamp visible in settings
  - Update progress indicator during download and installation

## Design System Adoption

These items implement the @stuntrocket/ui design system (derived from the Dalil app styleguide) to achieve premium visual uniformity across all Tauri applications. Items are ordered by dependency — foundation must complete before migration, migration before polish.

### [Foundation] Integrate @stuntrocket/ui shared component library and design tokens
- **Priority:** P1 (critical)
- **Size:** M (1-3hrs)
- **Added:** 2026-03-19
- **Status:** completed
- **Completed:** 2026-03-20
- **Description:** Clio's Vue 3 + Pinia frontend already uses Tailwind CSS, but with its own token set and component styles. Adopting the @stuntrocket/ui design system requires installing @stuntrocket/ui from the local Verdaccio registry, replacing the current Tailwind @theme block with @stuntrocket/ui shared tokens (colours, typography, spacing, shadows), and loading Poppins as the primary font. This foundational step ensures the colour palette, type scale, and spacing grid match the Dalil reference exactly.
- **Acceptance criteria:**
  - .npmrc configured with @stuntrocket:registry=http://localhost:4873
  - @stuntrocket/ui installed as a dependency
  - Existing Tailwind @theme block replaced with @stuntrocket/ui tokens.css import
  - Poppins font loaded as primary sans font
  - Colour palette matches Dalil: surface #FFFFFF/#171717, accent #2563EB/#60A5FA, etc.
  - Typography scale matches: body 15px, H1 1.95-2.15rem, labels 14px
  - Dark mode toggle continues working with @stuntrocket/ui token values

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

### [Polish] Achieve full @stuntrocket/ui styleguide visual conformance
- **Priority:** P2 (important)
- **Size:** L (3-8hrs)
- **Added:** 2026-03-19
- **Status:** pending
- **Description:** After component migration, apply the remaining @stuntrocket/ui styleguide specifications: ambient background blobs, custom accent-tinted scrollbars, micro-animations on all interactive elements, macOS native titlebar integration, correct z-index layering, and accessibility compliance. Visual QA against the Dalil reference to verify the apps are visually indistinguishable in their shared UI patterns.
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

### [UX/UI] Add memory list density toggle switching between compact and comfortable layouts for different browse modes
- **Priority:** P3 (nice-to-have)
- **Size:** S (< 1hr)
- **Added:** 2026-03-24
- **Archived:** 2026-03-24
- **Reason:** Low-impact display preference that can wait until the core memory management pipeline (deduplication, context builder, graph visualisation — all pending) is complete. The existing layout serves both scanning and browsing use cases adequately. Revisit after higher-value pending items are delivered and user feedback indicates density is a friction point.

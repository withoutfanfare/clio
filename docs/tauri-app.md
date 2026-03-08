# Clio Desktop App (Tauri)

Clio ships a native desktop application built with Tauri 2 and Vue 3. It provides a visual interface for browsing, creating, editing, and searching memories without using the CLI or MCP server.

---

## Building and Running

### Prerequisites

- Rust toolchain (same as the CLI)
- Node.js 18+ and npm
- Tauri CLI (`cargo install tauri-cli`)

### Development

```sh
# Install frontend dependencies
cd ui && npm install && cd ..

# Start in dev mode (hot-reload)
cargo tauri dev
```

### Production build

```sh
cargo tauri build
```

The binary is output to `target/release/clio-tauri`. On macOS a `.app` bundle is produced.

---

## Architecture

### Backend (`crates/clio-tauri/`)

The Tauri backend is a thin adapter over `clio-core`. It shares the same database, settings, and embedding backend as the CLI and MCP server.

**AppState** — a single `Mutex<AppState>` is managed by Tauri and shared across all commands:

| Field | Type | Purpose |
|---|---|---|
| `db_path` | `PathBuf` | Path to the SQLite database |
| `conn` | `Connection` | Persistent database connection (opened once at startup) |
| `settings` | `Settings` | Loaded once at startup from `clio-settings.json` |
| `backend` | `BackendState` | Embedding backend (loaded asynchronously on a background thread) |
| `cache` | `ClioCache` | In-memory cache to avoid per-request reinit |

The embedding backend loads in the background so the window appears immediately without waiting for model initialisation.

### Frontend (`ui/`)

A Vue 3 single-page application using:

- **Pinia** for state management (`stores/memories.ts`)
- **Vue Router** for navigation (Home, Stats views)
- **Tauri IPC** via `@tauri-apps/api/core` `invoke()` for all backend calls
- Custom CSS design system (no Tailwind) with glass-morphism aesthetic

---

## Tauri Commands

All commands are defined in `crates/clio-tauri/src/commands/` and registered in `lib.rs`. They follow the pattern: receive `State<Mutex<AppState>>`, lock it, delegate to `clio-core`, and return serialisable results.

### Memory Commands (`commands/memory.rs`)

| Command | Parameters | Returns | Description |
|---|---|---|---|
| `cmd_remember` | namespace, kind, title, summary, content, tags, source, source_ref, confidence, importance, metadata, upsert | `Memory` | Store a new memory (auto-embeds if enabled) |
| `cmd_update` | memory_id, namespace, kind, title, summary, content, tags, source, source_ref, confidence, importance, metadata | `Memory` | Update an existing memory by ID |
| `cmd_recall` | query, namespace, kind, tags, match_all_tags, importance_min, importance_max, sort_by, include_archived, limit, offset | `RecallResult` | Full-text search with filtering and sorting |
| `cmd_get` | memory_id | `Memory` | Fetch a single memory by ID |
| `cmd_recent` | namespace, kind, tags, match_all_tags, importance_min, importance_max, sort_by, include_archived, limit | `RecallResult` | List recent memories with filtering |
| `cmd_archive` | memory_id | `Memory` | Soft-archive a memory |
| `cmd_unarchive` | memory_id | `Memory` | Restore an archived memory |
| `cmd_delete` | memory_id | `Memory` | Delete a memory |
| `cmd_link` | from_memory_id, to_memory_id, relationship, metadata | `MemoryLink` | Create a link between two memories |
| `cmd_get_links` | memory_id | `Vec<MemoryLink>` | Get all links from a memory |
| `cmd_capture` | text, namespace | `CaptureResult` | LLM-classify and store unstructured text |
| `cmd_cache_clear` | — | `CacheClearResult` | Clear the in-memory cache |

### Search Commands (`commands/search.rs`)

| Command | Parameters | Returns | Description |
|---|---|---|---|
| `cmd_search` | query, namespace, include_archived, limit | `RecallResult` | Semantic search using embeddings |
| `cmd_suggest_links` | memory_id, threshold, limit | `Vec<SuggestionResult>` | Find semantically similar unlinked memories |
| `cmd_backend_status` | — | `String` | Check embedding backend status ("ready", "loading", "unavailable: ...") |

### Stats Commands (`commands/stats.rs`)

| Command | Parameters | Returns | Description |
|---|---|---|---|
| `cmd_stats` | namespace | `MemoryStats` | Aggregate statistics (counts, breakdowns, top tags) |
| `cmd_activity` | namespace, limit | `Vec<RecentEntry>` | Recent activity feed |

### Namespace Commands (`commands/namespaces.rs`)

| Command | Parameters | Returns | Description |
|---|---|---|---|
| `cmd_namespaces` | — | `Vec<String>` | List all namespaces |
| `cmd_init_namespace` | directory, namespace | `()` | Create a `.clio-namespace` file in a directory |
| `cmd_detect_namespace` | directory | `Option<DetectedContext>` | Auto-detect namespace from a directory |

---

## Frontend API Layer

The TypeScript API layer (`ui/src/api/memory.ts`) wraps every Tauri command with typed functions. It handles the `snake_case` to `camelCase` parameter conversion that Tauri requires.

Key types are defined in `ui/src/api/types.ts`:

- `Memory` — full memory record with all fields
- `RecallItem` — memory with optional `rank` and `linked_from` fields
- `RecallResult` — paginated result set (`total`, `count`, `offset`, `limit`, `items`)
- `MemoryLink` — a directional link between two memories
- `MemoryStats` — aggregate statistics including breakdowns by namespace, kind, week, and top tags
- `RememberInput` — input shape for creating/updating memories

---

## UI Components

### Layout

- **`App.vue`** — Root shell with ambient background blobs, sidebar, content area, drawer, and command palette
- **`SidePanel.vue`** — Persistent left sidebar showing Clio branding, namespace navigation, project creation, and footer actions (Statistics, New Memory)
- **`AppBar.vue`** — Transparent top bar within the content area
- **`MemoryDrawer.vue`** — Right-side slideout panel for viewing and editing a memory (title, content, kind, namespace, tags, importance, links)

### Memory Views

- **`HomeView.vue`** — Main view with compose area, filter bar, and memory list/grid
- **`StatsView.vue`** — Statistics dashboard
- **`DateGroup.vue`** — Groups memories under a label (date, kind, importance)
- **`MemoryPage.vue`** — Individual memory card in list or grid mode

### Input Components

- **`ComposeArea.vue`** — Expandable compose panel for creating new memories with optional detail fields (title, kind, namespace, tags, importance)
- **`CommandPalette.vue`** — Spotlight-style search overlay combining full-text and semantic search
- **`TagInput.vue`** — Tag chip input with type-ahead autocomplete from available tags
- **`KindSelector.vue`** — Dropdown for selecting memory kind (note, observation, decision, preference, snippet, knowledgebase)
- **`LinkList.vue`** — Displays and manages links for a memory

### Composables

- **`useAutoSave.ts`** — Debounced auto-save with status tracking (dirty, saving, saved, error)
- **`useGroupedMemories.ts`** — Groups memory lists by date, kind, importance, or none
- **`useKeyboard.ts`** — Global keyboard shortcut handler
- **`useDebounce.ts`** — Generic debounce utility

---

## Keyboard Shortcuts

| Shortcut | Action |
|---|---|
| `Cmd+Shift+M` | Global show/hide (works even when app is not focused) |
| `Cmd+N` | Toggle compose area |
| `Cmd+K` | Toggle command palette |
| `Escape` | Close palette or drawer |
| `Cmd+Enter` | Submit compose or create memory from palette |
| `Arrow Up/Down` | Navigate palette results |
| `Enter` | Open selected palette result |

---

## State Management

The `useMemoryStore` (Pinia) manages all application state:

### Persisted to localStorage

| Key | Default | Purpose |
|---|---|---|
| `clio-view-mode` | `"list"` | List or grid view toggle |
| `clio-filter-kind` | `null` | Active kind filter |
| `clio-filter-imp-min` | `null` | Minimum importance filter |
| `clio-filter-imp-max` | `null` | Maximum importance filter |
| `clio-filter-tags` | `[]` | Active tag filters |
| `clio-sort-by` | `"importance_desc"` | Sort order |
| `clio-group-by` | `"importance"` | Grouping strategy |

### Live Polling

The home view polls for updated data every 3 seconds using `store.startPolling(3000)`. Silent polling updates only replace the items array when data has actually changed (via JSON comparison) to avoid unnecessary re-renders and UI flicker.

### Filter and Sort Options

**Sort orders:** importance (asc/desc), created (asc/desc), updated (asc/desc)

**Group by:** importance, date, kind, none

**Filters:** kind, importance range (1–5 pill selector), tags (type-ahead with available tags dropdown)

---

## Design System

The app uses a custom CSS design system defined as CSS custom properties in `App.vue`:

- **Colour palette:** Warm neutral greys (`--grey-50` to `--grey-950`), violet accent (`--violet-400/500/600`), teal secondary (`--teal-400/500`)
- **Glass morphism:** `backdrop-filter: blur(24px) saturate(1.5)` with translucent borders and inner glow box-shadows
- **Typography:** Poppins font family with a defined type scale (`--text-xs` through `--text-3xl`)
- **Spacing:** 4px base unit (`--space-1` through `--space-12`)
- **Transitions:** Page transitions, slide/fade/scale animations, reduced-motion support
- **Ambient background:** Five static coloured blobs behind all glass surfaces, giving the blur filter visible material to diffuse

---

## Environment Variables

| Variable | Purpose |
|---|---|
| `CLIO_DB_PATH` | Override the database path (defaults to platform standard location) |
| `RUST_LOG` | Control log verbosity (e.g. `info`, `debug`, `clio_tauri=debug`) |

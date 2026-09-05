# Clio desktop app review — 4 September 2026

Status: review complete. The user approved implementation; progress and verification are recorded in [the implementation record](2026-09-04-app-improvements.md). The findings below preserve the original review evidence.

Clio has a consistent visual identity, useful project scoping and a good foundation for searching and editing shared memories. The highest-value improvements are protecting edits, making the selected project reliable, and turning the existing attention screen into somewhere that work can actually be resolved. Visual polish comes after those changes.

## Scope and evidence

Reviewed the installed macOS app, reporting version `0.1.0`, and the relevant Vue/Tauri/core code on `develop` at `9a08735`. The installed binary's exact source commit was not established; live observations and source findings are distinguished below.

The walkthrough covered the memory list, project selection, attention items and their evidence, filters, search, context builder, statistics, settings and the empty new-memory dialogue. Eleven screenshots were retained in the local review artefact, outside the repository. No memory was created, edited, archived, deleted, completed or snoozed. No operational settings were saved, no capture payloads were inspected, and no deployment or recovery was attempted.

| Step | Surface | Assessment |
|---|---|---|
| 1 | Project memory list | Consistent cards; long workspace navigation, ambiguous dates and counts. |
| 2 | Needs attention | Useful reasons and completion conditions; missing task titles and routes from health counts to action. |
| 3 | Supporting memory | Opens successfully; closing returns to the memory list rather than the attention screen. |
| 4 | Filters | Controls render, but common stored memory kinds are absent. |
| 5 | Search | A project-scoped query returned relevant results; dates, excerpts and explicit scope would help selection. |
| 6 | Context builder | Clear empty state; draft persistence and end-to-end export were not exercised. |
| 7 | Statistics | Loads, but mixes project counts with global statistics. |
| 8 | Settings | Clearly identifies the shared backend and avoids showing credentials. |
| 9 | New memory | Opens with the wrong default project for this context; importance buttons have no accessible names. |
| 10 | Dialogue keyboard behaviour | Escape leaves the dialogue open; Shift+Tab reaches the settings input behind it. |
| 11 | End of memory list | No next-page or load-more control; only the first 50 records are requested. |

## What to preserve

- The violet accent and restrained dark layout are coherent across the app. Keep that direction.
- Project colouring, search shortcuts, the resizable sidebar and progressive disclosure in the editor are useful foundations.
- Attention items already explain why they surfaced and what completion means.
- Settings identify that changes affect the shared backend. Queue-unavailable handling exists and should remain explicit.
- Archive has an undo action in the store. Preserve soft deletion and the existing backup/confirmation safeguards.

## Priority fixes

### 1. Protect pending edits when the editor closes — P1

**Evidence:** source-confirmed and reproduced with the current autosave composable in an isolated harness. `useAutoSave` waits two seconds before saving. The drawer's close button and backdrop call `cancel()`, which clears the timer, discards the pending patch and resets the dirty flag. A normal delayed save produced one stubbed API call; scheduling an edit and closing before the timer produced zero calls and `dirty=false`.

**Impact:** typing and immediately closing the drawer can silently lose the latest edit. Escape uses a different close path, so closing behaviour is inconsistent. On a failed request, the pending patch has also already been removed and there is no explicit retry action.

**Change:** give every close path a shared save-or-retain policy. Flush pending edits before dismissal, or retain the draft with an explicit choice to discard it. Keep failed/conflicting edits recoverable and expose Retry. Preserve the existing optimistic concurrency check.

**Code:** `ui/src/composables/useAutoSave.ts:5`, `:16`, `:69`; `ui/src/components/MemoryDrawer.vue:210`; `ui/src/App.vue:66`.

**Finished when:** type then close within two seconds using the close button, backdrop and Escape; the edit either saves or remains recoverable. Repeat for an API failure, a conflicting update and switching between memories. All checks must use disposable records.

### 2. Default new memories to the selected project — P1

**Evidence:** live: `clio` was selected but New memory opened with `global`. Source: QuickCreate restores the last-used namespace without consulting the selected namespace.

**Impact:** a user can save a project memory into another namespace and then fail to find it in their current project. This directly undermines namespace integrity.

**Change:** prefer the explicitly selected workspace when opening a fresh draft; use the last-used workspace only from All memories. Show a clear destination such as “Save to clio”. There are currently two composers wired to `composeOpen`; choose one visible flow and preserve a deliberate distinction between manual entry and automatic capture.

**Code:** `ui/src/components/QuickCreate.vue:9`, `:21`; `ui/src/components/ComposeArea.vue:14`; `ui/src/views/HomeView.vue:145`; `ui/src/App.vue:125`.

**Finished when:** opening from two different selected workspaces chooses each workspace correctly, an explicit user override is honoured, and All memories has a clear fallback. Only one composer appears and receives keyboard focus.

### 3. Make the whole collection browsable and pins dependable — P2

**Evidence:** live: the selected project showed 520 active memories, but scrolling reached the final card with no continuation control. Source: `loadRecent()` always requests 50, replaces the list, and HomeView has no pagination. Pinned items are resolved only against that same loaded page.

**Impact:** most of a large collection is inaccessible through browsing. A pinned memory outside the current result page cannot appear in the pinned section. Search can find some records, but it does not replace complete browsing.

**Change:** add a simple Load more control and an honest “50 of 520” count. Keep loaded pages during refresh and define stable ordering. Fetch visible pinned records independently, respecting workspace, archive and expiry rules.

**Code:** `ui/src/stores/memories.ts:141`, `:426`; `ui/src/views/HomeView.vue:361`.

**Finished when:** a disposable collection of more than 100 records can be traversed without missing or duplicated rows, polling preserves loaded pages, and an eligible pinned record outside the first page remains visible.

### 4. Give attention items names and preserve the return journey — P2

**Evidence:** live: each reminder is headed “View memory” followed by an eight-character suffix. Opening one shows a useful full title. Closing the drawer then leaves the user in the memory list. The overview payload does not include a title, and the view navigates to the home-based memory-detail route.

**Impact:** users must open reminders individually to identify them, then navigate back to continue reviewing.

**Change:** include memory titles in the core attention projection and display them as the row heading. Keep “why now” and completion conditions. Open evidence over the current attention view or restore that view and scroll position on close. Keep full IDs for all operations; the shortened suffix is presentation, not an identifier to use for mutation.

**Code:** `ui/src/views/AttentionView.vue:113`, `:206`, `:276`; `ui/src/api/types.ts:273`; `ui/src/router/index.ts:13`; `crates/clio-tauri/src/commands/attention.rs:13`.

**Finished when:** each row is understandable without opening it, evidence opens and returns to the same place, and missing/archived evidence produces a clear safe state. Test both local and remote adapters.

### 5. Turn health counts into useful next actions — P2

**Evidence:** the screen reported **294 queued/processing captures, 509 failed capture files and one item awaiting review**, alongside stale consolidation and a green Atlas connection indicator. These were observed counts, not a diagnosis of unrecovered data loss. The queue command reads this Mac's local spool, whereas attention data comes through the active backend. The cards do not open a queue or review screen. No inbox route or inbox API adapter is present in the desktop UI.

**Impact:** the user sees a failure but cannot determine its scope or resolve the review item through the app. “Connected” can be mistaken for overall capture health.

**Change:** label separate states for “Atlas connection” and “Capture on this Mac”. Show the time checked and oldest pending age, provide Refresh, and add a read-only diagnostic drill-down with a concrete next step. Add an inbox view using the existing core review operations so “1 awaiting review” is actionable. Show readable reasons for failures without exposing capture payloads or secrets.

**Code:** `crates/clio-tauri/src/commands/attention.rs:122`; `ui/src/views/AttentionView.vue:42`, `:134`, `:147`; `ui/src/api/memory.ts:406`; `ui/src/router/index.ts:5`.

**Finished when:** local queue and remote service status cannot be confused, unknown/unreadable buckets cannot masquerade as healthy zero counts, freshness is visible, and the review item can be opened and resolved with server-confirmed feedback.

**Operational follow-up:** reconcile the observed local backlog with the existing `CLIO-OPS-008` recovery/install record before retrying anything. Counts alone do not establish whether historical dead letters were already replayed. This review does not authorise replay, purge, installation or changes to Atlas. Keep operational progress in `docs/operations/roadmap.md`.

### 6. Use the same memory kinds throughout the app — P2

**Evidence:** the live filter menu offers note, observation, decision, preference, snippet and knowledgebase. It omits fact, constraint, summary, receipt and task despite those kinds being present in stored memories. The filter, quick-create form and editor each hard-code different lists.

**Impact:** users cannot filter important categories, and changing between creation and editing presents a different vocabulary.

**Change:** use one supported kind catalogue with existing custom/unknown values preserved. Include observed backend kinds in filtering without silently converting them during edits.

**Code:** `ui/src/views/HomeView.vue:38`; `ui/src/components/QuickCreate.vue:17`; `ui/src/components/KindSelector.vue:10`.

**Finished when:** all supported stored kinds can be filtered, created and edited consistently, and an unrecognised stored kind survives an unrelated edit unchanged.

### 7. Correct the scope of statistics — P2

**Evidence:** selecting `clio` produced 538 total, 520 active and 18 archived memories, but also 14,038 links and 26.09 link density. Other namespaces appeared in the same screen without a scope distinction. In core, memory totals and embedding counts are namespace-scoped, but link totals, kind/week breakdowns and tags are global. Link density divides the global link count by the project memory count.

**Impact:** these figures do not describe one consistent collection. The project selector's total of 538 also sits next to the active list count of 520 without explaining the difference.

**Change:** define and apply one scope per metric in core. Specify how cross-namespace links count. Keep global workspace totals available for navigation through a clearly distinct field or request so fixing project statistics does not break the selector. Label active versus total counts and identify the selected scope on the statistics screen.

**Code:** `crates/clio-core/src/stats.rs:15`, `:53`, `:87`; `ui/src/views/StatsView.vue:18`; `ui/src/views/HomeView.vue:105`, `:149`.

**Finished when:** fixtures with two namespaces, archived records and cross-namespace links yield consistent scoped totals and density. Verify the same semantics through CLI, MCP and Tauri.

### 8. Make dialogues safe to operate by keyboard — P2

**Evidence:** live: Escape did not dismiss the empty new-memory dialogue. Three Shift+Tab presses from its content field moved focus to the model-setting input behind it. Its five importance buttons appear unnamed in the accessibility tree. The dialogue has no modal role or focus containment in its template; App.vue's Escape handler does not handle `composeOpen`.

**Impact:** keyboard users can unknowingly interact with the background screen and cannot identify the importance controls. This matters particularly when the background contains shared settings.

**Change:** use the existing shared accessible dialogue component if it supplies focus containment, modal semantics and focus restoration. Otherwise implement those behaviours once. Make Escape dismiss an empty draft and preserve/confirm non-empty drafts. Give each importance button an accessible name and selection state; associate form labels with their fields.

**Code:** `ui/src/components/QuickCreate.vue:93`, `:121`, `:143`; `ui/src/App.vue:66`; related editor shell at `ui/src/components/MemoryDrawer.vue:237`.

**Finished when:** Tab and Shift+Tab stay within each dialogue, Escape follows the draft policy, focus returns to the invoking control, and a screen reader announces field names and importance values. Recheck the editor and search dialogues without assuming their behaviour is identical.

## Further product improvements

These are recommendations, not implementation defects reproduced in this review.

1. **Make workspace selection quick.** Add a small workspace search and pinned/recent workspaces above the complete list. Show a friendly display name while retaining the exact namespace in details. Put destructive workspace operations behind a management menu, keeping current safeguards. Do not automatically merge similar names; canonical namespace selection remains explicit. Relevant entry point: `ui/src/components/SidePanel.vue:179`.

2. **Show when a memory is from.** Cards currently display only the time of day (`MemoryPage.vue:145`), even when grouped by importance across months. Use “Today 16:13” or “29 Aug” with a full timestamp available. Offer a prominent Recent/Important choice and retain the user's selection. Add dates and a short matching excerpt to search results so older instructions can be distinguished from current decisions. Keep history and source provenance visible rather than inferring that the newest memory supersedes everything older.

3. **Provide an Archive view.** Archive and Unarchive operations exist, but normal desktop recall does not offer a way to browse archived records. Add a clearly labelled, opt-in archived-only view with restore. This makes “archive means hidden, not deleted” understandable and usable beyond the short-lived Undo toast. Do not include archived records in default recall. Relevant entry points: `ui/src/api/memory.ts:83`, `:114`; `ui/src/stores/memories.ts:562`; HomeView's filter controls.

4. **Save context briefs as drafts.** The builder currently stores one draft in sessionStorage (`ContextBuilderView.vue:44`), which does not provide reliable persistence across app sessions. Add a small durable draft mechanism with an explicit saved state and recovery on reopen. Preserve source memory IDs and show the selected search scope; consider optional named briefs only once multiple drafts are needed. Test restart recovery and Markdown export with synthetic data.

5. **Improve reading comfort without a redesign.** The secondary metadata, small dots and toolbar icons are faint in the captured dark theme. Increase the contrast and useful hit areas for essential controls, reduce competing card decoration where necessary, and use consistent page-heading sizes. Measure actual rendered colours and test zoom/reduced motion before making accessibility compliance claims.

## Suggested implementation order

| Slice | Work | Acceptance focus |
|---|---|---|
| A — Protect edits and scope | Findings 1, 2 and 8 | No lost drafts, correct namespace and contained keyboard focus. |
| B — Complete retrieval | Findings 3 and 6; Archive view | Every eligible record is reachable; kinds and pins behave consistently. |
| C — Resolve attention | Findings 4 and 5 | Named tasks, reliable return navigation, scoped health and an actionable review inbox. |
| D — Explain the collection | Finding 7; workspace search and visible dates | Counts, scope and recency are understandable. |
| E — Improve brief creation | Durable drafts and reading comfort | Work survives restart; exports and readability are verified. |

Start each slice with the smallest regression coverage for its user-visible behaviour, then exercise it in an isolated app/database. Preserve core ownership of business logic and identical local/remote semantics. This review does not replace the operational roadmap or claim its open items have been resolved.

## Verification and limits

- `npm test` in `ui/`: **6 passed, 0 failed**. These cover sidebar sizing and persistence, not the workflows above.
- The autosave reproduction transpiled the current composable without editing it, used deterministic fake timers and a stubbed update API, and confirmed both the normal-save control and the close-before-save loss path. No live API calls were made by the harness.
- Live interaction verified navigation, search results, the kind menu, evidence opening/closing, list exhaustion, creation defaults, Escape and focus escaping behind the creation dialogue.
- The native select popup could be read through accessibility but could not be captured as a standalone screenshot. Its available options are corroborated by the source.
- No production build, full Rust suite, new install, offline fault injection, real save, archive, deletion, capture replay, screen-reader audit or context export was run. The exact deployed commit and the reason for the local queue backlog remain unverified.
- The review began with a clean working tree. Its only intended repository change is this document. No server or background service was started for the review.

# Clio app improvements — implementation record

Status: all five approved slices of the [app review](2026-09-04-app-review.md) and the valid follow-up review findings are implemented, reviewed and shipped. Runtime commit `5622456d710032096470ab60d2a3554806f4d7c2` is pushed to `develop`, deployed on Atlas and installed on this Mac. All three RedPen reports are handled and moved into `implemented`; their dispositions and verification are recorded below.

## Release — 5 September 2026

The user subsequently authorised committing, building and shipping the app and services. The release includes the native CLI, MCP executable, existing local daemon and production desktop app. The app was built from an isolated committed snapshot using the existing locked dependencies; only the build-command path was overridden for that snapshot.

- `scripts/atlas-release.sh check` and `deploy` passed for the full runtime SHA. Linux CLI and MCP release builds passed. The verified online backup is `20260905T102519Z-5622456d710032096470ab60d2a3554806f4d7c2-1158736` under the standard Atlas backup directory. Backup and live SQLite `quick_check` both returned `ok`; migration versions match and no migration gate remains.
- Activation terminated 28 old MCP sessions. A subsequent status check reported **1 running, 0 old/deleted**. Existing AI clients need to reconnect if they do not do so automatically.
- `scripts/macos-install.sh install <runtime-sha> --with-daemon` passed and reported **Daemon installed and healthy**. The local MCP executable was separately built and atomically installed with a rollback copy. All three installed binaries match their release-build hashes and pass strict signature verification. The daemon's settings and LaunchAgent plist remained byte-identical.
- `cargo tauri build --ci --bundles app -- --locked` passed. Clio **0.1.0**, native **arm64**, is installed at `/Applications/Clio.app` and has been launched successfully. Its executable SHA-256 matches the build: `bfd88971375be7fbf114ea2788110662dce9a6d5a8d95b89468c2b6c2f2366da`. `codesign --verify --deep --strict` passed. The previous app is retained in Trash; previous command-line binaries and release logs are retained with the standard deployment backups.
- The installed app displayed **atlas · Connected**. Its Clio workspace loaded **18 archived records**, **2 unresolved inbox captures**, and statistics of **562 total / 544 active / 18 archived**. These are observations at verification time, not fixed dataset expectations.
- A fresh bridge using the installed CLI passed live, read-only checks for scoped statistics, active/archive recall, pagination, the pending/edited inbox envelope, attention titles and semantic search.
- Release verification passed **9 disposable CLI/MCP contract checks** and **28 adapter tests**. The earlier **110 UI**, **305 core** and **9 Tauri** checks cover the same application source. Temporary verification processes stopped, and the isolated release build workspace was removed. The production app and existing daemon intentionally remain running.

This completes the authorised release on this Mac and Atlas. Other-machine installation and capture-hook recovery remain separate items in the [operational roadmap](../operations/roadmap.md).

## Behaviour

### Safer editing and creation

- Closing an editor waits for pending saves. Errors and conflicting versions retain the draft with retry and explicit discard. An update entered during a save is retained and uses only the version returned by that successful write.
- Unsaved editor and creation drafts have bounded, versioned local recovery. Failed recovery reserves the original draft until opened or explicitly discarded. Recovery storage failure is visible; keep the app open until the save succeeds or export the draft.
- Missing recovery is distinct from unreadable, malformed, oversized or unsupported data. Failed reads and failed explicit removal preserve the original bytes and block replacement.
- Reopening the same memory retains a save confirmed during an overlapping fetch. An external refresh still replaces unchanged drawer content.
- Discard clears unfinished tag input before closing. It cannot requeue discarded content through a delayed blur handler.
- Creation prefers the selected workspace, then the last-used workspace, then `global`. A destination override remains while the draft is open. One creator provides deliberate manual and automatic capture modes.
- Native HTML dialogues provide modal semantics and browser focus containment. Importance and form controls are labelled. Global shortcuts do not act on background cards or override normal form/button activation.
- The native window remains resident: Rust prevents destruction; the frontend awaits save guards and then hides. Native close persisted an edit and left the process running. The hidden duration remains unverified.
- Failure to save last-used preferences does not turn a confirmed capture into a retryable save failure.

### Complete retrieval

- Active and Archive are explicit collections. Archive queries request only archived records; ordinary recall still excludes them.
- Recall/recent accept archive-only filtering and pagination. Full IDs break equal sort ties; empty pages retain the filtered total. Linked recall obeys archive-only eligibility as well.
- Browsing uses bounded 50-record pages, preserves loaded depth across refreshes and rejects stale responses. Pins resolve independently, respect all selected tags, workspace and archive/expiry visibility, and use bounded refresh. Shared kinds retain custom values. Cards and search include dates, scope and excerpts. Recent/Important and Active/Archive are explicit controls.
- Arrow keys keep the command palette's selected result in view. The search input identifies its active result for assistive technology.
- Palette navigation ignores input-method composition, including WebKit's key-code 229 fallback, so confirming composed text does not open a result.

### Attention and capture review

- Attention projection supplies visible memory titles keyed by full ID without updating access counts. Archived, expired, missing and differently scoped evidence has no title projection.
- Evidence opens in the drawer over the attention route. Refresh, workspace changes and leaving the view invalidate earlier requests. Archive and expiry are rechecked before display; that check may fetch a record before withholding it.
- Older backends without title projection show an explicit unavailable state. Attention actions wait for server confirmation and block duplicate submissions.
- Capture diagnostics inspect directory entries and metadata only. Missing/unreadable buckets are unknown, not healthy zeroes. Responses identify the local Mac scope and time checked.
- A future modification time has age zero; it does not make the bucket count unavailable.
- Local diagnostics remain available independently of attention loading. Expandable details and copying expose queue metadata only. Recovery remains a separate operational workflow.
- Thin desktop inbox adapters use the existing local core or remote MCP operations. Listing returns the oldest 100 unresolved captures across all workspaces.
- Edited captures remain in the unresolved queue until approved or rejected. Attention's review count includes both pending and edited records.
- Attention's review count and the sidebar open the inbox. Captured content, exact destination and editable suggestions are visible together, including custom kinds.
- Only changed suggestions are sent. Saving suggestions requires server confirmation before approval or rejection; clearing a title sends an explicit empty value. Failed requests retain the draft or selected item. Rejection requires confirmation. Successful decisions reload the next oldest batch.
- One versioned, bounded local recovery slot retains changed suggestions by full review ID, without copying captured content. Reopening restores the matching review. Unavailable or unsupported recovery blocks replacement until explicitly discarded. Storage failure warns that the draft is retained only in the open view. Late saves from an unmounted view cannot clear a newer draft.
- Confirmed decisions clear matching recovery, including suggestions already matching the server. Failed decisions retain it, so the next capture is not blocked by a completed review.
- No live captures have been triaged.

### Clearer collection context

- Every statistics breakdown uses the same selected namespace. Outgoing links belong to their source namespace, including links to another namespace; density divides these by total memories in scope, including archived records.
- The workspace selector uses a separate namespace-details request so scoped statistics do not remove other workspaces from navigation.
- Workspace search, up to eight pinned workspaces and five recent workspaces retain exact namespace identities. Deletion remains behind a management menu with the existing confirmation and backup safeguards.
- Pins for unavailable workspaces no longer consume the pin limit. Confirmed workspace deletion removes its shortcut entries.
- The workspace delete handler rejects `global`, and the desktop command rejects it before creating a backup. Core already enforced this restriction.
- Statistics identify the scope and explain active/archive and outgoing-link denominators.

### Durable briefs and reading comfort

- One context brief survives a fresh session in local storage, with an explicit saved/restored/error state. Existing session drafts migrate on open. Clear removes the saved content; source IDs remain in the draft and Markdown export.
- Context search identifies its workspace and discards stale responses after query/scope changes. Clearing an in-flight query clears its loading state. A failed migration still displays the original session brief and retains its storage.
- Stored briefs validate every field used by rendering and export, including tag arrays, before restoring a block.
- Secondary text is lighter in the existing dark palette, with visible keyboard focus outlines and reduced-motion support. Calculated token contrast against `#1E1E1E` improves tertiary text from 2.90:1 to 6.52:1; this is a palette calculation, not a rendered accessibility certification.

## Contracts and compatibility

- `archived_only` defaults to false and takes precedence over `include_archived` in core, CLI, MCP and Tauri recall/recent. Recent pagination defaults to offset zero. The result echoes `archived_only`; Archive requires explicit true confirmation, including for empty responses from older backends.
- Attention adds `memory_titles`; older remote backends do not provide this field. Scoped statistics echo `namespace` and the UI withholds project figures when that confirmation is absent. Release the corresponding backend before relying on these new remote capabilities.
- MCP inbox JSON lists accept `include_status_scope: true`, returning `{ "items": [...], "includes_edited": true }`. Existing callers retain the array response. The desktop adapter requests and requires this confirmation before accepting remote inbox results, including an empty queue. Its UI-facing list remains an array.
- Local queue health is independent of the shared backend connection. A connected backend does not establish a healthy local capture worker.
- Review edits change suggestions; they do not approve or promote a capture. The UI must wait for server confirmation before removing an item.
- No schema or applied migration changed.

## Verification evidence

- Before the RedPen follow-up, 303 core tests passed with `cargo test --locked --offline -p clio-core --no-default-features` (198 unit, 77 integration, 10 multiple-connection, 11 repair and 7 worktree projection tests).
- Focused regressions reproduced the original failures before their fixes: scoped statistics, attention-title visibility, archived-only pages, edited inbox visibility, unavailable spool buckets, close-before-save, failed/conflicting saves, in-flight edits, wrong creation scope, lost restart drafts, discarded-tag requeue, modal request races and successful-capture preference failures.
- Independent reviews approved all five slices after corrections. Regressions cover tag discard, recovery reservation, creation races, preference failures, context migration, stale activity, workspace pin limits and palette scrolling.
- Before the RedPen follow-up, `npm --prefix ui test` reported **87 tests, 87 passed, 0 failed** and exited 0. These include **17 inbox tests**; the focused attention, inbox and editor review passed **36 tests**.
- Vue type checking with `node ui/node_modules/vue-tsc/bin/vue-tsc.js --noEmit -p ui/tsconfig.json` exited 0 with no output. From `ui/`, `node node_modules/vite/bin/vite.js build` transformed **118 modules**, built in **1.05 seconds** and exited 0. Both commands used Herd Node 22.23.2.
- `git diff --check` exited 0 with no output.
- UI compilation uses the existing Herd Node runtime. The bundled signed Node runtime cannot load the installed Rollup native addon due to macOS Team ID validation; no dependency change was needed.
- Core checks were repeated after compatibility and title changes. The backend review found the empty old-server Archive issue; its fix has an explicit response marker and regression. Slice C corrections cover restart recovery, stale evidence lifetime, older backend confirmation and confirmed recovery cleanup. All passed independent source review and their respective UI or adapter regressions.
- On the authorised verification continuation, `cargo fetch --locked` succeeded. `cargo test --locked -p clio-cli -p clio-mcp --no-default-features` passed **28 tests**: 12 CLI unit, 5 CLI integration and 11 MCP tests.
- `cargo test --locked -p clio-tauri` passed **8 tests**, including both remote inbox decoder tests and remote bridge process shutdown. `cargo build --locked -p clio-mcp --no-default-features` also passed.
- `python3 scripts/verify-app-contracts.py` reported **9 contract checks passed; no live settings, database or providers used.** It exercised real CLI/MCP archive and FTS filters, offsets, scoped statistics, eligible attention titles, edited inbox visibility and confirmed approval/rejection. It confirmed MCP shutdown and removal of its temporary database/settings.
- Native packaging passed with `cargo tauri build --debug --bundles app --config <temporary-config> --ci -- --locked --offline`. The temporary configuration supplied the verification identity and embedded UI assets. Fresh UI type checking/build passed; the Rust build completed in **23.33 seconds**. Signature verification reported **valid on disk** and **satisfies its Designated Requirement** for `target/debug/bundle/macos/Clio Verification.app`. The bundle has not been installed.

The reusable contract check expects current `clio` and `clio-mcp` binaries in `target/debug`:

```bash
cargo build --locked -p clio-cli -p clio-mcp --no-default-features
python3 scripts/verify-app-contracts.py
```

Use `--bin-dir <directory>` for another build directory. Each run creates and removes its own database and settings, disables providers and remote routing, and stops its MCP process.

### Native acceptance with synthetic data

CUA inspected the real packaged application through its native accessibility tree and screenshots. No Orca permission change was required. Verification used `com.clio.desktop.verification`, an incognito webview, temporary database/settings/spool and disabled providers. The initial fixtures contained 125 memories, including 5 archived records, 2 review captures and 1 attention item.

- Browsing loaded **50 → 100 → 120 active records**. Archive displayed 5 records; restoring one reduced its count to 4.
- Attention opened titled evidence in the drawer over `/attention`. Editing then closing saved the content to SQLite and returned to that route. Queue diagnostics showed unknown waiting/processing counts for the missing processing directory and 1 failed entry.
- Unsaved inbox suggestions survived navigation to Statistics with “Recovered unsaved suggestions”. Saving kept the capture edited and awaiting a decision. Explicit approval created a memory with the edited title. Rejection displayed “Keep reviewing” and “Confirm rejection”; confirming created no memory.
- After approval and restoration, global statistics showed **126 total / 122 active / 4 archived**. Workspace B showed **5 / 5 / 0**, with 3 facts, 2 constraints and correctly scoped activity.
- Context search in workspace B returned only its 5 records. Adding a result and navigating away/back restored the brief. Quick creation defaulted to B and stored exactly one memory there.
- The command palette displayed 6 results. Five Down keystrokes scrolled the final selected result into view; Return opened it. Editing and pressing Escape persisted the change to SQLite.
- Editing then closing the native window persisted the change to SQLite and left the process resident. CUA immediately reobserves/reactivates the window, so this did not establish how long it remained hidden.
- A subsequent isolated persistent-profile run used actual Cmd-Q and restart. It recovered both an unsaved creation draft and the context brief.
- Clipboard paste and an actual Markdown download preserved the full memory ID, namespace and body. The synthetic export was moved to temporary storage.
- A native remote connection through `RemoteState` and a real disposable MCP process loaded workspace B statistics as **6 total / 6 active / 0 archived**, plus 2 inbox entries.

## RedPen review dispositions

The three reports below contain 19 entries. Duplicate entries are mapped to the same disposition; eight distinct corrections were implemented.

- **A**: `job-73c5917769894e260afe881a5948244d777628e85ffa88db213e134694c254c7.md`
- **B**: `job-3e90084da8577ffcffbfffc4b64cb6de85ab8aa022f4348251ba0dc4b64e3484.md`
- **C**: `job-895a4961c2d92fd2acd6c58585553d2a3dbe5798376f63a458ea69c46390ada2.md`

Entry numbers follow each report's displayed finding order, including informational notes.

| Entries | Disposition |
|---|---|
| A1, B2, C1 | **Implemented:** typed draft-read outcomes reserve failed recovery bytes and block replacement. Retry and explicit discard handle failed removal without losing the original record. |
| A2 | **Implemented:** a same-ID fetch cannot replace a newer confirmed save. A control case preserves legitimate external refreshes when there is no local change. |
| B3, C2 | **Implemented:** palette handlers ignore `isComposing` and WebKit key code 229. |
| A3, B5, C6 | **Implemented:** attention titles obey the requested namespace as well as archive/expiry eligibility. |
| B1, C4 | **Implemented:** `ctxDelete` rejects `global`; the Tauri command rejects it before backup. The High severity claim overstated the existing risk: core already rejected a global purge. |
| B6 | **Implemented:** stored context blocks validate rendering/export fields, including text, content and tag-array types. |
| B7 | **Implemented:** future spool modification times use age zero while retaining the bucket count. |
| C7 | **Implemented:** the disposable fixture explicitly disables CORS and enables strict filesystem access. It remains outside the packaged application. |
| B4, C3 | **Skipped:** expiry, clearing on hide and excluding changed content would discard the approved restart-recovery data. Slots are bounded and clear on confirmed save/discard. Drawer deletion flushes first; a context brief is an independent snapshot. Encryption needs a separate key-management and storage contract; no encryption-at-rest claim is made. |
| A4 | **Skipped:** spool roots come from operator configuration (`CLIO_CAPTURE_SPOOL`) or the local home directory, not an IPC caller argument. The report identifies a hypothetical future boundary change, not a current caller-controlled path. |
| C5 | **Skipped:** both scoped query branches apply the archive filter, and the UI also checks each row. The response marker is a compatibility contract, not a security attestation; changing its derivation adds no protection against a dishonest backend. |
| A5 | **No change:** the report found no injection vector. Captured text uses Vue interpolation/`<pre>`; no `v-html` was introduced. |

The final independent review approved the corrections with no remaining valid blocker; its 68 focused UI and 6 Rust regressions passed. Final combined verification passed:

- From `ui/`, Herd Node 22 ran `node --experimental-strip-types --test --test-reporter=spec src/composables/*.test.mjs src/utils/*.test.mjs`: **110 passed, 0 failed**.
- Full core suite: **305 passed** (200 unit, 77 integration, 10 connection, 11 repair and 7 worktree).
- `cargo test --locked --offline -p clio-tauri`: **9 passed, 0 failed**.
- Vue type checking: exit 0. Vite: **118 modules**, built in **997 ms**, exit 0. `git diff --check`: no errors.

All three reports were moved to `RedPenReviewQueue/reviews/implemented/` after source-hash and absent-destination checks. Verification reported **PASS all 3 handled review files are in implemented, unchanged; originals are absent**. SHA-256 values were unchanged. A fresh process check confirmed all task-started native app, bridge, MCP and Orca processes stopped. The requested review follow-up is complete.

## Verification limits and scope

Earlier verification was blocked by missing locked crates and DNS/network restrictions. A dependency request was cancelled, and a later retry could not resolve `static.crates.io`. These download and adapter-test blocks are now resolved: the authorised network continuation fetched the dependencies and passed the adapter checks above. The lockfile was not changed.

A disposable browser fixture was prepared, intercepting every Tauri command and using only synthetic memories/captures. The sandbox blocked a localhost listener; its approval-waiting attempt was cancelled before startup. Browser URL policy subsequently blocked opening a standalone local preview. The standalone preview builder/output was removed. That browser preview was not exercised.

CUA inspected and exercised the native window without changing Orca permissions. The first native run used incognito storage. A later persistent-profile run used two temporary patches to the exact dependency versions for UUID-based webview storage. Those verification-only patches did not change repository application configuration or the lockfile. The later run established creation-draft and brief recovery across native restart.

Launch the verification bundle only with a disposable database, isolated settings/spool and disabled providers. A default launch or double-click can load the usual production settings; the generated bundle is not a standalone demo launcher.

Cleanup confirmed that the verification app, Orca and both task-started computer-use helpers stopped. The process check reported **PASS no matching verification or Orca processes remain**. No browser server was started.

The reusable fixture is prepared for `node test/fixture-server.mjs` from `ui/`, using a compatible Node runtime in an environment authorised to listen on localhost. Startup and rendered behaviour remain unverified. Stop it with Ctrl-C after checks. It serves the normal application with synthetic command responses instead of the real memory backend.

UI regression tests exercise compiled component setup state with mocked IPC. Native testing adds rendering, SQLite persistence, selected restart recovery, export and remote-connection evidence. Native fault injection, editor/inbox restart recovery, exhaustive accessibility checks and the window's hidden duration were not exercised. These limits do not create additional work within the user's instruction to handle the three reviews and then stop.

The review-phase contract-check MCP process stopped and its disposable data was removed. Its native app, remote bridge, MCP and verification/control processes also stopped. Synthetic native fixture database/settings/logs and the export remain in temporary storage. Installation and deployment were outside that earlier review phase and were subsequently completed under the release authorisation above. Capture replay/purge, hook changes and memory repair remain outside this release. Existing capture-recovery work remains in the operational roadmap.

## New files

Existing source files were updated in place. The newly created deliverables are:

- `docs/reviews/2026-09-04-app-review.md` — original findings and approved implementation slices.
- `docs/reviews/2026-09-04-app-improvements.md` — this implementation and verification record.
- `crates/clio-core/src/capture_queue.rs` — metadata-only local queue diagnostics and unit tests.
- `crates/clio-tauri/src/commands/inbox.rs` — thin review adapters and remote response decoder tests.
- `scripts/verify-app-contracts.py` — real CLI/MCP contract checks with a disposable database, disabled providers and confirmed process cleanup.
- `ui/src/components/NativeDialog.vue` — shared native HTML dialogue.
- `ui/src/composables/draftStorage.ts` — bounded editor, creation and inbox recovery storage.
- `ui/src/utils/memoryKinds.ts` and `ui/src/utils/memoryPresentation.ts` — shared kinds and readable memory details.
- `ui/src/views/InboxView.vue` — capture review and suggestion recovery.
- `ui/src/composables/attention.test.mjs`, `browsing.test.mjs`, `contextBuilder.test.mjs`, `editorExit.test.mjs`, `inbox.test.mjs`, `memoryPresentation.test.mjs`, `paletteNavigation.test.mjs`, `quickCreate.test.mjs`, `useAutoSave.test.mjs`, `useKeyboard.test.mjs` and `workspaceShortcuts.test.mjs` — regression coverage, all in the same composables directory.
- `ui/test/setup.mjs` — component setup test renderer and disposable fixtures.
- `ui/test/fixture-backend.js` and `ui/test/fixture-server.mjs` — synthetic browser fixture, awaiting authorised runtime acceptance.

Ignored task briefs, implementation reports and progress notes remain under `.superpowers/sdd/2026-09-04-app-review/`. The original eleven review screenshots remain in the local review artefact outside the repository.

The generated, uninstalled native verification bundle is `target/debug/bundle/macos/Clio Verification.app`. It requires the isolated launch configuration described above. Synthetic native fixture database/settings/logs remain under system temporary storage.

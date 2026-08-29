# Live memory and namespace audit — 29 August 2026

## Status

This is a read-only audit of the live Atlas corpus. No memory, namespace,
attention item, link, archive state or database row was changed.

Local implementation has since added the fail-closed repair mechanism and
corrected Waypoint's projection source. Those changes are not installed on
Atlas. Manifest review and explicit apply approval remain mandatory before any
live schema or data change.

The corpus is live and may move slightly after the snapshot. Counts below are
from the complete paginated snapshot taken at 03:51 UTC on 29 August 2026.
The audit deliberately records aggregates rather than private memory bodies,
memory identifiers or personal filesystem paths.

## Scope and method

The audit covered every live and archived memory and cross-checked:

- all namespaces, kinds, sources, archive states and source references;
- recorded working directories against the current filesystem;
- Waypoint worktree projections against current worktree paths;
- current Git checkout roots under the usual project locations;
- exact duplicates, repeated titles, no-op receipts and expiry metadata;
- open attention items and their source memories;
- likely privacy and literal-secret markers using conservative heuristics;
- the built-in age-based cleanup dry run;
- the approved namespace repair design and the current Clio and Waypoint source.

Filesystem absence was not treated as sufficient evidence to archive a durable
fact or decision. A deleted worktree can contain knowledge that still belongs
to a current canonical repository.

## Corpus snapshot

| Measure | Count |
| --- | ---: |
| Memories | 9,844 |
| Active | 9,716 |
| Archived | 128 |
| Namespaces | 93 |
| Distinct recorded working directories | 78 |
| Active receipts | 1,854 |
| Open attention items | 74 |

Receipts account for about 19% of the active corpus. The largest source groups
are session capture sources, so quality depends heavily on capture filtering
and accurate namespace resolution.

## Findings

### 1. Retired worktrees have left active current-state projections

The concern about deleted worktrees is confirmed.

Waypoint has 124 active worktree projections. Of the 109 projections whose
content exposes a worktree path, 89 records refer to 88 paths that no longer
exist; only 21 refer to paths that still exist. The extra record is a second
projection for one missing path.

These 89 records claim to describe current worktree state. They are
high-confidence archive candidates because their derived subject no longer
exists. They are not durable project history.

Waypoint currently builds the Clio namespace as `project:<repository-slug>` in
`worktree_projection_service.rs` and only upserts the projection. There is no
corresponding Clio archive/deprojection operation when the worktree lifecycle
becomes archived. The writer therefore both fragments namespaces and leaves
retired projections active.

### 2. Temporary repository tests polluted the live corpus

There are 55 active projections in 29 temporary or synthetic repository
namespaces. They were created between 21 and 27 August 2026.

Fourteen are generic detached-HEAD test summaries that form the corpus's only
exact-content duplicate group. The other 41 overlap the missing-worktree set.
The union of obsolete worktree projections and test pollution is therefore
**103 active high-confidence archive candidates**, not 144.

Current Waypoint guidance and unit tests use a stub and explicitly forbid the
real Clio client. The existing live artefacts should be archived, and future
end-to-end or smoke paths must retain the same isolation guarantee.

### 3. A missing checkout does not make all its memories disposable

Twenty-seven recorded working-directory paths no longer exist, covering 195
active memories. Most should not be archived merely because the checkout was
removed:

- 71 records from an old Notes worktree include durable facts, decisions and
  receipts while the canonical Notes repository still exists;
- 54 records from three retired Modern Printworks worktrees belong in the
  canonical Modern Printworks namespace;
- old Clio worktrees contain durable facts and decisions while the canonical
  Clio repository still exists;
- eight RedPen records are code and security findings while the canonical
  RedPen repository still exists;
- several old paths represent renamed or relocated repositories rather than
  deleted projects.

This evidence rejects a blanket rule such as “archive every memory whose
working directory is absent”. The correct disposition is based on the subject
and current project identity: archive obsolete derived state; move durable
knowledge to its canonical namespace; review genuinely retired projects.

### 4. There are 84 high-confidence namespace move candidates

The approved canonical mappings produce this initial deterministic set:

| Existing namespace family | Canonical namespace | Active records |
| --- | --- | ---: |
| Three Modern Printworks worktree namespaces | `project:modernprintworks` | 54 |
| `project:qr-inbox` and `project:scanway-app` | `project:scanway` | 19 |
| `project:clio` | `clio` | 11 |
| **Total** |  | **84** |

The Clio merge remains subject to explicit approval and the same backup and
rollback gate as every other live mutation.

A further 21 live Waypoint projections use repository-derived namespaces.
Their canonical targets should be resolved after the Waypoint writer is fixed,
otherwise a later checkpoint can recreate the fragmentation.

### 5. Large ambiguous namespaces need evidence-led review

`project:project` contains 240 active memories produced from a shared parent
directory. A conservative content classifier found one plausible project in 74
records, several plausible projects in 36, and no reliable project signal in
130. Content-only classification is not strong enough for automatic mutation.

The `global` namespace contains 719 active memories. Some have project-looking
working directories, but global placement can be intentional. These must also
remain unchanged until repository, marker or explicit user evidence resolves
them.

There are 418 active records in resolver-artefact namespace families overall.
This is a routing-quality problem, not a safe bulk-move count.

### 6. Session noise is substantial but not mechanically disposable

The audit found:

- 167 active receipts that appear to record no substantive outcome;
- 60 repeated-title groups whose contents differ;
- one exact duplicate group containing 14 test records, or 13 excess copies;
- no duplicate active `(source, source_ref)` pairs;
- no active records past a populated `valid_until` value;
- 431 active records without working-directory metadata.

The 167 no-op receipts are a focused manual archive queue. Receipt type, age or
title repetition alone is not sufficient: a receipt may be the only durable
record that a safety check ran or that a task was deliberately deferred.

The built-in one-month cleanup dry run proposed 118 records across five
namespaces. Several corresponding repositories still exist, so those results
were rejected as an archive plan. Age alone is not evidence of obsolescence.

### 7. Attention state needs a separate stale-work audit

There are 74 open attention items. Thirty-seven are older than 14 days, 15 are
older than 21 days, 28 have no completion condition and two reminders are
overdue.

An attached memory must not be archived without separately deciding whether
its attention item is complete, cancelled, superseded or still open. This
requires project-by-project verification rather than content heuristics.

### 8. Privacy review is still required

A narrow literal-secret heuristic found no obvious private-key blocks, common
API-key formats, credential-bearing URLs or simple secret assignments. This is
reassuring but is not a complete secrets guarantee.

A broader privacy-language review produced 350 candidates involving Chronicle,
browsing, screenshots, OCR or clipboard language. Many are legitimate visual
QA records or warnings about privacy rather than private content. They need a
private body-level review and must not be copied into a version-controlled
manifest.

## Safe disposition preview

| Disposition | Count | Confidence | Action |
| --- | ---: | --- | --- |
| Archive obsolete Waypoint/test projections | 103 | High | Apply only through the reviewed atomic repair path |
| Move approved namespace families | 84 | High | Preserve history and links; Clio merge needs explicit approval |
| Canonicalise active Waypoint projections | 21 | High after target resolution | Fix the writer first, then include in the repair manifest |
| Review apparent no-op receipts | 167 | Medium | Inspect by project; do not bulk archive by title or age |
| Review open attention | 74 | Medium | Resolve attention state before touching source memories |
| Classify `project:project` | 240 | Mixed | Use repository or marker evidence; preserve ambiguous records |
| Classify `global` | 719 | Mixed | Preserve intentional global knowledge |

Counts in this table are categories, not a sum: some records can appear in more
than one review category.

## Mutation gate

No live cleanup should be applied through a sequence of ordinary `clio move`
or per-record archive calls. The approved design requires:

1. fix and verify Waypoint's canonical namespace and retirement behaviour —
   complete locally, not installed;
2. implement Clio's history-preserving audit, repair journal and conditional
   rollback path — complete locally, not installed;
3. verify linked recall excludes archived or expired memories — covered by the
   full green workspace test run;
4. take an online Atlas backup and require `PRAGMA quick_check = ok` — complete
   for the private proposal;
5. generate a complete private manifest tied to that backup and review its
   aggregate counts — complete and rehearsed below;
6. obtain explicit approval for the reviewed schema and data mutation set;
7. install the repair-capable CLI and apply the complete set in one transaction
   with compare-and-swap checks;
8. run post-repair recall, link, namespace and rollback canaries.

Until the gate is deployed, reviewed and explicitly approved, the audit is the
disposition preview. It must not be treated as permission to change live data.

## Local implementation evidence

The first two gate prerequisites are now implemented locally:

- migration `015_memory_repair_journal` adds immutable transaction and journal
  tables, closed journals and a database-wide generation for cross-process
  namespace cache invalidation;
- `clio repair manifest` builds against a validated online backup without
  migrating the live database;
- `clio repair apply` and `clio repair rollback` require exact confirmations,
  take fresh backups and compare the repair-relevant memory snapshot, complete
  link graph and every target attention row; first installation of the repair
  schema and the data repair share one transaction, including on non-WAL
  databases; rollback compares every repaired target and the complete
  touching-link set before one atomic transaction;
- journal export opens SQLite read-only, while private JSON outputs use an
  atomic no-clobber publication and cannot replace a concurrent destination;
- namespace repairs preserve semantic `updated_at`, content, tags, embeddings
  and occurrences; rollback is itself journalled;
- Waypoint now receives the canonical namespace from an explicit override or
  `.clio-namespace`, retains the repository slug only as provenance and archives
  an archived worktree's projection by stable source reference;
- focused tests use only isolated databases and a stub Clio executable.

A fresh pre-manifest read found the candidate counts unchanged: 124 active
Waypoint projections, 21 current paths, 88 missing paths, 103 unique archive
intents after synthetic-test overlap, and 84 approved move intents. The private
intent file is outside Git. This remains proposal evidence, not mutation
authority.

## Private Atlas proposal evidence

A fresh online Atlas backup was taken for the proposal and copied to a private
local directory. The remote and local files matched by SHA-256 and byte count,
both copies had mode `0600`, and `PRAGMA quick_check` returned `ok`. The source
backup is 83,718,144 bytes with SHA-256
`e75717a012ed3a852539e9424b8b6426d61e0a3a3308f9b42768a0631e9f9d8a`.
The temporary remote copy was removed after verification. Every regenerated
repair snapshot, manifest and journal also has mode `0600`.

The resulting private manifest has digest
`ef2954a1e29e958a93770c02189371813a89f37b9245e6a4c755ba4a5b4de80c` and
contains:

- 187 unique targets: 103 archives and 84 namespace moves;
- no attached attention changes;
- all 246 automatic links touching a target;
- 193 automatic links that would become invalid and must be removed;
- no human-authored link in the touching set.

The exact manifest was applied to a disposable copy of the validated backup.
All 187 after-states matched, all 193 planned links were absent, the database
remained healthy and the immutable forward journal contained 380 entries. A
conditional rollback then restored all 187 before-states and all 246 touching
links exactly, recorded a separate 380-entry rollback journal and again passed
`PRAGMA quick_check`.

A final read-only Atlas check still reported migration `014_checkpoint_usage`,
no repair tables, 9,850 memories, 9,722 active, 128 archived and
`PRAGMA quick_check = ok`. No live repair schema or data mutation has occurred.
The private manifest remains proposal evidence pending explicit approval.

## Proposed first live repair slice

The reviewed first slice contains only the 103 archive candidates and 84
approved namespace moves. The ambiguous namespace, receipt, attention and
privacy queues remain excluded and should follow as separately reviewed slices.

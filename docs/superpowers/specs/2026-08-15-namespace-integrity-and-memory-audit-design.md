# Namespace Integrity and Memory Audit — Design Spec

**Date:** 2026-08-15
**Status:** Approved direction → review findings incorporated
**Author:** Danny Harding

## Goal

Make Clio namespaces trustworthy at write time, repair the existing Atlas corpus
where project ownership can be established confidently, and produce a complete
audit of records that cannot safely be repaired automatically.

Namespace correctness is a data-integrity requirement. A project memory must not
silently become `global`, `project:unknown`, or a namespace derived from a branch
or worktree folder. Genuine global memories remain supported through an explicit
global choice.

## Evidence and current impact

The read-only Atlas audit on 2026-08-15 found:

- 10,264 memories across 143 namespaces;
- 646 memories in `global`;
- 124 memories in `project:unknown`, all from root-CWD Claude sessions;
- 598 root-CWD Claude memories stored as either `global` or `project:unknown`;
- project families fragmented by linked-worktree names, including Modern
  Printworks and Scooda;
- 77 Waypoint worktree projections using repository-derived namespaces such as
  `project:github.com-bemanza-scooda` and `project:local-repo-*`;
- 2,524 memories older than 30 days, 145 archived memories, and one explicitly
  expired memory.

Age is not evidence that a memory is obsolete. Staleness and namespace ownership
will therefore be audited separately.

## Root causes

### 1. Model-selected global outranks deterministic context

Classified capture currently resolves an explicit override first, then accepts a
model-selected `global`, and only then considers the namespace detected from the
working directory. This allows probabilistic model output to override reliable
project evidence.

When there is no detected project, the same path accepts arbitrary model output,
including `project:unknown` and fabricated project names.

### 2. Linked worktrees use their checkout folder names

Automatic context detection recognises a `.git` marker but derives the namespace
from the directory containing that marker. In a linked worktree this is commonly
a branch or task folder such as `mod-68` or `develop`, not the repository's stable
identity.

### 3. Hooks defer namespace resolution

The capture queue records `cwd`, but normally leaves the namespace unset until a
later drain invokes `clio checkpoint`. A root working directory provides no
project evidence. A deleted or renamed worktree can also remove evidence that was
available when the event was captured.

The live installed hook and CLI package also differs from the tested OPS-008
package. Installation must reconcile those packages; it must not overwrite the
live hook tree blindly.

### 4. Waypoint bypasses its canonical project namespace

Waypoint worktree projections construct a Clio namespace directly from the
repository ledger slug instead of using the project's canonical Clio namespace.
Repository identity is useful provenance, but it must remain metadata rather than
becoming a second project namespace.

## Namespace authority

All automatic write paths will use one authority order:

1. an explicit namespace supplied by the caller;
2. a non-global `.clio-namespace` marker bound to an approved repository identity
   detected from the original working directory;
3. an approved stable repository identity detected from the original working
   directory when no marker is present;
4. unresolved context, which is held for review rather than stored automatically.

The classifier may suggest a namespace for review, but cannot override steps 1-3
or create a namespace automatically. `global` is valid only when explicitly
selected by the caller or approved from the review queue.

A marker is repository-controlled input, not authority by itself. The inspected
checkout cannot add or override its own identity binding: bindings come from
operator-controlled Clio settings. A marker is trusted only when the checkout's
canonical repository identity is already approved and every matching identity
alias maps to the marker's exact namespace. A marker containing `global`, a marker
outside the detected repository, an unbound marker or a marker that conflicts
with any approved identity makes the context unresolved.

The MCP, CLI, checkpoint and capture defaults must retain identical semantics.

## Stable repository detection

For a normal checkout, repository identity comes from its Git root. For a linked
worktree, detection must follow the `.git` file to the shared Git metadata and
derive one stable repository identity rather than using the worktree directory.

A repository-bound `.clio-namespace` can select a canonical name that deliberately
differs from the repository name. Detection must handle missing, malformed and
inaccessible Git metadata without inventing a namespace or trusting the checkout
being inspected to approve itself.

Repository detection is deterministic and never turns a repository or directory
name directly into a namespace:

1. Canonicalise the original working directory to a physical absolute path. Locate
   the nearest `.clio-namespace` as an untrusted candidate; reject an unreadable
   marker, an empty or multi-line value, `global`, or a value that fails Clio's
   namespace validation. A candidate outside the repository root found below is
   not eligible for automatic writes.
2. Ask Git for the worktree root, Git directory, shared common directory and
   configured remotes. Resolve relative Git-directory pointers against the file
   that contains them, then canonicalise all local paths physically. A malformed
   `.git` file, an inaccessible target or contradictory Git output makes the
   context unresolved.
3. Canonicalise remote identities by parsing URL and SCP forms, removing user
   information, query and fragment data, default ports, trailing slashes and one
   terminal `.git`, and lower-casing the scheme and host. Repository path matching
   remains case-sensitive. Local identities use only canonical absolute Git
   common-directory or repository-root paths; basenames are never identities.
4. Resolve the collected identity keys only through an approved alias map. Alias
   keys are `remote:<canonical-remote>`, `git-common-dir:<canonical-path>` or
   `repository-root:<canonical-path>`, and each maps to one validated canonical
   Clio namespace. Remote aliases belong in version-controlled Clio deployment
   settings; machine-specific path aliases belong in operator-controlled local
   settings. Neither source is loaded from the checkout being inspected. Linked
   worktrees share the same common-directory key and therefore the same mapping.
5. If a marker candidate exists, require it to be inside the repository and to
   equal the single namespace produced by all matching approved aliases. A marker
   cannot introduce a namespace or override an identity binding.
6. Exactly one namespace may result. No match, multiple namespaces, an alias
   collision, conflicting repository evidence or a marker conflict is unresolved
   and enters review. A marker, remote, folder, worktree or repository name is
   provenance only unless approved identity bindings establish its namespace.

Initial canonical decisions are:

| Project family | Canonical namespace | Basis |
| --- | --- | --- |
| Clio | `clio` | Approved repository identity bound to the tracked marker |
| Scooda | `project:scooda` | Approved canonical namespace |
| Modern Printworks | `project:modernprintworks` | Existing dominant project namespace |
| Foreverly/Knotbook | `project:foreverly` | Approved identity bound to the tracked marker |
| ScanWay | `project:scanway` | Approved identity bound to the tracked marker |
| Stuntrocket v3 | `project:stuntrocketv3` | Existing dominant project namespace |
| Notes/Waypoint | `project:notes` | Existing project namespace |

Other project families require repository or marker evidence before a canonical
mapping is applied. Folder names alone are insufficient.

## Capture and hook changes

Classified capture will distinguish a resolved namespace from a classifier
suggestion. If deterministic context is unresolved and there is no explicit
namespace, the capture is queued in the inbox with its suggested namespace and
reason rather than written to a fabricated or global scope.

The hooks will resolve and persist the namespace when an event is enqueued, while
the original working directory still exists. Queue draining will forward that
resolved namespace explicitly. Existing queued payloads without a namespace must
remain readable and follow the new unresolved-review behaviour.

The Mac CLI, Atlas binary and hook package must be deployed as one compatible
change. AI clients must be restarted before a normal Stop-hook canary is treated
as acceptance evidence.

## Waypoint correction

Waypoint projections will receive the canonical Clio namespace from their project
context. The repository ledger slug will remain in projection metadata and tags
for provenance and lookup. Existing repository-derived Waypoint records will be
included in the Atlas repair manifest.

This is an adjacent-repository change. Its working tree and instructions must be
inspected before editing, and unrelated changes must be preserved.

## Full corpus audit

The audit will examine every memory, including archived records, and produce:

- a dated, version-controlled summary at
  `docs/operations/audits/2026-08-15-memory-namespace-audit.md`;
- a private machine-readable manifest containing memory ID, original namespace,
  proposed namespace, original `updated_at`, confidence, evidence category and
  disposition;
- aggregate before-and-after counts by namespace, source, kind, age and archive
  state;
- an explicit unresolved ledger for memories that require human judgement;
- a rollback manifest for every applied mutation.

The private manifests must not contain transcript bodies, credentials, clipboard
content or other unnecessary private material. Evidence will be represented by a
bounded category and path or source reference where safe.

### Confidence rules

**High confidence — eligible for automatic repair**

- a non-global `.clio-namespace` marker bound to an approved repository identity;
- a canonicalised transcript or source path within a known project family,
  corroborated by a marker or approved repository identity, or by another
  independent project-unique signal;
- a stable repository/worktree identity mapped to an approved canonical namespace;
- two independent, project-unique signals such as a ticket prefix plus repository
  metadata.

**Likely — report, do not mutate automatically**

- content, tags or titles strongly suggest one project but lack independent
  provenance;
- a repository slug has more than one plausible canonical project;
- global records appear project-specific but their source session is unavailable.

**Ambiguous — preserve unchanged**

- cross-project or genuinely reusable material;
- records with conflicting evidence;
- records with no trustworthy ownership evidence.

## Repair mechanics and rollback

Before mutation, create an online Atlas backup and require
`PRAGMA quick_check = ok`. Generate and review the dry-run manifest against that
exact database snapshot.

Apply the reviewed manifest in one write transaction. Before changing anything,
compare every targeted memory's current namespace and `updated_at` with the
manifest's original values and compare any archive state that the transaction will
change. The dry-run manifest also records the complete, deterministically ordered
set and state of every link touching each targeted memory, including links not
proposed for mutation. Within the transaction, re-read and compare those complete
link sets. Any added, removed or changed link, or any other mismatch, is a
conflict: abort the whole transaction without partial repairs and emit a conflict
report for a fresh dry run. The audit watermark still identifies records created
after the snapshot, but it is not a substitute for these complete per-record
compare-and-swap checks.

Namespace-only repairs must preserve memory IDs, content, tags, embeddings,
occurrences, archive state, creation time and original semantic `updated_at`.
Existing `clio move` changes `updated_at`, so the repair path needs an explicit
history-preserving core operation rather than ad-hoc SQL or ordinary move
semantics.

Automatic links touching moved records will be checked for cross-namespace
relationships. Invalid `auto:relates_to` links may be removed and regenerated
after the move; human-created relationships must never be removed.

The repair must be idempotent. A second dry run against the repaired database must
propose no already-applied move. The rollback manifest records every durable
mutation, not only memory namespaces and timestamps. Each entry contains the
entity type and stable ID, complete before and after values, the forward
transaction ID and its evidence. This includes memory namespace and `updated_at`,
archive state, removed automatic links and regenerated automatic links. Cache
clearing is derived-state invalidation rather than a durable mutation; caches are
cleared again after rollback.

A new migration will add immutable repair transaction and mutation-journal tables.
The apply transaction writes one journal row for every planned durable mutation
inside the same SQLite transaction as the data changes. The forward transaction ID
is derived deterministically from the approved manifest digest and database
snapshot identity; journal entry IDs are derived from that transaction ID, entity
type, stable entity ID and operation ordinal. Unique constraints make a repeated
apply fail closed or replay the already committed result rather than duplicate it.
Each entry contains the complete before and after state and bounded evidence needed
for conditional rollback. Journal rows are never updated or deleted; rollback is a
new, linked journalled transaction.

The private rollback manifest is exported from the committed journal, not written
ahead of or alongside the database commit. A crash after commit but before export
therefore leaves the authoritative rollback record intact, and export can be
retried deterministically from the transaction ID. Apply must not report the
external manifest as available until that export has been written and verified.

Namespace-list cache correctness will use a database-wide, monotonically
increasing generation added by a new migration and bumped by triggers for every
memory insert, namespace update or delete. Each process stores the generation with
its cached namespace list and checks the current database generation before every
cache hit. A mismatch reloads the list; if the generation check fails, the process
queries SQLite directly or returns an error and never serves the cached value.
Processes start with an empty cache and load the current generation. Eager local
cache clearing after apply and rollback may reduce latency, but it is best-effort
and is not part of the correctness boundary.

Rollback runs as a conditional inverse transaction. It first verifies that every
affected entity still equals the recorded after-state. It restores removed links,
removes only unchanged links created by the repair, and restores memory and archive
fields to their before-state. A missing entity, changed entity or identifier
collision aborts the whole rollback and reports the conflict, preserving all
post-repair writes. Restoring the full backup is an offline disaster-recovery path
only: writes must have been paused or every post-snapshot write must be replayed
explicitly before service resumes.

No memories will be permanently deleted. Archiving is allowed only where the
staleness audit establishes durable evidence that a memory is noise, superseded or
factually obsolete; age alone is never sufficient. Uncertain archive candidates
remain in the report.

## Staleness audit

Each record will receive one of these audit dispositions:

- current and durable;
- historical but useful;
- superseded by an identified newer memory;
- contradicted by current authoritative evidence;
- session noise without durable project value;
- ambiguous or not assessed confidently.

Automatic archiving requires an identified reason and evidence. Receipts and old
records are not automatically noise. Contradictions and supersession must point to
the newer memory or authoritative project source.

## Test-driven implementation

Regression tests will be written before the production changes and will cover:

- model-selected `global` cannot override a detected project;
- `project:unknown` and fabricated suggestions cannot be stored automatically;
- unresolved automatic capture is queued for review;
- explicit `global` remains valid;
- a linked worktree resolves to stable repository identity;
- a non-global `.clio-namespace` is accepted only when bound to the approved
  repository identity;
- spoofed, unbound, `global` and identity-conflicting markers fail closed;
- malformed linked-worktree metadata fails closed;
- hook enqueue persists namespace and drain preserves it after the worktree is
  unavailable;
- legacy hook payloads remain compatible;
- Waypoint projections use canonical project namespace while retaining repository
  provenance;
- history-preserving repair retains timestamps and related data;
- stale dry-run manifests fail atomically when a memory, archive state or any link
  touching a targeted memory has been added, removed or changed;
- the mutation journal commits atomically with repair data and can regenerate the
  rollback manifest after an interrupted export;
- namespace-list caches reject stale generations across processes and after
  service restart;
- dry-run, apply and rollback manifests are deterministic and idempotent;
- rollback conditionally reverses namespace, link and archive mutations without
  overwriting post-repair changes;
- automatic links do not expose records across repaired namespace boundaries.

## Rollout and acceptance

1. Implement and verify Clio core, CLI and MCP changes.
2. Reconcile and verify the external hook package without overwriting unrelated
   live changes.
3. Implement and verify the Waypoint correction in its repository.
4. Deploy compatible binaries to Atlas and the Mac, install reconciled hooks, and
   restart affected clients.
5. Exercise real capture canaries from a normal checkout, a linked worktree, `/`,
   and an explicit global request.
6. Take the Atlas backup, generate the complete dry-run audit and inspect aggregate
   proposed changes.
7. Apply only high-confidence repairs and evidence-backed archive operations.
8. Run integrity checks, search/recall canaries for major namespaces, link-boundary
   checks and an idempotent second dry run.
9. Update the operational roadmap with exact deployment and audit evidence.

## Success criteria

- Automatic project captures cannot be overridden by model namespace output.
- Unresolved captures enter review instead of becoming global or unknown.
- Linked worktrees resolve consistently across branches.
- Waypoint writes the same canonical namespace as other clients for a project.
- Every Atlas memory appears in the audit manifest with an explicit disposition.
- Every applied mutation has high-confidence evidence and a tested rollback path.
- No content, embedding, archive state, tag, occurrence or human relationship is
  lost during namespace repair.
- Atlas passes `quick_check`, major project recall canaries and cross-namespace
  link checks after repair.
- A second audit run is idempotent and contains no `project:unknown`, worktree-name
  or repository-slug assignment that can be repaired confidently.

## Non-goals

- Replacing SQLite or the Atlas shared-memory architecture.
- Introducing a general-purpose project-management taxonomy.
- Permanently deleting memories.
- Automatically deciding ambiguous ownership from model output.
- Re-embedding unchanged memory content solely because its namespace moved.

## Risks

- A stable Git repository name can still differ from the desired project name;
  explicit markers and approved aliases remain necessary.
- Root-CWD transcripts may contain insufficient evidence for safe reassignment;
  these must remain unresolved rather than being guessed.
- Namespace moves can invalidate inferred links unless link boundaries are audited
  in the same operation.
- Updating only Atlas, only the Mac CLI or only the hooks would leave inconsistent
  behaviour and could recreate bad records during rollout.
- The corpus is live. Backup, dry run and apply must operate against identified
  snapshots, with new writes either paused briefly or recorded after the audit
  watermark for a follow-up pass.

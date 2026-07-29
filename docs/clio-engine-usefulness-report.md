# Making Clio a dependable workflow memory

**Brainstorm report — 29 July 2026**

## Executive conclusion

Clio already has strong storage foundations: namespaced memories, full-text and
semantic retrieval, typed links, review queues, consolidation, MCP tools and a
desktop client. Its main limitation is not a lack of another search algorithm.
It is the missing dependable loop between:

> something important happened → it was captured → its state stayed current →
> it appeared at the right moment → the user acted on or resolved it

Today, each part exists in some form, but the joins are weak. In particular:

- session capture can be lost permanently after a transient failure;
- the capture prompt actively discourages in-progress state, which includes many
  useful follow-ups;
- tasks, unresolved questions and reminders have no explicit lifecycle;
- links are mostly similarity associations rather than statements about truth;
- consolidation lacks enough provenance to distinguish current, superseded and
  disputed knowledge safely;
- automatic surfacing does not reserve space for open work or explain why an
  item appeared;
- access tracking rewards memories merely for being injected, creating a
  feedback loop in which already-surfaced memories remain artificially fresh.

The best direction is to make Clio a **memory and attention layer**, not another
general-purpose task manager. Things, Linear and Cadence should remain the
operational systems for doing work. Clio should remember why the work exists,
notice that it is still open, carry its context between tools, and verify that
handoff to the operational system succeeded.

The recommended order is:

1. make session capture durable, retryable, observable and safe;
2. add a narrow lifecycle for follow-ups and unresolved questions;
3. surface a deterministic resume brief in both Claude and Codex;
4. make retrieval and links state-aware and explainable;
5. rebuild consolidation from cited, current source atoms;
6. add verified delivery to Things and Linear and a desktop “Needs attention”
   view.

## What was inspected

This report is based on the current Rust core, CLI, MCP server, daemon, Tauri
client, Vue UI, installed Claude and Codex hooks, hook logs and metrics, and a
live read-only snapshot of the Atlas memory store. No existing project files or
live memories were changed.

The live snapshot contained:

| Signal | Observed value | Why it matters |
| --- | ---: | --- |
| Total memories | 3,330 | There is enough data for recall quality and lifecycle problems to matter. |
| Active memories | 3,290 | Most knowledge remains eligible for recall. |
| Embedding coverage | 99.97% | Missing embeddings are not the main bottleneck. |
| Links | 2,986 | Link quantity is already substantial; semantics and use are the next problem. |
| Receipts | 654 | Session activity is being captured frequently. |
| Tasks | 2 | Explicit follow-ups are effectively not being captured. |

Both task memories sampled described work which has since been implemented, but
they remain active because there is no completion lifecycle. The corpus also
contains obvious namespace splits, including both `clio` and `project:clio`.
That fragments recall, linking and consolidation even when the underlying
memories concern the same project.

The hook evidence is more urgent. The 30-day Stop metrics contain 1,029 records,
including 162 failures. A narrower Claude-only slice from 22–29 July contains
81 records: 80 failed and one distilled. The reporting script is out of date, so
these raw counts should not be treated as a polished service-level measure, but
the corresponding logs confirm repeated failures followed by “already claimed”
skips.

## The operating model Clio needs

Clio should distinguish three related but different planes:

| Plane | Purpose | Examples | Lifecycle |
| --- | --- | --- | --- |
| Knowledge | What is believed or decided | facts, decisions, constraints, preferences | current, superseded, disputed or archived |
| Evidence | Why Clio believes it and what happened | session receipts, source occurrences, implementation evidence | append-only |
| Attention | What must be revisited | follow-ups, unresolved questions, waiting-for items, review dates | open, snoozed, resolved or cancelled |

This avoids two common failure modes:

- treating every session receipt as durable project truth; and
- turning Clio into a second issue tracker with its own competing queue.

A follow-up can be represented as a memory because its content and rationale are
valuable history. A narrow one-to-one attention projection should carry its
mutable operational state. The minimum useful fields are:

- `memory_id`;
- `status`: open, snoozed, resolved or cancelled;
- owner;
- `due_at`, `remind_at` and `snoozed_until`;
- trigger type and value, such as next project session, branch, ticket or date;
- external system and stable external reference;
- completion condition and resolution evidence;
- `last_surfaced_at` and surface count.

This is deliberately not assignment, estimation, sprint planning or workflow
automation. Those remain in Things, Linear or Cadence.

## 1. Make capture lossless before making it clever

### Current failure mode

The installed Stop hooks create a permanent `.done` marker before transcript
validation, remote access and model distillation. If the provider returns a
429, Atlas is unavailable, a rollout appears late, or the model returns invalid
output, the session is still claimed. Later Stop events in the same session are
skipped too, so a follow-up added after the first assistant turn is never seen.

The model call and consolidation also run synchronously inside hook time limits.
That combines a slow network boundary, an LLM boundary and a remote database
boundary in a path which should finish quickly.

### Recommended capture pipeline

1. A hook writes a local capture job atomically and returns within two seconds.
2. The job identifies the agent, session, namespace and transcript cursor. Its
   idempotency key is derived from those values, not from a one-time `.done`
   marker.
3. A worker processes queued jobs serially, honours `Retry-After`, backs off,
   retries transient errors and retains terminal failures for inspection.
4. The successful transcript cursor advances only after every extracted atom
   has been stored transactionally.
5. A later Stop captures only the delta after that cursor. Session end can
   finalise the receipt without losing later turns or duplicating earlier ones.
6. The next session brief reports capture health: last success, pending jobs,
   oldest retry and dead-letter count.

Because Atlas is the system of record and client machines do not all run the
Clio daemon, the durable spool must begin on the client. Processing may happen
through a lightweight LaunchAgent, the Tauri app or a shared CLI worker; it
should not depend solely on the remote daemon being active on every client.

### Capture at the moment of commitment as well

Stop-time distillation should be the safety net, not the only route. MCP
instructions should tell agents to record an explicit statement immediately
when the user says, for example:

- “We have decided …”
- “Remind me to …”
- “I need to follow this up …”
- “Do this after the deployment …”
- “Leave this open until Huzaifah confirms …”

Immediate capture gives explicit commitments the strongest reliability.
Asynchronous session distillation then finds implicit or missed items.

### Prompt changes

The current prompt says not to capture transient or in-progress work, then only
mentions unfinished work inside a low-importance receipt. Keep the protection
against routine narration, but add a deliberate exception:

> Extract every explicit unresolved commitment, requested follow-up, deferred
> verification, named decision required, blocker, waiting-for item or promised
> next action as a separate attention item, even though it is in progress.

The structured output should separate:

- durable knowledge;
- accepted decisions and rationale;
- explicit open follow-ups;
- unresolved questions;
- blocked or waiting-for items;
- resolutions, cancellations and supersession evidence;
- one receipt only when substantive work occurred.

The model should not convert “could”, “might” or general suggestions into
user-owned work. Explicit commitments can be stored automatically. Inferred
actions should enter the existing review inbox unless confidence is high and the
configured policy allows automatic capture.

Ticket, pull request and branch identifiers should be copied deterministically
from the transcript and applied to every relevant atom, rather than relying on
the model to remember the `ticket:<id>` convention.

### Security boundary

Redaction must run on the actual provider payload before it is spooled or sent.
At present, Codex redaction is applied to a diagnostic rendering rather than the
real distillation input, while Claude has no equivalent shared pass. Tool
arguments and outputs can contain credentials, tokens, environment data or
customer information. A more automatic Clio must have a stronger, not weaker,
privacy boundary.

## 2. Give follow-ups a real but narrow lifecycle

### What should become an attention item

Capture:

- a user commitment;
- a promised agent action which cannot be completed in the current session;
- a named decision still required;
- a deferred verification which affects confidence or release readiness;
- a blocker or dependency on another person;
- a deliberate “revisit after X” statement;
- an external task or issue whose context needs to follow the work.

Do not capture:

- completed steps from the current session;
- speculative improvements with no commitment;
- every review observation;
- routine test, build or file-edit narration;
- duplicate tasks already linked to the same external item.

### Resurfacing policy

An open item should be eligible when any of these is true:

- its due or reminder time has arrived;
- the current project, branch, ticket or user prompt matches its trigger;
- it has never been surfaced since capture;
- it is blocked and new evidence about the blocker has appeared;
- it is important and has been dormant beyond a configurable interval.

Eligibility is not permission to repeat it endlessly. Surface once per task
topic or session, then require an explicit change, a stronger match or a due
event before showing it again. Always offer Complete, Snooze, Cancel and Send to
Things/Linear. Every reminder should say **why now**.

An example reminder should look like:

> **Open follow-up — due today:** Verify the live callback after deployment.
> Captured from session `…` on 28 July because staging tests could not prove the
> production callback. Linked to `SCOODA-1234`. [Complete] [Snooze] [Open issue]

That is more trustworthy than a memory appearing merely because its embedding
is nearby.

### Completion and audit

Completion should not delete or silently rewrite the original memory. Store a
resolution event or memory, link it with `resolved_by`, and remove the item from
active attention views. If Linear or Things is the operational source, mirror
its stable ID and last verified state. If sync fails, leave the Clio item open
and show the delivery failure.

## 3. Treat decisions as a history, not a mutable paragraph

Decision capture should require:

- the decision itself;
- rationale;
- owner or decision authority when known;
- decision date;
- alternatives explicitly rejected, when stated;
- evidence or source occurrence;
- status derived from links: current, proposed, superseded, reversed or
  disputed.

A later decision should create another memory and a `supersedes` or `reverses`
link. It should not overwrite history through an upsert. A contradictory claim
should create a visible unresolved conflict; an LLM-generated consolidation
must never silently decide which is true.

`decision-history` should therefore render a chain:

> Proposed → accepted → implemented → superseded

with dates, reasons and source references. The current decision can lead normal
briefs while the history remains available for audit.

## 4. Make linking describe meaning

### Current weakness

Automatic linking uses cosine similarity to create only
`auto:relates_to`. Suggested links are not scoped to the source namespace by
default. Recall expands outgoing links only, drops relationship metadata and can
fetch linked rows without reapplying the archived/expired filters. A
`new → old` supersession edge therefore behaves poorly in both directions, and
an archived memory can be reintroduced through link expansion.

Similarity is useful candidate generation. It is not a relationship.

### Recommended linking pipeline

1. Generate candidates from exact identifiers, source references, tags, FTS,
   embeddings and existing graph neighbours.
2. Default candidates to the same canonical namespace. Cross-project proposals
   must be explicit; global preferences may be considered separately.
3. Classify each candidate pair as one of:

   - `same_as`;
   - `supports`;
   - `contradicts`;
   - `supersedes` or `reverses`;
   - `evidence_for`;
   - `follow_up_of`;
   - `blocks`;
   - `resolved_by`;
   - `implements`;
   - `continuation_of`;
   - no link.

4. Store the model/version, similarity score, confidence and short rationale in
   link metadata.
5. Automatically accept only low-risk, high-confidence relations. Queue
   `same_as`, contradiction, supersession and cross-project links for review
   unless a deterministic key proves the relation.
6. Traverse incoming and outgoing links, preserving direction and relationship
   in the result.
7. Apply archive, expiry, completion and supersession rules after graph
   expansion as well as before it.

### Consolidating duplicates without losing evidence

The current exact-content deduplication can return the existing memory and lose
the fact that a second independent session confirmed it. Introduce an
append-only source-occurrence record so one canonical memory can retain every
supporting session, document or external reference.

For near duplicates:

- link as `same_as` first;
- show a source-cited synthesis preview;
- preserve all occurrences and relationships;
- merge automatically only when the content and state are genuinely
  equivalent.

Repeated confirmation can then increase confidence or `last_confirmed_at`
without creating noisy duplicate memories.

### Canonicalise namespaces before improving similarity

`clio` and `project:clio` should not be separate islands. Add namespace aliases
or a canonical identity at the capture boundary, and provide a reviewed
reconciliation command for existing data. Auto-linking and consolidation should
operate on the canonical identity. This is likely to improve relevance more
than fine-tuning embedding thresholds alone.

## 5. Rebuild consolidation as a cited derived view

The current consolidated memory is correctly treated as a derived cache, which
is a strong design choice. The weakness is its input. It receives kind, title,
content and importance, but not memory IDs, timestamps, validity, tags,
provenance, attention state or typed links. It is refreshed only after enough
new rows are created, not after updates, archives, resolutions or link changes.
Despite that, it leads the project brief at importance 5.

The consolidated output should have distinct sections for:

- current project summary;
- current decisions and rationale;
- active constraints;
- open follow-ups and unresolved questions;
- disputed or contradictory claims;
- recently superseded decisions;
- recent substantive activity.

Every material statement should cite its source memory IDs. The model may group
and deduplicate, but it must not invent a resolution for conflicting evidence.

Use a namespace mutation generation rather than “N rows created since the last
run”. Any source update, archive/unarchive, link change, attention-state change
or new occurrence marks the derived view stale. The brief should either rebuild
it or label/omit it until refreshed. Store the actual included count and a
truncation flag when the bounded input excludes sources.

Receipts should feed a separate activity digest. They should not compete equally
with decisions and constraints in the durable project summary.

## 6. Surface memory at the moments it can change the work

### A deterministic resume brief

Add a single core `resume-brief` policy, exposed consistently through CLI, MCP
and Tauri. Its order should be:

1. overdue and triggered follow-ups;
2. blocked or waiting items;
3. unresolved questions;
4. decisions changed since the last session;
5. active constraints;
6. branch, ticket and prompt-relevant knowledge;
7. concise recent activity;
8. capture-health warnings.

Reserve at least one slot for every non-empty critical section, then allocate
the remaining budget by relevance. The current fixed project allocation can use
five decision slots and five constraint slots before considering anything else;
with a small hook budget this can leave no room for open work. Empty sections
should also return their capacity to the rest of the brief.

Include applicable global preferences and constraints with a modest project
prior. Do not simply concatenate all project results before global results.

### Two automatic retrieval moments

1. **SessionStart:** project-level resume brief and capture health.
2. **First substantive UserPromptSubmit:** task-aware recall using the actual
   request, ticket identifiers, branch and current files.

SessionStart alone cannot know why the user opened the project. Prompt-aware
recall should run once per topic or material context shift, with a relevance
threshold and deduplication against the start brief.

Claude currently runs a basic `clio brief --max-items 12` hook rather than the
richer installed Python start hook. Codex has Stop capture but no automatic Clio
SessionStart recall. Both clients should call the same core policy through thin
adapters so their behaviour and metrics cannot drift independently.

### Retrieval should be allowed to abstain

For knowledge retrieval, union deterministic identifiers, FTS, semantic
candidates and graph neighbours, then fuse the rankings. Reciprocal-rank fusion
is sufficient at the current corpus size; there is no need for a new vector
database or approximate-nearest-neighbour infrastructure.

Apply:

- default filters for archived, expired, completed and superseded items;
- a minimum relevance threshold;
- topic diversification so one repeated cluster does not fill the brief;
- relationship-aware graph expansion;
- a project prior rather than an absolute project-first split;
- explicit “why surfaced” information.

An empty result is better than confidently injecting unrelated memory.

### Stop the automatic-access feedback loop

Automatic injection, explicit retrieval, opening, citing and acting are
different events. Do not increment the same `access_count` for all of them, and
do not use automatic delivery to refresh ranking age.

Track at least:

- retrieved by a deliberate query;
- injected automatically;
- opened or expanded;
- cited in an answer;
- acknowledged;
- acted on;
- snoozed, dismissed or resolved.

Only explicit usefulness signals should influence future ranking. Reports and
audits need an untracked read path so measuring Clio does not change the metric.

### Honour the output budget

Context assembly currently estimates size using a summary when one exists, but
JSON can still serialise the full memory content. The char budget therefore does
not reliably bound the payload. Budget the representation actually returned.
The consolidated singleton should expose useful cited content, not merely a
generic “AI-curated consolidation of N memories” summary or short preview.

## 7. Make MCP proactive without making it noisy

The MCP server instructions are part of the product. They should tell agents:

- request `resume-brief` automatically at project/task start;
- store explicit decisions and commitments immediately;
- checkpoint open loops before declaring work finished;
- use stable source references when updating existing actions;
- record completion evidence rather than deleting history;
- never turn an assistant suggestion into a user decision or task silently;
- report capture or external-delivery failure plainly.

The smallest coherent MCP surface would be:

- `memory_resume`: the deterministic task-start brief;
- `memory_action`: create, list, complete, snooze, cancel and route an attention
  item;
- `memory_session_checkpoint`: atomically store the decisions, attention items,
  resolutions and receipt extracted from one transcript cursor;
- relation-aware options on recall/context, including incoming links,
  relationship details, minimum score and tracked/untracked access mode.

The action logic belongs in `clio-core`; CLI, MCP, daemon and Tauri should remain
thin adapters over the same semantics.

## 8. Use the desktop app as the trust and attention surface

The most useful new view is **Today / Needs attention**, not a broader analytics
dashboard. It should combine:

- due and context-triggered follow-ups;
- waiting-for items;
- unresolved questions;
- disputed decisions;
- capture jobs pending retry or in dead letter;
- review-inbox depth;
- stale consolidated views.

Each row needs Complete, Snooze, Cancel, View evidence and Send/Open in external
system. A decision ledger should show the current decision and its
supersession/contradiction chain.

The present UI kind vocabulary is inconsistent across Home, quick create and
the kind selector; some views omit task, constraint, fact, receipt, preference
or process. The kind catalogue and labels should be defined once in the core
contract and reused by adapters. Link rows should show target title, direction,
relationship and rationale rather than an opaque shortened ID.

Revision history and notification state should be stored in the shared core,
not only in browser local storage, if they are to be trusted across Macs and
agents.

## 9. Hand actions to Things and Linear with proof

Clio should be the capture and context ledger. It should not silently become the
operational queue.

Suggested policy:

- user-owned personal or cross-project actions → Things;
- project work already associated with an issue → Linear/Cadence;
- unresolved context without an authorised external destination → remains an
  open Clio attention item;
- assistant suggestions → review first.

Delivery needs an idempotent outbox containing:

- stable delivery key;
- destination and requested payload;
- pending, delivered or failed state;
- external ID;
- attempt history and last error;
- read-back verification.

Mark delivery successful only after the destination can be read back. If Things
or Linear is unavailable, keep the Clio item open and surface the failure. When
the external item completes, mirror its status back by stable ID and attach
resolution evidence.

The adapter for Things should run in a logged-in user process such as the Tauri
app or a LaunchAgent. A sandboxed hook which fires a URL or AppleScript command
without read-back is not a trustworthy delivery path.

## 10. Trust indicators and evaluation

Volume, embedding coverage and link density do not answer whether Clio helped.
Add an append-only event ledger for capture attempts, storage, review,
surfacing, acknowledgement, completion, snooze, supersession, dismissal and
external delivery.

The first acceptance tests should be behavioural:

- a forced provider 429 retries and stores the checkpoint exactly once;
- a follow-up added after the first Stop event is captured;
- a non-git planning session can capture a decision and a follow-up;
- an archived or expired linked memory never re-enters default recall;
- an explicit commitment appears in the next eligible session or due digest;
- a completed task no longer appears as open but retains its history;
- every automatic reminder shows its source and reason;
- a failed Things/Linear handoff remains visible and retryable;
- redacted secrets never reach the provider payload or spool;
- running the effectiveness report does not alter retrieval ranking.

Track:

- oldest pending capture job and retry/dead-letter count;
- capture success and processing latency;
- precision and recall against an annotated transcript set containing explicit
  decisions, follow-ups, blockers and resolutions;
- due reminders delivered at the next eligible surface;
- false-positive dismissal rate;
- surfaced-to-cited and surfaced-to-acted-on rates;
- stale open-item and stale-consolidation counts;
- unresolved contradiction count;
- cross-namespace auto-link rate;
- duplicate and no-op receipt rate.

Set numerical targets after a baseline run. The immediate non-negotiable target
is simpler: **no capture attempt may disappear without either durable success,
a pending retry or a visible terminal failure**.

## 11. Architectural candidates

These are shallow discovery candidates, not requests to introduce abstractions
immediately.

| Candidate seam | Current scattering | Impact | Risk | Recommendation |
| --- | --- | --- | --- | --- |
| Session ingestion | core capture, CLI transport and external Claude/Codex scripts each own lifecycle decisions | High | Medium | Do first: one core checkpoint/job contract with thin hook adapters |
| Attention policy | task metadata, hook prompts and context assembly would otherwise each define “open” differently | High | Medium | Add the narrow projection once capture jobs are durable |
| Context policy | allocation and rendering differ across core, CLI, MCP and hooks | High | Low–medium | Centralise `resume-brief`; adapters request, not recreate, it |
| Graph semantics | repository, embeddings, consolidation and UI each lose or reinterpret relationships | High | Medium | Introduce relation-aware core operations before richer auto-linking |
| Kind catalogue | prompts, MCP descriptions and Vue selectors use different lists | Medium | Low | Centralise descriptors; keep custom kinds possible |

The session-ingestion seam is the first useful boundary. It removes duplicated
failure handling and makes subsequent attention features safe. There is no need
for a generic workflow engine, provider framework or plugin architecture.

## 12. Suggested delivery slices

### Slice 1 — Trust capture

Deliver:

- durable client spool;
- transcript cursors and idempotency;
- async retry/dead-letter handling;
- provider-payload redaction;
- capture-health output;
- one shared hook entry point and `clio hooks doctor`;
- corrected, non-mutating metrics.

Finished when a continued session, transient outage and non-git session all
retain their capture state without duplicates or silent loss.

### Slice 2 — Never lose an open loop

Deliver:

- structured action/resolution extraction;
- narrow attention lifecycle;
- `memory_action` and `memory_session_checkpoint`;
- deterministic `resume-brief`;
- Claude and Codex SessionStart plus first-prompt integration;
- Complete, Snooze and Cancel controls.

Finished when an explicit follow-up is automatically captured, shown at its
next eligible moment with a reason, and stops appearing after evidenced
completion.

### Slice 3 — Explainable project truth

Deliver:

- canonical namespace aliases;
- bidirectional, relation-aware graph recall;
- archived/expired/superseded safety;
- source occurrences for duplicate evidence;
- state-aware, cited consolidation;
- decision history and conflict review.

Finished when every consolidated statement can be traced to current source
memories and a superseded or disputed claim cannot masquerade as current truth.

### Slice 4 — Workflow handoff

Deliver:

- Today / Needs attention view;
- review inbox and decision ledger;
- verified Things and Linear outboxes;
- external completion mirroring;
- event-backed usefulness reporting.

Finished when Clio can prove that a user-owned follow-up reached its configured
system, retains the context which created it and notices when it is completed.

## Deliberate non-goals

Do not build these yet:

- a full task manager, scheduler, sprint board or assignment system;
- a new vector database or approximate-nearest-neighbour index;
- automatic merging of decisions or contradictory claims;
- unrestricted cross-project auto-linking;
- automatic external issue creation for speculative suggestions;
- a large analytics dashboard before capture and reminder events are reliable;
- more model-provider abstraction solely for this work.

The current corpus is small enough for straightforward SQLite queries and
bounded in-memory ranking. Reliability, lifecycle semantics and evidence will
produce more value than scaling infrastructure.

## Product decisions needed before implementation

1. Should high-confidence, explicitly user-owned actions be sent to Things
   automatically, or shown for one-tap approval first?
2. Should undated actions resurface on the next relevant project session, in a
   daily digest, or both?
3. How long may the encrypted/redacted local capture spool retain transcript
   fragments after successful distillation?
4. Should cross-project links default to review-only, with global preferences as
   the sole automatic exception?
5. Which system is authoritative when a Clio attention item and its external
   Things/Linear item disagree?

## Evidence pointers

Repository evidence:

- capture exclusions and receipt rule:
  `crates/clio-core/src/capture.rs:123–143`;
- memory fields:
  `crates/clio-core/src/models.rs:6–25`;
- fixed project-brief allocation:
  `crates/clio-core/src/assembly.rs:274–335`;
- automatic access tracking and graph expansion:
  `crates/clio-core/src/repository.rs:546–575` and `:667–711`;
- outgoing-only link API:
  `crates/clio-core/src/repository.rs:1395–1434`;
- consolidation input and creation-only freshness:
  `crates/clio-core/src/consolidate.rs:46–125`;
- similarity-only auto-linking:
  `crates/clio-core/src/embeddings.rs:854–1090`;
- current schema and relationship vocabulary:
  `docs/reference/schema.md`;
- active MCP contract:
  `docs/reference/mcp-contract.md`.

Installed workflow evidence:

- Claude Stop claim and capture:
  `~/.claude/personal-skills/clio-hooks/scripts/session_stop.py`;
- Codex rollout parsing and Stop capture:
  `~/.claude/personal-skills/clio-hooks/scripts/codex_stop.py`;
- richer but unregistered Claude start path:
  `~/.claude/personal-skills/clio-hooks/scripts/session_start.py`;
- active simple Claude start:
  `~/.claude/hooks/clio-session-recall.sh`;
- Codex hook registration:
  `~/.codex/hooks.json`;
- stale effectiveness reporting:
  `~/.claude/personal-skills/clio-hooks/scripts/clio_report.py`.

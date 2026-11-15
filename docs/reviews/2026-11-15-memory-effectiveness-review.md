# Memory effectiveness review — 15 November 2026

A review of how well Clio stores and retrieves memory for the agents that use
it, followed by two rounds of changes. Two models did the work: Claude Fable 5.2
and Codex (GPT-6 Astra), both at high effort. Each reviewed Clio independently,
each built part of the changes, and each reviewed the other's code before
anything was merged. One of the Fable cross-reviews was run by Opus 5.5 after
the system switched models.

Nothing here is deployed. The live store was not written to.

## The main finding

Clio could not tell a memory that is still true and worth showing from one
that is not, so it fell back on fixed lists and word matching.

- Of the roughly 20 items a model was handed on its first prompt, 13 were the
  same constraints, decisions and follow-ups whatever the task.
- The 7 task-matched items were chosen by the first twelve words of the first
  200 characters of the prompt. About 1.25 of the 7 were relevant.
- Every memory has an embedding, but the path that injects memory into a
  session never used them.
- A plain sentence given to `memory_recall` returned nothing for 49 of 51
  answerable prompts.
- `clio export` was not read-only: it marked every active memory as just used.
  Something did this to the live store at 09:45 on 15 November.
- Capture stored too much. The session distiller returned its maximum on 56%
  of calls, and no memory had ever been given an expiry date.

## How it was measured

`scripts/bench/resume-eval.py` (see [resume-eval.md](../../scripts/bench/resume-eval.md))
replays 60 real first prompts through the path the hooks use
(`clio resume --query`) and through the MCP tools, on a frozen copy of the
store, and scores what would be injected against 2,014 relevance judgements.
One judge (a Fable agent) labelled every result from titles and summaries,
blind to which build surfaced it. The prompts, labels and snapshot are private
and are kept outside the repository.

The score is nDCG at 7: a rank-weighted measure, 0 to 1, of the 7 task-matched
slots. A difference counts as shown when its 95% interval, from resampling the
prompts, excludes zero.

## Result

| Measure | Before | Round one | Round two | Round two, 1,000-character query |
|---|---|---|---|---|
| nDCG at 7 | 0.377 | 0.439 | 0.496 | 0.544 |
| Relevant items in the 7 matched slots | 1.25 | 1.37 | 1.73 | 1.98 |
| Off-topic share of matched items | 55.7% | 57.7% | 39.9% | 37.1% |
| Task-independent characters per first prompt | 1,962 | 541 | 461 | 457 |
| Stays quiet when nothing fits (of 9 prompts) | 1 | 1 | 6 | 6 |
| Answerable prompts given nothing (of 51) | 7 | 1 | 4 | 3 |
| Duplicate slots per brief | 0.13 | 0.28 | 0.12 | 0.13 |
| Slowest prompts (p95, local) | 2.3 s | 2.2 s | 0.4 s | 0.5 s |

- Round two against the baseline: +0.119 [+0.050, +0.197], shown.
- Round two against round one: +0.057 [−0.000, +0.112] on nDCG, on the line;
  +0.35 relevant items in the first 7 [+0.04, +0.67], shown.
- The 1,000-character query needs a hook change (below). Against 200
  characters it is +0.048 [−0.007, +0.107]: a large gain on prompts over 1,000
  characters (+0.26) and no clear effect on shorter ones.
- `memory_recall` with a sentence: empty for 49 of 51 answerable prompts
  before, none now. `memory_search`: nDCG 0.449 to 0.496, and it now finds
  prompts that name a ticket (0.28 to 0.62 on that category).

Limits to keep in mind: one judge and no human spot-check; only memories that
some build surfaced are labelled, so this measures precision, not recall; one
run per build in round two; all timings are on a Mac, not on Atlas.

## What changed

Round one (merged to `develop` at `15c18e3`):

| Area | Change |
|---|---|
| Resume brief | Task-matched knowledge is funded before the fixed sections. Follow-ups are capped at three per brief. The reason no longer quotes the prompt back. Items carry dates. |
| Use tracking | Export, browsing and query-less listings no longer count as use. Shown-memory events carry a namespace. |
| Delivery | A brief can be fetched without recording it (`--no-record`) and the injected ids acknowledged afterwards (`--acknowledge`). Constraints and decisions already acknowledged in a session are left out of later prompts. |
| Term selection | The keyword query takes its terms from up to 1,000 characters, keeps identifiers and prefers rare words. |
| MCP surface | A sentence that matches nothing strictly is retried with any-word matching, in core, so the CLI and desktop app agree. `response_format: "compact"` gives a reply about a quarter the size of `json`. Write tools can return a short receipt. `CLIO_MCP_TOOLS=core` offers eleven everyday tools. |
| Capture | The distiller's limit is three knowledge items, read as a ceiling; explicit commitments may take the total to five. Status notes get `valid_until` 30 days ahead. Later slices of a session are told what earlier slices stored. Automatic capture no longer un-archives; it proposes a revival for review. Finishing a follow-up archives its task memory. |
| Expiry | Expired memories are hidden from ordinary recall, search and context briefs, and kept in export and direct reads. `include_expired` brings them back. |
| Namespaces | Bare project names are canonicalised in core. Migration `018` repairs 53 follow-ups filed under bare names. |

Round two (merged to `develop` at `a759522`, with `c0a1e59`):

| Area | Change |
|---|---|
| Fused retrieval | "Relevant knowledge", `memory_search`, `clio search` and the desktop app's search merge keyword and semantic results by rank position, within a time budget, falling back to keywords. |
| Quiet rule | Nothing is returned when no memory is close in meaning and no keyword match shares enough of the query's terms. An identifier match is never silenced. Empty knowledge slots go to follow-ups, not to filler. |
| Near-copies | A near-copy of an item already chosen for the brief is skipped. |
| Term selection | Words that match nothing in the searched namespaces are dropped first. |
| Speed | The keyword query's join order is pinned; the slowest briefs went from 2.3 s to 0.4 s. |

Every round-two behaviour has a setting under `scoring` in
[settings.md](../reference/settings.md).

## Defects the cross-reviews caught before merge

Each of these was written by one model and found by the other.

- Suppressing shown constraints and then refilling the list handed a session
  47 of a project's 55 constraints over five prompts.
- A constraint marked as shown before it was injected stayed hidden for the
  rest of the session, including after a restart.
- A compact default for `json` replies would have broken the installed desktop
  app, including writes that commit and then report failure.
- Migration `018` could have merged differently spelled projects and turned
  `GLOBAL` into `project:global`.
- The quiet rule could silence an exact ticket match, and a setting of `0` did
  not switch it off.
- Near-copy detection compared vectors from different embedding models.
- The CLI could crash on exit while the embedding model was still loading.
- An unreadable expiry date (for example `next Friday`) was stored and then
  hid the memory everywhere. Such dates are now refused.

## A fix that was measured and taken back out

The re-check of round two argued that when nothing is close in meaning, each
keyword match should have to agree with the query on its own account, where
the rule as built lets one agreeing match open the gate for the rest. That was
implemented, then benchmarked: it scored 0.459 against 0.496 at 200 characters
and 0.507 against 0.544 at 1,000, with the same six of nine no-answer prompts
kept quiet and the same off-topic share. The partial matches it removed were
more often useful than not, so it was reverted (`c0a1e59`). The figures in the
table above are for the build as merged.

## Still open

| Item | Detail |
|---|---|
| Deploy | Roadmap item `CLIO-QUAL-002`. The desktop app on each Mac should be rebuilt in the same release, because its search and browse lists changed. |
| Hook changes | Roadmap item `CLIO-QUAL-003`. A tested patch makes the hooks acknowledge what they inject and send 1,000 characters of the prompt. It is safe to apply before or after the deploy. |
| Capture benchmark | Roadmap item `CLIO-QUAL-004`. The new distiller wording and limits have not been run against the capture model; no key was available during this work. |
| Polluted use counts | Roadmap item `CLIO-QUAL-005`. Access counts and last-used times in the live store are noise after the export sweeps. They were not reset. |
| Importance weight | The benchmark cannot separate the default from a clamped multiplier for briefs; switching it off doubles stale results in `memory_search`. Left unchanged. |
| Quiet rule | It still silences three answerable prompts in the set and lets three no-answer prompts through. Watch `clio effectiveness` after deploy. |
| Atlas timings | The fused path loads the embedding model in a one-shot process. It costs about 45 ms on a Mac and is unmeasured on Atlas; the budget is one second, after which keywords alone are returned. |
| Timed-out embeddings in the MCP server | They run to completion against the shared backend, with no cancellation. |
| Usefulness signal | Proposed in the first review and not built: have the distiller mark each injected memory as used, contradicted or ignored. |

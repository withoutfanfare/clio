# Topic notes: consolidating captured memories (17 November 2026)

**Status:** prototype measured on a snapshot copy; result neutral (see Results). Nothing changed in any live store.
Nothing will be archived live without Danny's explicit yes.

## The problem in one paragraph

Only 4% of the memories Clio injects get used, and about 17% of what it matches
is relevant. 99% of the store is automatic session capture, and ranking
changes are near their ceiling (see Clio memory 01a1ea33). So the store itself
has to get denser: fewer, better memories for the same knowledge.

## What the store looks like (snapshot of 15 Nov, read-only)

| Measure | Value |
|---|---|
| Active memories | 26,752, 99% captured |
| Age | 51% under 30 days, 35% 30–90 days, 14% over 90 days |
| Near-identical (a neighbour at cosine ≥ 0.95 / ≥ 0.90, same project) | 0% / 3% |
| Superseded (a *newer* neighbour at ≥ 0.85) | 8% |
| Receipts (per-session write-ups) | 12% |
| Captured facts, decisions, observations, summaries and notes | 21,620 |
| …that fall into a topic group of 3+ (centroid ≥ 0.70) | 9,453 in 1,558 groups (44%) |

The store is not full of copies. It is full of **fragments**: one topic (say,
"the resizable sidebar") is written up as eight separate small memories over a
day or a month, each slightly different. Each fragment competes for the
8 slots the hook injects, so one topic can fill a brief, and the old
fragments carry stale detail the newest one has replaced.

Two projects are outliers: `laws-of-work-linkedin-…` (57% near-identical) and
`i-am-producing-a-series-…` (24%), both repeated content-writing sessions.

Per-namespace table: `.support/Evidence/2026-11-17-topic-notes/store-shape.txt`.

## Design

**One maintained topic note replaces each group of fragments; the fragments
are archived, never deleted.**

1. **Group.** Per namespace, take active captured memories of kind fact,
   decision, observation, summary or note (constraints, tasks and receipts
   keep their own lifecycle and are left alone). Walk them oldest first; each
   joins the closest group whose running centroid is at cosine ≥ 0.70, else
   starts a new one. Groups cap at 30. Groups under 3 stay as they are.
2. **Write.** A small model (Haiku in the prototype) turns each group into one
   note: a title naming the topic, a 300-character summary of how it stands
   now, and up to 1,500 characters of bullet points. Newer fragments win where
   they disagree; status chatter is dropped. A group that is nothing but status
   chatter is archived with no note.
3. **Store.** The note keeps the newest fragment's dates, the highest
   importance, and lists its fragment IDs in metadata (provenance and undo).
   Source `clio-topic`, `source_ref` `topic:<id>`, so it upserts in place.
4. **Archive.** The fragments are archived. `memory_get` on a fragment still
   works, and unarchiving restores it.
5. **Maintain (not built yet).** New captures that land close to an existing
   note's centroid are folded into it on the next maintenance pass (daemon
   job), rewriting the note and archiving the newcomer. This is what keeps the
   store from fragmenting again.

This differs from the existing `consolidate.rs`, which writes one document per
*namespace* that sits beside its sources. Topic notes replace their sources, so
they shrink what ranking has to choose from.

## Prototype and how it was judged

- `scripts/bench/topic-notes.py` — `cluster`, `synth`, `build` on a copy only.
- `scripts/bench/resume-eval.py` — new arm field `snapshot`, so one run compares
  stores (same binary, same prompts, same time). A note built from the
  prompt's own session is hidden along with that session's fragments, so no
  arm sees the write-up of the task it is being asked to do.
- Arms, all `hook` with `session: true`, the current hook settings
  (8 items, 3,500 chars, minimum 40-character prompt), build 88709ad:
  - **current** — the snapshot as it is
  - **prune** — each group keeps only its newest fragment (no model; a control)
  - **notes** — each group replaced by its topic note

## Results (run `topics-fair-20261117`, 60 sessions, Sonnet labels throughout)

| Arm | Active memories | Relevant / session | Partly / session | Junk % | Stale % | Chars / session | nDCG@7 vs current |
|---|---|---|---|---|---|---|---|
| current | 26,752 | 2.5 | 5.0 | 36.1 | 10.5 | 4,549 | — |
| prune | 18,857 | 2.1 | 5.0 | 37.8 | 9.9 | 4,433 | −0.028 [−0.063, +0.003] |
| notes | 18,868 | 2.4 | 5.2 | 35.3 | 9.2 | 4,505 | −0.011 [−0.058, +0.034] |

First-prompt rel@7 against current: prune −0.235 [−0.451, −0.020] (shown worse);
notes −0.118 [−0.314, +0.078] (not shown either way).

What it cost: $1.63 to write 1,554 notes with Haiku, and $2.56 for Sonnet labels and the re-judging.

Two biases were found and removed before these figures, and both had favoured the current store:

1. **Hidden notes.** A note that held any fragment from the prompt's own session was hidden whole,
   taking other sessions' knowledge with it. resume-eval now restores those other-session
   fragments; prune survivors get the same treatment. Red Pen review, High.
2. **Generous older labels.** The 15 Nov labels say "relevant" more often than the Sonnet judge
   does: of 110 such pairs, Sonnet kept 56. Every new note was judged by Sonnet, so mixing the two
   understated notes. Re-judging all older-labelled pairs in the run with Sonnet gives the same
   table as above.

The first, biased run had shown notes at 2.3 relevant per session against 3.0.

## What this means

- **Topic notes are neutral on quality and cut the store by 30%.** They keep the knowledge (prune,
  which simply drops fragments, is measurably worse), cut stale items slightly (10.5% to 9.2%) and
  leave junk and size per session unchanged.
- **They do not deliver the hoped-for gain.** Relevant memories per session do not rise. The
  judge tends to call a broad topic note "partly" where it called the one matching fragment
  "relevant", so a denser store trades specific hits for broader ones.
- So store density is not the lever for the 4% used rate either, at least not at the hook's
  8-item / 3,500-character cap. What is injected is limited by what is captured: 42% of captured
  memories are one-off singletons that no consolidation touches.

## Recommendation

1. **Do not archive anything in the live store for this.** The evidence shows no gain in
   relevance, and archiving 9,438 memories is a large change to justify on size alone.
2. If store size or maintenance cost becomes a problem (the O(N) embedding scan), topic notes
   are a safe way to shrink it by about 30% without losing quality. Build step 5 (fold new
   captures into existing notes) first, or the store fragments again.
3. The better lever is at capture time: write fewer, more specific memories (the per-session
   capture cap on develop at cdec800 is a step this way). Judge it with `used-rate.py`, which
   measures use rather than topical relevance.

Since the snapshot (15 Nov), the live store has changed: on 17 Nov, with Danny's yes, 3,835 memories were archived (dead projects, captures from deleted worktrees, receipts over 30 days old; ids in `.support/Evidence/2026-11-17-store-cut/archived-ids.json`). The group counts above describe the snapshot.

Raw results: `$PRIV/results/topics-fair-20261117/report.txt`; notes in `$PRIV/topics/notes.jsonl`;
copy stores in `$PRIV/snapshot-{prune,topics}-fair`.

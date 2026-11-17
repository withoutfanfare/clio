# resume-eval.py — what does Clio actually hand a model?

`recall-eval.py` scores `clio brief` against a tag-overlap stand-in for relevance.
This script scores the path that matters in daily use: the hooks call
`clio resume` at session start and again with `--query` on the first prompt, and
the result is injected into the session. It replays that over a fixed set of real
prompts, scores the injected text against judgements made once and stored, and
drives the two MCP tools a model would reach for (`memory_recall`,
`memory_search`) the same way.

Use it to accept or reject a retrieval change: run the current build and the
candidate side by side on the same snapshot, prompts and labels.

```sh
scripts/bench/resume-eval.py selftest
scripts/bench/resume-eval.py run --private "$PRIV" --arms "$PRIV/arms.json" --runs 2 --name my-change
```

Python standard library only. No paid model calls: capture is forced off and the
script refuses any embedding provider other than the local model.

## The private directory

Prompts, labels, snapshots and results are private data. They live in one
directory that you pass as `--private`; the script refuses to run if that
directory is inside a git repository.

```text
$PRIV/snapshot/memory.db           frozen copy of the store
$PRIV/snapshot/clio-settings.json  the scoring, embedding and attention settings the store runs with
$PRIV/prompts.jsonl                the prompt set
$PRIV/labels.jsonl                 the judgements
$PRIV/arms.json                    your arms (paths are machine-specific; see resume-eval-arms.example.json)
$PRIV/models/                      local embedding model cache (downloaded on first use if empty)
$PRIV/results/<name>/              raw output per arm and run, meta.json, report.txt
```

### Taking a snapshot

Use SQLite's backup, read-only. Two traps, both hit on 15 November 2026:

- **Not `.dump`.** A dump-and-restore renumbers rows, and the full-text index is
  keyed on row number, so keyword search silently returns the wrong rows. `run`
  checks the index against the rows before it starts and refuses a broken one.
- **Not `clio export`.** At `fa3ceb5` export writes to the store: it bumps the
  access count and last-used time of every memory, which are ranking inputs.
  Nothing can detect that afterwards, so never run it against a snapshot either.

```sh
# shared store on another host
ssh atlas 'python3 -c "
import sqlite3, sys
src = sqlite3.connect(\"file:/home/ubuntu/.local/share/clio/memory.db?mode=ro\", uri=True)
src.execute(\"PRAGMA query_only=ON\")
copy = sqlite3.connect(\":memory:\")
src.backup(copy)
sys.stdout.buffer.write(copy.serialize())
" | gzip -1' | gunzip > "$PRIV/snapshot/memory.db"

# local store
sqlite3 "file:$LIVE_DB?mode=ro" ".backup '$PRIV/snapshot/memory.db'"
```

Then write `$PRIV/snapshot/clio-settings.json` with the live store's `scoring`,
`embeddings` and `attention` values (they change ranking). Leave keys and the
`remote` block out; the script forces `remote` off, capture off and the daemon
off in every copy whatever the file says.

### Building the binaries

Build each arm's binaries into a scratch target directory, from a checkout that
includes `.cargo/config.toml`:

```sh
CARGO_TARGET_DIR=/scratch/target-baseline cargo build --release --locked -p clio-cli -p clio-mcp
```

Without that config file SQLite is built without its maths functions and every
`resume` fails with `no such function: exp`. Do not point an arm at the `clio` or
`clio-mcp` on your `PATH`; they may be an older build (the script warns).

## Arms

An arm is a binary plus how the query is prepared. `--arms` is a JSON list:

| Field | Meaning |
|---|---|
| `name` | Unique label. The first arm listed is the reference the others are compared with. |
| `kind` | `hook` (session start, then first prompt, through `clio resume`) or `mcp` (one tool call over stdio). |
| `clio` / `mcp` | Absolute path to the binary. |
| `tool` | `mcp` arms: the tool name, e.g. `memory_search`. |
| `query_chars` | Query is the prompt with whitespace collapsed, cut to this many characters. Today's hook uses 200. Omit for the whole prompt. |
| `query_cmd` | Instead of `query_chars`: a shell command that reads the prompt on stdin and prints the query. Use it to test any extractor. |
| `extra_args` | `hook` arms: extra arguments for the first-prompt `resume` call, e.g. `["--char-budget", "3000"]`. |
| `args` | `mcp` arms: the tool arguments; `"{query}"` and `"{namespace}"` are filled in. Default `{"query": "{query}", "_clio_namespace": "{namespace}"}`, which is what the local bridge sends. |
| `settings` | Settings merged over the snapshot's `clio-settings.json` for this arm, e.g. `{"scoring": {"decay_lambda": 0.0}}`. |
| `start_max_items` | `hook` arms: `--max-items` for the session-start call (default 12, as the hook). |
| `session` | `hook` arms: `true` also replays each prompt's `followups` through the hook's gates (minimum length, same topic as before, already shown) and adds the whole-session table. |
| `min_chars` | `hook` arms: the hook's minimum prompt length. 12 before 17 Nov 2026, 40 after. Applies to the first prompt too; omit it and every first prompt is asked. |

### Whole sessions

Most injections come after the first prompt, so the first-prompt figures miss
most of what a model is handed. `resume-eval.py followups --private "$PRIV"`
adds to every prompt the later prompts of its session, read from the
transcript (up to 15; Stop-hook feedback and continuation summaries are left
out because the hook never sees them). An arm with `"session": true` then
replays them. A later turn is labelled under its own key, `p07#3`, judged
against the session's first prompt and that turn's prompt together.
`resume-eval.py label --private "$PRIV"` fills `unjudged.jsonl` with a model
judge (Sonnet by default, through Claude Code in safe mode) and appends the
labels with a `"by"` field, so model labels can be told from earlier ones.

### How to run a new arm

1. Build the candidate into its own target directory (above).
2. Add it to your arms file **after** the baseline arms, and keep the baseline
   arms in the file. Ranking depends on the current time, so a baseline from
   last week is not comparable; always run both in one invocation.
3. `resume-eval.py run --private "$PRIV" --arms "$PRIV/arms.json" --runs 2 --name <name>`
4. If the report ends with `UNJUDGED`, label them (next section) and run
   `resume-eval.py report --private "$PRIV"`. Do not read the quality figures
   before that: an unjudged slot scores nothing, so a new arm that finds
   different memories looks worse than it is.
5. Read the paired table at the bottom first, then the per-prompt table for any
   prompt that got worse.

## Labels

`labels.jsonl` has one line per (prompt, memory) pair:
`{"prompt_id": "p07", "memory_id": "…", "label": "relevant"}`. A later line for
the same pair replaces an earlier one, so corrections are appended.

| Label | Meaning |
|---|---|
| `relevant` | About the thing this prompt asks for, still current, and it would help do the task. |
| `partly` | Same area and of some use: a neighbouring feature, a project rule that bears on this task, background. |
| `stale` | On topic, but a point-in-time status note, a receipt, or something later memories supersede. |
| `irrelevant` | Not about this task. Includes standing constraints and follow-ups that have nothing to do with the prompt. |

Judge from the title and summary, against the prompt as written. A memory with no
label is **unjudged**. It is counted in its own column and never as irrelevant.

### How to label unjudged items

```sh
scripts/bench/resume-eval.py unjudged --private "$PRIV"      # writes $PRIV/unjudged.jsonl
# fill in "label" on every line (each carries the prompt, title, summary, kind and date)
cat "$PRIV/unjudged.jsonl" >> "$PRIV/labels.jsonl" && rm "$PRIV/unjudged.jsonl"
scripts/bench/resume-eval.py report --private "$PRIV"        # re-scores the stored run; nothing is re-run
```

A blank or misspelt label stops the script, so a half-filled file cannot be
merged by accident. Label blind to which arm surfaced the item (the `arms` field
is there for curiosity, ignore it while judging).

## The prompt set

`prompts.jsonl`, one prompt per line:
`{"id", "namespace", "category", "expect_nothing", "prompt", "session"}`.

- `namespace` is what the prompt's working directory resolved to.
- `expect_nothing` marks prompts where nothing relevant exists in the namespace
  or in `global`; the right answer is silence. The report flags any such prompt
  that later gains a relevant label.
- `session` is the Claude Code session the prompt came from. Memories captured
  from that same session are archived in the prompt's copy before it runs. When
  the prompt was typed they did not exist; left in, every arm is handed the
  write-up of the very task it is asked to do.

`resume-eval.py prompts --private "$PRIV" --clio <binary>` lists candidates: the
first prompt of each recent Claude Code session that the hook would act on, with
its namespace and a flag on anything that looks like a credential. Choosing from
the candidates is manual: spread them over projects and include long prompts
with preamble, short follow-ups, prompts naming a ticket, broad prompts and
no-answer prompts. Delete `candidates.jsonl` afterwards; it holds the flagged
prompts too.

Changing the prompt set or the snapshot starts a new series. Numbers from
different snapshots or prompt sets are not comparable.

## Reading the report

Per arm, one line per prompt, then:

| Column | Meaning |
|---|---|
| `items` | Mean memories injected per prompt, all sections. |
| `rel@7`, `partly@7` | Mean number of relevant / partly relevant memories in the first 7 task-matched slots (answerable prompts). Seven is what a model gets today. |
| `nDCG@7` | Rank-weighted score of those 7 slots, 0 to 1 (relevant 2, partly 1). The ideal is the best ordering of everything labelled for that prompt. |
| `junk%`, `stale%`, `unjudged%` | Share of every injected memory, all sections. `junk%m`: of the task-matched ones only. |
| `dup/brief` | Slots per brief spent on a memory near-identical to an earlier one (embedding cosine at or above `--dup-cosine`, default 0.85, or the same title). |
| `chars m` / `other` / `start` | Characters injected on the first prompt that are task-matched / task-independent, and the session-start resume. |
| `abstain` | No-answer prompts that got no task-matched memory. Higher is better. |
| `silent` | Answerable prompts that got none. Lower is better. Read it with `abstain`: an arm that returns nothing scores perfectly on one and terribly on the other. |
| `p50 ms`, `p95 ms` | The timed call: the first-prompt `resume` process, or the tool call (server start-up excluded). Local disk, no SSH. |
| `failed` | Calls that errored or timed out (8 s, as the hook). They score as injecting nothing; they are never dropped. |

For `hook` arms "task-matched" is the `Relevant knowledge` section; everything
else in the brief is task-independent. For `mcp` arms every result is
task-matched.

### How big a difference counts

Two tables answer this.

- **Run-to-run spread.** Each arm is run `--runs` times. On one snapshot the
  quality figures do not move at all (see the baseline), so this mostly tells
  you about latency.
- **The paired table.** For each arm against the reference: prompts better /
  same / worse, and the mean difference with a 95% interval from resampling the
  prompts. With about 50 answerable prompts the interval on nDCG@7 is roughly
  ±0.14 wide for two arms that return different memories. **If the interval
  includes 0, the change has not been shown**, however good the means look.
  Then look at which prompts got worse.

## Baseline: develop `fa3ceb5`, 15 November 2026

Snapshot of the shared store taken 15 Nov 2026 10:20 UTC (32,324 memories,
26,752 active). 60 prompts over 25 namespaces (51 answerable, 9 no-answer),
1,121 judgements. Two runs.

```text
arm         items  rel@7  partly@7  nDCG@7  junk%  junk%m  stale%  unjudged%  dup/brief
hook-200     11.7   1.25      1.33   0.432   75.5    55.7     3.6        0.0       0.13
hook-1000    11.7   1.18      1.41   0.431   75.4    55.4     3.7        0.0       0.13
mcp-recall    0.1   0.00      0.02   0.012   40.0    40.0    40.0        0.0       0.00
mcp-search    8.0   1.29      2.06   0.522   42.8    42.8    10.9        0.0       0.37

arm         chars m  chars other  chars start  abstain  silent  p50 ms  p95 ms  failed
hook-200       2712         1888         3408        1       7     201    2070       0
hook-1000      4030         1888         3408        1       7     216    2174       0
mcp-recall       72            0            0        9      49      18      29       0
mcp-search     2686            0            0        1       3      59     214       0

nDCG@7 by category  hook-200  hook-1000  mcp-recall  mcp-search
broad (9)              0.218      0.211       0.000       0.595
follow-up (6)          0.288      0.288       0.102       0.487
identifier (8)         0.700      0.700       0.000       0.332
long-preamble (11)     0.390      0.387       0.000       0.510
specific (17)          0.499      0.499       0.000       0.593

against hook-200     nDCG@7 better/same/worse, diff [95%]      rel@7
hook-1000    1 / 48 / 2   -0.002 [-0.008, +0.003]    0 / 49 / 2   -0.078 [-0.196, +0.000]
mcp-recall   1 / 11 / 39  -0.420 [-0.513, -0.319]    0 / 24 / 27  -1.255 [-1.647, -0.843]
mcp-search  28 / 1 / 22   +0.089 [-0.048, +0.232]   20 / 14 / 17  +0.039 [-0.451, +0.569]
```

Run-to-run spread: zero on every quality and character figure, and no prompt's
injected memories differed, across two runs and against an earlier pair taken 14
minutes before. Latency moved by up to 17 ms at p50 and 320 ms at p95 (hook arms), 3 ms
and 5 ms (MCP arms).

What the baseline says:

- Today's hook path puts 1.25 relevant memories in its 7 matched slots, and
  three-quarters of everything it injects on the first prompt is irrelevant to
  the task. Seven of 51 answerable prompts get no matched memory at all.
- A 1,000-character query finds the same memories as 200 characters (48 of 51
  prompts identical) and injects 1,300 more characters, because every matched
  item quotes the query back in its reason.
- `memory_recall` given a sentence returns nothing for 49 of 51 answerable
  prompts.
- `memory_search` is ahead on broad and long prompts and well behind on prompts
  that name a ticket. Overall it is +0.09 nDCG against the hook path, which the
  interval does not separate from zero. Neither path stays quiet on no-answer
  prompts (1 of 9 each).
- The hook's `resume` takes about 0.2 s typically and 1.3 to 2.6 s in the two
  largest namespaces, before any SSH.

## Limits to keep in mind

- **One judge, from titles and summaries.** The labels were made by one model in
  one pass, without reading full content and without a human check. Spot-check a
  tenth before leaning on a close call. Staleness in particular is a guess from
  wording and dates.
- **Only what some arm surfaced is labelled.** A relevant memory no arm found is
  invisible, so this measures precision and ranking, not recall, and the nDCG
  ideal rises when a new arm finds something better (re-run `report` on the old
  results after labelling; old arms' nDCG can go down).
- **A new arm starts with unjudged items.** Until they are labelled its quality
  figures are floors. Label first, compare second.
- **The prompts come from the past, the store is from today.** Memories from the
  prompt's own session are hidden, but memories from sibling and later sessions
  about the same task are not. They are labelled relevant, they match the
  prompt's wording unusually well, and they flatter every arm, keyword matching
  most of all.
- **The query for the MCP arms is the prompt, not what a model would type.** It
  is the unhelpful end of realistic use. A capable model writes a short keyword
  query and `memory_recall` then does far better than 49 silent prompts.
- **The clock is not fixed.** Ranking decays with age and follow-ups go dormant,
  and the CLI has no way to pin the time. Compare arms within one invocation,
  never across days.
- **Last-used and access counts in this snapshot are polluted** (nearly every
  memory was touched in the same minute on 15 November). A change that leans on
  those signals is being tested against noise.
- **60 prompts, 25 namespaces, one person's work.** Category figures rest on 6 to
  17 prompts each. The no-answer set is 9 prompts, three of them context-free
  follow-ups.
- **Session start is replayed in part.** Only the `resume` call is, because it
  decides what the first prompt may repeat. The branch-recall search, the
  project-brief fallback and the review-queue line are not, so `chars start`
  understates the real session-start injection.
- **The rendering is a copy of the hooks' as of 14 November 2026.** If the hooks
  change how they print items, update `render()` or the character counts drift.
  Which memories are injected is unaffected.
- **Latency is local.** No SSH, no Atlas, warm disk cache, a fresh database copy
  per prompt. Use it to compare arms, not to predict the hook's real timing.
- **Duplicates are counted conservatively.** At cosine 0.85 the pairs are clear
  paraphrases; plenty of looser repeats sit between 0.80 and 0.85 and are not
  counted.

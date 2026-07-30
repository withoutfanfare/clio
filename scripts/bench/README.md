# Benchmarks

Tools for tuning Clio on measured outcomes rather than intuition. All three run
against a copy of a database and never modify live data.

| Script | Answers |
|---|---|
| `link-invariants.sh` | Does auto-linking obey its design rules? |
| `recall-eval.py` | Does retrieval return the *right* memories? |
| `dial-in.sh` | Which threshold and cap maximise retrieval quality? |

## link-invariants.sh

A regression test on controlled synthetic fixtures, using the production threshold
and cap. Thirteen checks: similar memories link, unrelated ones do not, links never
cross a namespace, archived memories are never targets, an excluded kind links in
neither direction, the total-degree cap holds **and provably binds** (the cluster
is larger than the cap, so the assertions cannot pass with the cap logic deleted),
every member of a cluster is reachable, a second pass is idempotent, a later pass
against new similar memories breaches no cap, and a newcomer to a saturated
cluster still connects to its under-cap members.

```sh
scripts/bench/link-invariants.sh
```

Needs the local embedding model cached — set `FASTEMBED_CACHE_PATH` on non-macOS.

It aborts rather than reporting a result if its fixtures fail to build. Without
fixtures, every "no unwanted link" assertion passes trivially and the suite would
report success while proving nothing.

## recall-eval.py

Scores retrieval by hold-one-out. For a memory M the query is M's title, and the
relevant set is other memories in M's namespace sharing at least two tags with M.

The decisive figure is the last one printed: **of the memories `--include-links`
adds to a brief, what fraction are relevant?** Above the chance baseline, links are
contributing; at chance, they are padding briefs regardless of how tidy the graph is.

```sh
sqlite3 "$LIVE_DB" ".backup /tmp/eval.db" && cp settings.json /tmp/clio-settings.json
scripts/bench/recall-eval.py /tmp/eval.db 110
```

Arms are scored as pairs (both invocations must succeed and return something) and
failed invocations are counted and reported. The script refuses the live database
path — briefs bump access counts, which are scoring inputs.

Baseline recorded 2026-07-30 against 3,842 live memories at threshold 0.6, cap 5,
receipts excluded:

```text
without links:  2.3 results, 33.2% precision, MRR 0.393
with links:     7.7 results, 59.6% precision, MRR 0.712
added 5.7 per brief — 57.4% relevant vs 6.3% chance = 9.1x lift
```

## dial-in.sh

Relinks a copy at each setting and scores the result, sweeping threshold then cap.
One variable moves at a time: changing both leaves the outcome unattributable.

```sh
scripts/bench/dial-in.sh /path/to/copy-of-memory.db 110
```

Findings from 2026-07-30 (110 queries) that set the current configuration:

| threshold (cap 5) | precision | MRR | added precision |
|---|---:|---:|---:|
| 0.55 | 55.3% | 0.714 | 48.7% |
| **0.60** | **59.6%** | **0.712** | 57.4% |
| 0.65 | 59.5% | 0.680 | 66.1% |

0.60 is the only setting near the top of both precision and ranking. Raising the
threshold buys purer links but fewer of them and worse ranking; lowering it dilutes.
On the cap at 0.60: 3 gave 58.7%/0.737, 5 gave 59.0%/0.749, and 8 gave 58.6%/0.749
with 1,134 more links for no gain.

**Noise floor — read before trusting any row above.** The 2026-07-30 run measured
threshold 0.60 / cap 5 twice (once in each sweep) and got different answers: 59.6%
precision / MRR 0.712 against 59.0% / 0.749. Same configuration, so that gap —
0.6pp precision, 0.037 MRR — is run-to-run noise, and every difference the table
was used to decide (0.60 vs 0.65 at 0.1pp; cap 3 vs 5 vs 8 spanning 0.4pp) sits
inside it. The added-precision column, the figure this README calls decisive,
rises monotonically with threshold and favours 0.65. The 0.70 sweep ran but was
never recorded. The one conclusion outside the noise is negative: cap 8 buys ~1,100
extra links for no measurable gain. Before treating any of these settings as
chosen on evidence, re-run each configuration over at least three `CLIO_SEED`
values and compare the spread to the differences; note also that these sweeps
relink from empty (a single-pass cap), while production applies the cap
cumulatively across timed runs.

## The caveat that applies to all of this

Ground truth is tag overlap, and tags are assigned by the same model pass that wrote
the content — so they are correlated with the embeddings being tested. The chance
baseline shows the signal is real, but absolute percentages are softer than they
appear. These tools reliably **rank configurations against each other**; they do not
measure absolute relevance. Establishing that would need human or independent-model
judgements.

Two further gaps worth knowing:

- `recall-eval.py` scores `brief`, whereas the prompt hook calls `resume`, which
  assembles a different mix (open work, constraints, knowledge).
- Scoring settings (`scoring.decay_lambda`, `scoring.access_boost_weight`) affect
  ranking and have never been tuned. `recall-eval.py` would score them as-is.

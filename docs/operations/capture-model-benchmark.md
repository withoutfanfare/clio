# Capture Model Benchmark

This is the reproducible baseline for choosing Clio's shared capture model.
The [deployment runbook](deployment.md#capture-model-changes) contains the
commands for switching and repeating the comparison.

## Baseline: 29 July 2026

The benchmark used five synthetic cases covering an architectural decision, a
data invariant, a documentation preference, a routine session with nothing
durable, and a substantive session containing a decision, an API constraint, a
receipt and an attempted prompt injection. Each case ran twice through each
model using `--dry-run --metrics`, so no memories were written.

GPT-5.6 requests used `reasoning_effort: none` and no `temperature`. GPT-4.1
retained `temperature: 0.1`. No paid model judge was used; outputs were checked
against the expected kind, durability, prompt-injection handling and namespace.

| Model | Calls | Mean latency | Input tokens | Output tokens | Reasoning tokens | Estimated cost |
|---|---:|---:|---:|---:|---:|---:|
| GPT-4.1 | 10 | 1,949 ms | 5,260 | 1,525 | 0 | $0.0227 |
| GPT-5.6 Luna | 10 | 1,789 ms | 5,250 | 1,063 | 0 | $0.0116 |
| GPT-5.6 Terra | 10 | 1,894 ms | 5,250 | 1,086 | 0 | $0.0294 |

Costs use the provider prices current on the test date: GPT-4.1 $2/$8,
Luna $1/$6 and Terra $2.50/$15 per million input/output tokens. Recheck the
[OpenAI model pages](https://developers.openai.com/api/docs/models) before
using these figures for a future budget.

All three models correctly classified the decision and invariant, rejected the
routine session, ignored the injected instruction, and extracted the expected
three durable items from the substantive session. Differences that mattered:

- Luna was fastest and cheapest, but varied between `constraint` and `note` for
  the preference case.
- GPT-4.1 was stable but produced 40% more output tokens than Luna and Terra.
- Luna twice promoted every substantive-session memory to `global`. GPT-4.1
  kept the project decision and receipt scoped; Terra kept the project decision
  scoped.

The benchmark exposed an underspecified namespace prompt. Clio was updated to
reserve `global` for explicitly cross-project knowledge and require identifiable
project decisions, implementation details and receipts to remain project-scoped.
A two-repeat targeted rerun then produced:

| Model | Correctly scoped project memories |
|---|---|
| GPT-4.1 | All three memories in both runs |
| GPT-5.6 Luna | All three in one run; all three still `global` in one run |
| GPT-5.6 Terra | All three memories in both runs |

## Decision (29 July 2026 — superseded, see below)

Use **GPT-5.6 Terra** for shared Clio capture, distillation and consolidation.
It matched GPT-4.1's namespace reliability in this corpus, used fewer output
tokens, was slightly faster, and provides the newer model family requested.
Its estimated cost was about 29% above GPT-4.1 for these ten calls, but the
absolute difference was under one cent.

Do not switch to Luna until a representative rerun shows reliable project
scoping. This baseline is deliberately small and synthetic; rerun it after a
prompt change, model snapshot change, or material increase in Clio API spend.

## Revision: GPT-4.1 (30 July 2026)

The clause above — rerun after a material increase in spend — triggered the day
after the baseline was set. Live volume turned out to be nothing like the ten
synthetic calls: 119 distillations in a day, a 361-memory day, and 21
consolidations against 1–4 on previous days. At that volume Terra's output price
of $15/M against GPT-4.1's $8/M stopped being "under one cent".

**The active capture model is now `gpt-4.1`.** The reasoning is cost at volume,
not quality: this benchmark had already found GPT-4.1 namespace-reliable in both
targeted runs, equal to Terra, so the property the original decision turned on is
preserved. GPT-4.1's known cost is 40% more output tokens than Terra, which the
lower output price more than offsets.

What this revision did **not** do is rerun the five cases. The switch rests on the
existing table, which is legitimate for GPT-4.1 because it was measured here — but
it means no cheap model has been assessed. `gpt-4o-mini` in particular remains
**unbenchmarked**, so it should not be adopted on price alone: namespace scoping is
the property that separated these models, and nothing suggests a mini-class model
holds it.

Two things changed alongside the model, both of which alter the arithmetic for any
future rerun:

- Capture usage now records `cached_input_tokens`. The distillation system prompt is
  a ~1,200-token stable prefix on every call — 34% of input, larger than the median
  payload — so a rerun should compare cached versus uncached input, not just nominal
  token counts.
- `consolidate.auto_threshold` moved from 10 to 50. Consolidation sends up to
  `MAX_INPUT_CHARS` (60,000 chars, ~15,000 tokens), so it was contributing on the
  order of a third of total spend at the old threshold.

Rerun the five cases including a cheap candidate before changing model again.

## Prompt caching, measured (30 July 2026)

Taken with `clio distill - --dry-run --metrics` against `gpt-4.1` on Atlas,
repeating an identical payload back to back:

| Total input tokens | Cached on first call | Cached on repeat |
|---:|---:|---:|
| 1,233 | 0 | 0 — never cached |
| 2,875 | 0 | 2,688 (93%) |

Caching populates on the first call and hits from the second, but only once total
input clears roughly two thousand tokens; at 1,233 it never engaged, despite the
provider's documented 1,024-token minimum. Treat ~2,000 as the practical floor.

Two consequences:

- **Do not shorten the distillation system prompt.** It is ~1,200 tokens, and with
  the median digest at ~1,180 a typical call lands near 2,400 total — inside the
  working range, so most of that prefix is billed at the reduced cached rate.
  Trimming the prompt would drop calls under the floor and lose the discount on
  everything, which costs more than the tokens saved.
- **Compare cached and uncached input separately in any rerun.** Nominal token
  counts overstate the cost of a large stable prefix. The first call of a session
  cluster pays full price; the rest largely do not.

Check the provider's current cached-input rate before turning these counts into
money — the discount is not the same across model families.

## Correction: the namespace findings above graded unresolved output

The 29 July comparison judged namespace scoping from `--dry-run`, which at the time
reported the **model's raw suggestion**. Storage does not use that value directly:
`resolve_namespace` (named `resolve_distill_namespace` when this was written; since
30 July it is the shared rule for capture as well as distill) applies explicit
`--namespace` → the model's `"global"` promotion → the working directory's
namespace → and only then the model's suggestion. In practice the working
directory almost always wins.

So the differentiator the original decision leaned on — "Luna twice promoted every
substantive-session memory to `global`; Terra kept the project decision scoped" —
was measuring something production largely discards. A model naming
`project:wrong-guess` and a model naming `project:right-guess` are stored
identically when a working-directory namespace is available. Only `"global"`
promotion genuinely survives, because it is honoured ahead of the default.

`--dry-run` now resolves the namespace the same way storage does, so a future rerun
grades what would actually be stored. The narrower question a rerun should ask is
whether a model over- or under-uses `"global"`, since that is the one namespace
decision the model still controls.

This does not change the GPT-4.1 decision, which rests on cost at volume.

## Rerun with cheap candidates (7 August 2026)

The original five cases were never preserved, so this rerun reconstructed them
from the descriptions above as checked-in fixtures: `scripts/bench/capture-model/`
now holds the cases, an automated judge, and a runner (`run.py`) — a rerun is one
command against a throwaway DB. Comparisons within this table are like-for-like;
comparison against the July table is indicative only, because the case text
differs. This run also used the current distillation prompt, which since today
states a hard limit of 5 memories plus the receipt.

Two repeats per case per model, `--dry-run --metrics`, local binary, neutral
working directory (so `global` promotion — the one namespace decision the model
still controls — is what gets graded):

| Model | Calls | Mean latency | Input (cached) | Output (reasoning) | Est. cost | Failed checks |
|---|---:|---:|---:|---:|---:|---|
| GPT-4.1 | 10 | 5,394 ms | 14,312 (6,272) | 4,830 (0) | $0.0579 | routine session → 1 receipt (both repeats) |
| GPT-4.1 mini | 10 | 10,970 ms | 14,312 (4,992) | 5,461 (0) | $0.0130 | routine session → 2 and 4 status facts |
| GPT-4o mini | 10 | 6,130 ms | 14,312 (11,776) | 4,107 (0) | $0.0037 | routine session → 2 status facts (both); cross-project preference not promoted to `global` once |
| GPT-5 mini | 10 | 17,289 ms | 14,302 (6,528) | 18,223 (11,008) | $0.0386 | routine session → 3 memories (both repeats) |

What mattered:

- **Every model passed the July-critical checks**: project memories stayed
  project-scoped, the injected instruction was ignored by all, exactly one
  receipt on the substantive session, and the new 6-memory cap was respected.
- **GPT-4o mini failed a `global`-promotion once** — the one namespace decision
  the 30 July analysis says still matters. Its 16× saving comes with that risk.
- **GPT-5 mini is a poor fit for this workload**: default-effort reasoning
  burned 11k hidden tokens across 10 calls, leaving only a ~1.5× saving at 3×
  the latency.
- **The routine session separates discipline, not correctness**: GPT-4.1 emitted
  a single defensible receipt; the minis added "tests are green"-style status
  facts — transient state the prompt forbids. The hard cap bounds the blast
  radius, and `is_session_noise` could be extended to title patterns like
  "… status" if this shows up in live traffic.

**Recommendation (pending operator approval): GPT-4.1 mini** — 4.5× cheaper
than the incumbent on identical input, clean on scoping and injection in both
repeats, with routine-session noise as the known, bounded weakness. GPT-4o mini
is the aggressive option (16×) only if an occasional missed `global` promotion
is acceptable. Do not use GPT-5 mini for capture.

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

## Decision

Use **GPT-5.6 Terra** for shared Clio capture, distillation and consolidation.
It matched GPT-4.1's namespace reliability in this corpus, used fewer output
tokens, was slightly faster, and provides the newer model family requested.
Its estimated cost was about 29% above GPT-4.1 for these ten calls, but the
absolute difference was under one cent.

Do not switch to Luna until a representative rerun shows reliable project
scoping. This baseline is deliberately small and synthetic; rerun it after a
prompt change, model snapshot change, or material increase in Clio API spend.

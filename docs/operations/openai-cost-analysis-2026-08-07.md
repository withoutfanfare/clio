# Spike: Where Clio's OpenAI Spend Goes, and How to Cut It

**Date:** 2026-08-07
**Question:** Which Clio actions are spending OpenAI tokens, how often and how large are the calls, and what would reduce the daily cost?
**Scope:** Narrow — call-site inventory in the codebase, cross-checked against live metrics (hook CSVs, capture-spool health log, Atlas database). No code changes.
**Verdict:** Go — three levers, in order: cheaper model, fewer calls, smaller outputs.

## Context

Daily OpenAI usage attributed to Clio has grown to a level Danny considers too high. The OpenAI key is shared with other apps, so the provider dashboard cannot attribute spend; this spike reconstructs it from Clio's own records. It feeds the decision of which optimisation work to schedule next.

## Findings

### 1. Every paid call is a chat-completion on `gpt-4.1`; embeddings cost nothing

Both machines (this Mac and Atlas, the shared server) run **local** embeddings (`all-MiniLM-L6-v2` via fastembed) — semantic search, auto-embed and auto-link make **no** API calls. AI titles are disabled. The complete inventory of OpenAI call sites:

| Call site | Trigger | Frequency | Verdict |
|---|---|---|---|
| Session distillation (`checkpoint.rs` → `capture::distill`) | Every Claude Code *Stop* event (end of every assistant turn) and every Codex session, via the hook → spool → `clio checkpoint` → Atlas | **100–260 calls/day** | **~95% of spend** |
| Capture classification (`memory_capture` MCP tool, `clio capture`) | When an agent explicitly captures | ~1–6/day | Negligible |
| Consolidation (`consolidate.rs`) | `consolidate --if-due` after checkpoints, threshold 50 new memories per namespace | ~1/day, ≤15k tokens in | Negligible (~$0.05/run) |
| AI titles (`title.rs`) | Disabled in both settings files | 0 | Nil |
| `clio distill/migrate --classify` dry-runs | Manual only | Ad hoc | Only when run |

All distillation executes **on Atlas** (the local CLI bridges over SSH), so Atlas's `clio-settings.json` decides the model: currently `gpt-4.1` ($2 per million input tokens, $8 per million output — prices from the 30 July benchmark; recheck before budgeting).

### 2. Measured volume: the Stop hook dominates

From the spool health log (`capture-spool/health.jsonl`), Atlas's `session_checkpoints` table, and `~/.claude/metrics/clio-stop.csv`:

| Day | Distillation calls (Atlas) | Payload sent | Memory text stored (model output) |
|---|---:|---:|---:|
| 29 Jul | 108 | ~977k chars | ~201k chars |
| 30 Jul | 92 | ~680k chars | ~199k chars |
| 31 Jul | 74 | ~208k chars | ~118k chars |
| 1–5 Aug | 0 (SSH bridge down — 447 deltas dead-lettered) | 0 | 0 |
| 6 Aug | **263** (backlog + normal day) | ~2.04M chars | ~587k chars (892 memories) |
| 7 Aug (partial) | 161 | ~779k chars | ~366k chars (555 memories) |

Per call: ~1,200-token fixed system prompt + median ~1,180-token digest (mean ~1,900 — the p90 tail reaches 22k chars, capped at 24k). Estimated cost at gpt-4.1 rates, allowing for the measured ~93% prompt-cache hit on warm calls:

- **Steady day (~100–160 calls): roughly $1.50–2.00/day.**
- **Heavy day (6 Aug): roughly $2.80–3.30.**
- Roughly **$45–85/month**, of which ~95% is distillation. These are estimates from character counts (chars ÷ 4 ≈ tokens); Clio does not yet persist real token usage (see finding 5).

Output is nearly half the bill despite being smaller, because gpt-4.1 output costs 4× input.

### 3. Calls are too frequent: distillation fires per *turn*, not per session

The Stop hook fires at the end of every assistant turn. Any delta with a user turn, ≥1,500 chars, or a commit is queued — the metrics show the same session queueing two deltas **six seconds apart** (each paying the system prompt again, and each below the ~2,000-token floor where prompt caching engages, so billed at full rate). The `low_value` skip only catches 10–25% of stops.

### 4. Output is too noisy: ~3.4 memories per checkpoint

6 August stored **892 distilled memories** in one day (568 Claude + 324 Codex). That is both the second-largest cost component (model-written text at $8/M tokens) and exactly the memory noise the "Improvement Strategy for Clio Memories" decision already wants rid of. Fewer, higher-bar atoms cut cost and improve recall quality together.

### 5. No token accounting, no attribution

`capture.rs` already parses `TokenUsage` (input/output/reasoning/cached) from every response, but it is only printed by manual CLI commands — nothing persists it. The hook CSVs record characters, not tokens or cost. And because the API key is shared across Clio, local apps and production, the OpenAI dashboard cannot separate them. The code already prefers a dedicated `OPENAI_API_KEY_CLIO` env var when present; it just isn't being used.

### 6. Current breakage affecting cost (both directions)

- **486 dead-lettered deltas** sit in the local spool from the 1–5 Aug SSH outage. If they are retried, that is ~500 distillation calls in one burst (~$5–8 at gpt-4.1 rates) — do it *after* any model swap, or accept the knowledge loss and purge.
- ~~`consolidate --if-due` is currently failing on every Stop~~ **Correction (same day):** consolidation is NOT dead — it runs 1–3 times a day (the July threshold change already tamed it from 15–22/day) at negligible cost. What the log actually showed was SSH multiplexing noise on successful runs, plus occasional 120-second timeouts and outright call failures when the shared SSH connection hit the server's 10-session limit. Fixed 7 Aug: Atlas `MaxSessions` raised to 64 and the wedged local control master (stale since 6 Aug) cycled.

## Trade-offs

| Lever | Saving | Risk / cost |
|---|---|---|
| **A. Swap model** (`clio settings set-capture-model …`) | 5–13× on ~95% of spend: gpt-4.1-mini ≈ 5×, gpt-4o-mini ≈ 13×, gpt-5-mini ≈ 7× (prices as of 30 Jul benchmark — recheck) | Mini-class models are **unbenchmarked** for the one property that separated models in July: namespace scoping. The 5-case eval in `capture-model-benchmark.md` must pass first. One command to apply and to revert. |
| **B. Debounce checkpoints** (hook change, outside this repo) | ~40–60% fewer calls: only checkpoint when ≥N chars or ≥M minutes accrued, flush remainder on SessionEnd | Slightly staler capture (a crash loses the un-flushed tail — the spool already tolerates this). Larger merged digests distill *better* and always clear the ~2k-token prompt-cache floor. |
| **C. Cap distilled atoms per delta + raise the bar** (prompt/code change) | ~30–50% of output tokens; also reduces DB noise | Risk of dropping a genuinely durable fact from a dense session; mitigate with the review queue rather than a hard drop. |
| **D. Persist `TokenUsage` per checkpoint + dedicated `OPENAI_API_KEY_CLIO`** | No direct saving; makes spend visible and attributable | Small schema/settings change. Without it, every future cost question is another archaeology session. |

The levers compound: A×B×C at the midpoints takes a ~$2/day habit to roughly **$0.10–0.20/day**.

## Risks

- **Quality regression from a cheaper model** — the July benchmark showed namespace scoping is fragile (Luna failed it once even at full price). Never swap without the eval rerun.
- **Pricing drift** — all $ figures here use the benchmark doc's 30 July prices and my estimates from character counts; treat them as ±30%.
- **Backlog burst** — retrying 486 dead letters before swapping the model spends at the old rate.

## Assumptions

- Atlas is the only place distillation runs (local settings bridge everything to it; the 1–5 Aug outage producing zero checkpoints supports this). If another Mac captures directly with its own settings, its model choice needs checking too.
- Codex digests behave like Claude ones (same spool, similar sizes observed).
- chars ÷ 4 ≈ tokens for mixed English/code digests.
- No other tool in this household uses the shared key *under Clio's name* — the daily totals above are Clio-only because they come from Clio's own records, not the OpenAI dashboard.

## Recommendation

Go, in this order:

1. **Benchmark gpt-4.1-mini, gpt-4o-mini and gpt-5-mini** with the existing five-case eval (`capture-model-benchmark.md` documents the method; the runbook has the swap command). Adopt the cheapest one that holds namespace scoping. This is the big win — one settings change, ~5–13× off ~95% of spend.
2. **Debounce the Stop hook** (clio-hooks repo): minimum interval (e.g. 10 min) or minimum accrued delta (e.g. 6k chars) before enqueueing, flush on SessionEnd. Halves call count and stops paying the system prompt per turn.
3. **Cap atoms per checkpoint and tighten the durability bar** in the distillation prompt — cost and memory-noise win together.
4. **Persist token usage per checkpoint and move Atlas to a dedicated `OPENAI_API_KEY_CLIO`** so the next cost question is a query, not a spike.
5. Decide the fate of the 486 dead letters (retry after the swap, or purge), and fix the SSH mux error that is silently killing consolidation.

## Next Steps (if go)

The first slice: run the eval against gpt-4.1-mini on Atlas (`clio distill - --dry-run --metrics` per the benchmark doc). If it scopes namespaces correctly across two repeats, `clio settings set-capture-model gpt-4.1-mini` — reversible in one command, and the metrics CSVs will show the effect within a day.

## Outcome (updated same day, 7 Aug)

Everything except the model swap was implemented on 7 Aug:

- SSH: Atlas `MaxSessions` 10 → 64, wedged control master cycled (bridge calls ~0.16 s, no warnings).
- Queue: 486 dead letters purged (operator-approved; note they collided with the in-flight CLIO-OPS-008 recovery — see roadmap), 22 pending purged after verifying their Codex transcripts never existed.
- Lever B: per-session batching shipped in clio-hooks (`dd0d9ff` in `~/.claude`) — Stops defer under 20k chars, a SessionEnd hook flushes the remainder.
- Lever C: distillation hard-capped at 6 memories with a stricter prompt bar (`a53052d`).
- Lever D: token usage persisted per checkpoint, `clio usage` added (`5675856`, migration 014). Dedicated `OPENAI_API_KEY_CLIO` still needs a key created by the operator.
- Lever A: cheap-model benchmark run and recorded in `capture-model-benchmark.md` — recommendation gpt-4.1-mini, awaiting the operator's go, then deployment of the new binaries to Atlas.

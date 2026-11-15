#!/usr/bin/env python3
"""Capture-model benchmark: ten distillation cases, N repeats per model.

Cases 01-05 reconstruct the 29 July 2026 benchmark
(docs/operations/capture-model-benchmark.md) as checked-in fixtures so a rerun
is a command, not archaeology. Cases 06-10 were added in November 2026 to
grade selectivity in both directions. Three must stay quiet: a busy session
with nothing durable in it, a session that is all point-in-time status, and a
later slice of a long session that only repeats what earlier slices stored.
Two must not lose anything: a later slice that corrects an earlier memory, and
a session with three distinct valuable facts. Runs
`clio distill - --dry-run --metrics --json` per case — dry-run writes nothing —
against a throwaway DB so live settings are untouched. The model call itself is
real and paid; a full 4-model run is a few dozen small calls.

Usage:
  scripts/bench/capture-model/run.py --db /tmp/bench.db [--models m1,m2] [--repeats 2]

The clio binary comes from $CLIO_BIN, defaulting to ./target/debug/clio.
Run from a NON-project directory (or pass --cwd) so namespace resolution shows
the model's own suggestion rather than the working directory's namespace.
"""

import argparse
import json
import os
import re
import statistics
import subprocess
import sys
from pathlib import Path

CASES_DIR = Path(__file__).resolve().parent / "cases"

DEFAULT_MODELS = ["gpt-4.1", "gpt-4.1-mini", "gpt-4o-mini", "gpt-5-mini"]

# $/1M tokens (input, cached input, output) — snapshot of provider prices when
# this harness was written (Aug 2026). Recheck the OpenAI model pages before
# using the cost column for a budget decision; tokens are the durable numbers.
PRICES = {
    "gpt-4.1": (2.00, 0.50, 8.00),
    "gpt-4.1-mini": (0.40, 0.10, 1.60),
    "gpt-4o-mini": (0.15, 0.075, 0.60),
    "gpt-5-mini": (0.25, 0.025, 2.00),
}

INJECTION_CANARY = "BENCHMARK-INJECTION-CANARY"

# capture::MAX_KNOWLEDGE_ATOMS — the most knowledge items one call may return.
CEILING = 3


def run_case(clio: str, db: str, model: str, text: str, cwd: str) -> dict:
    result = subprocess.run(
        [clio, "--local", "--db-path", db, "--json", "distill", "-",
         "--dry-run", "--metrics", "--model", model],
        input=text, capture_output=True, text=True, timeout=180, cwd=cwd,
    )
    if result.returncode != 0:
        return {"error": result.stderr.strip()[:400]}
    return json.loads(result.stdout)


# Substantive work (a commit) happened, so exactly one receipt is owed.
RECEIPT_REQUIRED = ("02", "05", "07", "09")
# Nothing was changed or produced, so a receipt would be noise.
RECEIPT_FORBIDDEN = ("04", "06", "08", "10")


def judge(case_id: str, memories: list[dict], returned=None) -> list[str]:
    """Return a list of failed-check descriptions (empty = clean).

    `memories` is what would be stored, after the parser's cap. `returned` is
    how many knowledge items the model itself returned before that cap; only
    it can show whether the model obeyed the ceiling.
    """
    fails = []
    kinds = [m.get("kind") for m in memories]
    namespaces = [m.get("namespace", "") for m in memories]
    blob = json.dumps(memories).lower()

    if INJECTION_CANARY.lower() in blob:
        fails.append("followed the injected instruction")

    knowledge = [m for m in memories if m.get("kind") != "receipt"]
    knowledge_blob = json.dumps(knowledge).lower()
    if returned is not None and returned > CEILING:
        fails.append(f"ceiling ignored: model returned {returned} knowledge items")

    receipts = kinds.count("receipt")
    if case_id.startswith(RECEIPT_REQUIRED) and receipts != 1:
        fails.append(f"expected exactly 1 receipt, got {receipts}")
    elif case_id.startswith(RECEIPT_FORBIDDEN) and receipts:
        fails.append("receipt for a session with no substantive work")
    elif receipts > 1:
        fails.append(f"{receipts} receipts")
    # A decision or constraint that expires after 30 days is lost knowledge.
    if not case_id.startswith("07") and any(
        m.get("lifetime") == "status" and m.get("kind") in ("decision", "constraint")
        for m in memories
    ):
        fails.append("durable knowledge marked as status")

    if case_id.startswith("01"):
        if "decision" not in kinds:
            fails.append("no decision captured")
        if any(ns == "global" for ns in namespaces):
            fails.append("project decision promoted to global")
    elif case_id.startswith("02"):
        if not any(k in ("constraint", "fact") for k in kinds):
            fails.append("invariant not captured as constraint/fact")
        if any(ns == "global" for ns in namespaces):
            fails.append("project invariant promoted to global")
    elif case_id.startswith("03"):
        if not memories:
            fails.append("durable preference dropped")
        # Cross-project preference: global is the CORRECT scope here.
        elif not any(ns == "global" for ns in namespaces):
            fails.append("cross-project preference not promoted to global")
    elif case_id.startswith(("04", "06")):
        if knowledge:
            fails.append(f"routine session produced {len(knowledge)} knowledge item(s)")
    elif case_id.startswith("07"):
        # Nothing here outlives the day. The receipt (checked above) is the
        # durable record; storing no status item at all is acceptable.
        durable = [m for m in knowledge if m.get("lifetime") != "status"]
        if durable:
            fails.append(f"{len(durable)} status note(s) stored as durable knowledge")
    elif case_id.startswith("08"):
        if knowledge:
            fails.append(f"restated {len(knowledge)} memory(ies) the session already stored")
    elif case_id.startswith("09"):
        # The stored limit of 30 is now wrong; the correction must survive
        # the instruction not to repeat what is already stored.
        text = " ".join(
            str(m.get(field, "")) for m in knowledge for field in ("title", "summary", "content"))
        if not re.search(r"\b20\b", text):
            fails.append("correction to an already-stored memory was dropped")
    elif case_id.startswith("10"):
        wanted = {
            "money held as integer pence": ("pence", "integer", "minor unit"),
            "webhooks arrive out of order": ("out of order", "out-of-order", "arrival order",
                                             "created"),
            "export must finish before 02:00": ("02:00", "2am", "2 am"),
        }
        for fact, words in wanted.items():
            if not any(w in knowledge_blob for w in words):
                fails.append(f"valuable fact dropped: {fact}")
        if len(knowledge) < 3:
            fails.append(f"three distinct facts stored as {len(knowledge)} memory(ies)")
    elif case_id.startswith("05"):
        if "decision" not in kinds:
            fails.append("queue decision missing")
        if not any(k in ("constraint", "fact") for k in kinds):
            fails.append("API constraint missing")
        if any(ns == "global" for ns in namespaces):
            fails.append("project memory promoted to global")
    return fails


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", required=True, help="throwaway DB path (created if absent)")
    ap.add_argument("--models", default=",".join(DEFAULT_MODELS))
    ap.add_argument("--repeats", type=int, default=2)
    ap.add_argument("--out", help="write full results JSON here")
    ap.add_argument("--cwd", default="/", help="directory to run clio from (non-project)")
    args = ap.parse_args()

    clio = os.environ.get("CLIO_BIN", "./target/debug/clio")
    clio = str(Path(clio).resolve()) if Path(clio).exists() else clio
    cases = sorted(CASES_DIR.glob("*.txt"))
    if not cases:
        print("no cases found", file=sys.stderr)
        return 1

    all_results = []
    total_errors = total_failed_checks = 0
    for model in args.models.split(","):
        model = model.strip()
        latencies, in_tok, cache_tok, out_tok, reason_tok = [], 0, 0, 0, 0
        knowledge_counts = []
        fail_lines, calls, errors = [], 0, 0
        for case in cases:
            text = case.read_text()
            for repeat in range(args.repeats):
                r = run_case(clio, args.db, model, text, args.cwd)
                all_results.append({"model": model, "case": case.stem, "repeat": repeat, "response": r})
                if "error" in r:
                    errors += 1
                    fail_lines.append(f"  {case.stem} r{repeat}: ERROR {r['error'][:120]}")
                    continue
                calls += 1
                u = r.get("usage", {})
                latencies.append(r.get("elapsed_ms", 0))
                in_tok += u.get("input_tokens", 0)
                cache_tok += u.get("cached_input_tokens", 0)
                out_tok += u.get("output_tokens", 0)
                reason_tok += u.get("reasoning_tokens", 0)
                # Older binaries do not report the pre-cap count; fall back
                # to the stored list, which then understates an overrun.
                returned = r.get("knowledge_items_returned")
                knowledge_counts.append(
                    returned if returned is not None else
                    sum(1 for m in r.get("result", []) if m.get("kind") != "receipt"))
                for fail in judge(case.stem, r.get("result", []), returned):
                    fail_lines.append(f"  {case.stem} r{repeat}: {fail}")

        cost = ""
        if model in PRICES and calls:
            pin, pcache, pout = PRICES[model]
            est = ((in_tok - cache_tok) * pin + cache_tok * pcache + out_tok * pout) / 1e6
            cost = f"${est:.4f}"
        lat = f"{statistics.mean(latencies):,.0f} ms" if latencies else "-"
        print(f"\n== {model}: {calls} calls, {errors} errors, mean latency {lat}")
        print(f"   tokens: {in_tok:,} in ({cache_tok:,} cached), {out_tok:,} out, "
              f"{reason_tok:,} reasoning   est cost {cost}")
        if knowledge_counts:
            print(f"   knowledge items returned per call, before the cap: "
                  f"{sorted(knowledge_counts)} "
                  f"(mean {statistics.mean(knowledge_counts):.1f}, "
                  f"{knowledge_counts.count(0)} empty, "
                  f"{sum(1 for c in knowledge_counts if c == CEILING)} at the ceiling of {CEILING}, "
                  f"{sum(1 for c in knowledge_counts if c > CEILING)} over it)")
        print("   PASS" if not fail_lines else "\n".join(["   FAILED CHECKS:"] + fail_lines))
        total_errors += errors
        total_failed_checks += len(fail_lines) - errors

    if args.out:
        Path(args.out).write_text(json.dumps(all_results, indent=1))
        print(f"\nfull results -> {args.out}")

    # Non-zero exit so automated gating cannot accept a broken run or model:
    # 2 for call errors (harness/provider failure), 1 for failed judge checks.
    if total_errors:
        return 2
    if total_failed_checks:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

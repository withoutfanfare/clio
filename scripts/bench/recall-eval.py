#!/usr/bin/env python3
"""Score retrieval quality, so linking and scoring settings can be tuned on outcome.

Structural measures — link counts, degree distributions — say nothing about whether
retrieval returns the right memories. This does, and its central question decides
whether auto-linking earns its keep:

    of the memories that --include-links ADDS to a brief, what fraction are relevant?

If that beats the chance baseline, links are contributing context. If it matches
chance, they are padding briefs with noise however tidy the graph looks.

Method: hold-one-out over real memories. For memory M the query is M's title, and
the relevant set is other memories in M's namespace sharing at least two tags with
M. Two structural caveats, on top of the tag-proxy one below:

- The FTS query is M's own title with implicit AND between terms, so the baseline
  seed set is usually little more than M itself, and --include-links then expands
  M's own neighbours. The added-items figure therefore leans towards measuring
  embedding-tag agreement rather than end-to-end retrieval.
- Tags are model-assigned labels, partially correlated with the embeddings being
  tested. Every figure is reported against a random same-namespace baseline, but
  treat results as a way to RANK configurations, never as absolute relevance.

Both arms are scored as PAIRS: a query counts only when the with-links and
without-links invocations both succeeded and both returned at least one result,
so the two means cover the same query set. Failed invocations and empty pairs
are counted and reported, never silently dropped.

Always run against a COPY of a database: briefs are not read-only — recall bumps
access counts, which are themselves scoring inputs. The script refuses the live
database path unless CLIO_EVAL_ALLOW_LIVE=1 is set.

Usage:
    scripts/bench/recall-eval.py <db-path> [n-queries]

Environment:
    CLIO                  path to the clio binary (default ~/.cargo/bin/clio)
    CLIO_SEED             RNG seed for query selection (default 20260730)
    CLIO_EVAL_ALLOW_LIVE  set to 1 to permit running against the live db path
"""

import json
import os
import random
import sqlite3
import statistics
import subprocess
import sys

CLIO = os.environ.get("CLIO", os.path.expanduser("~/.cargo/bin/clio"))
random.seed(int(os.environ.get("CLIO_SEED", "20260730")))

# Queries are drawn from substantive kinds only. Receipts and summaries describe
# sessions rather than ideas, so scoring retrieval against them measures the wrong
# thing.
QUERY_KINDS = ("decision", "constraint", "fact", "observation")
MIN_NAMESPACE_SIZE = 40  # below this a namespace has too few neighbours to score
MIN_RELEVANT = 3  # a query with fewer known-relevant neighbours is too noisy
MIN_TITLE_CHARS = 20


FAILURES = []  # (reason, detail) per failed invocation, reported at the end


def brief_ids(db, query, namespace, include_links):
    """Return the memory ids a brief surfaces, in order, deduplicated.

    Returns None on an execution failure (non-zero exit, undecodable output,
    timeout) — distinct from [], which is a brief that legitimately found
    nothing. Callers must treat None as "this query cannot be scored".
    """
    args = [
        CLIO, "--db-path", db, "--local", "brief",
        "--preset", "custom", "--query", query,
        "--namespace", namespace, "--json",
    ]
    if include_links:
        args.append("--include-links")
    try:
        result = subprocess.run(args, capture_output=True, text=True, timeout=180)
    except subprocess.TimeoutExpired:
        FAILURES.append(("timeout", query[:60]))
        return None
    if result.returncode != 0:
        FAILURES.append(("exit %d" % result.returncode, result.stderr.strip()[-200:]))
        return None
    try:
        brief = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        FAILURES.append(("bad json", str(exc)))
        return None

    ordered, seen = [], set()
    for section in brief.get("sections", []):
        for item in section.get("items", []):
            mid = item.get("id")
            if mid and mid not in seen:
                seen.add(mid)
                ordered.append(mid)
    return ordered


def load_corpus(db):
    conn = sqlite3.connect(db)
    tags = {}
    for mid, tag in conn.execute("select memory_id, tag from memory_tags"):
        tags.setdefault(mid, set()).add(tag)
    meta, by_namespace = {}, {}
    for mid, ns, kind, title in conn.execute(
        "select id, namespace, kind, title from memories where archived_at is null"
    ):
        meta[mid] = {"ns": ns, "kind": kind, "title": title or ""}
        by_namespace.setdefault(ns, []).append(mid)
    conn.close()
    return tags, meta, by_namespace


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    db = sys.argv[1]
    wanted = int(sys.argv[2]) if len(sys.argv) > 2 else 60

    # Both platform defaults: Linux XDG and the macOS Application Support path.
    live_paths = [
        os.path.expanduser("~/.local/share/clio/memory.db"),
        os.path.expanduser("~/Library/Application Support/clio/memory.db"),
    ]
    hit = next(
        (p for p in live_paths if os.path.realpath(db) == os.path.realpath(p)), None
    )
    if hit and not os.environ.get("CLIO_EVAL_ALLOW_LIVE"):
        sys.exit(
            "Refusing to run against the live database: briefs bump access counts,\n"
            "which are scoring inputs. Copy it first —\n"
            '  sqlite3 "%s" ".backup /tmp/eval.db"\n'
            "— or set CLIO_EVAL_ALLOW_LIVE=1 to override." % hit
        )

    tags, meta, by_namespace = load_corpus(db)

    pool = [
        mid for mid, m in meta.items()
        if m["kind"] in QUERY_KINDS
        and len(tags.get(mid, ())) >= 2
        and len(by_namespace[m["ns"]]) >= MIN_NAMESPACE_SIZE
        and len(m["title"]) >= MIN_TITLE_CHARS
    ]
    random.shuffle(pool)

    def relevant_to(mid):
        mine = tags.get(mid, set())
        return {
            other for other in by_namespace[meta[mid]["ns"]]
            if other != mid and len(mine & tags.get(other, set())) >= 2
        }

    queries = []
    for mid in pool:
        rel = relevant_to(mid)
        if len(rel) >= MIN_RELEVANT:
            queries.append((mid, rel))
        if len(queries) >= wanted:
            break

    if not queries:
        sys.exit("No usable queries — corpus too small or too sparsely tagged.")

    print("Queries selected: %d (each with >=%d tag-relevant neighbours, seed %s)"
          % (len(queries), MIN_RELEVANT, os.environ.get("CLIO_SEED", "20260730")))

    scores = {False: [], True: []}
    added_relevant = added_total = 0
    added_per_brief = []
    scored_queries = []
    empty_pairs = failed = 0

    for mid, rel in queries:
        query, namespace = meta[mid]["title"], meta[mid]["ns"]
        without_raw = brief_ids(db, query, namespace, False)
        with_raw = brief_ids(db, query, namespace, True)
        if without_raw is None or with_raw is None:
            failed += 1
            continue
        without = [i for i in without_raw if i != mid]
        with_links = [i for i in with_raw if i != mid]
        # Paired scoring: a query counts only when both arms produced something,
        # so the two means describe the same query set. Admitting arms
        # independently lets the without-links mean be taken over a different
        # (and systematically easier) subset than the with-links mean.
        if not without or not with_links:
            empty_pairs += 1
            continue

        scored_queries.append((mid, rel))
        for flag, got in ((False, without), (True, with_links)):
            precision = sum(1 for i in got if i in rel) / len(got)
            reciprocal_rank = next(
                (1 / (n + 1) for n, i in enumerate(got) if i in rel), 0.0
            )
            scores[flag].append((precision, reciprocal_rank, len(got)))

        extra = [i for i in with_links if i not in without]
        added_per_brief.append(len(extra))
        added_total += len(extra)
        added_relevant += sum(1 for i in extra if i in rel)

    print("Scored as pairs: %d   dropped: %d with an empty arm, %d failed invocations"
          % (len(scored_queries), empty_pairs, failed))
    if FAILURES:
        reason, detail = FAILURES[0]
        print("  first failure (%s): %s" % (reason, detail or "<no stderr>"))
    if not scored_queries:
        sys.exit("Nothing scored — every query failed or returned an empty arm.")

    chance = statistics.mean(
        len(rel) / (len(by_namespace[meta[mid]["ns"]]) - 1)
        for mid, rel in scored_queries
        if len(by_namespace[meta[mid]["ns"]]) > 1
    )
    print("Chance baseline: a random same-namespace memory is relevant %.1f%% of the time"
          % (100 * chance))
    print()

    row = "%-16s %9s %11s %8s"
    print(row % ("brief", "results", "precision", "MRR"))
    for flag, label in ((False, "without links"), (True, "with links")):
        rows = scores[flag]
        print("%-16s %9.1f %10.1f%% %8.3f" % (
            label,
            statistics.mean(r[2] for r in rows),
            100 * statistics.mean(r[0] for r in rows),
            statistics.mean(r[1] for r in rows),
        ))

    print()
    print("Memories that --include-links added:")
    print("  %d across %d scored queries (mean %.1f per brief)"
          % (added_total, len(scored_queries), statistics.mean(added_per_brief)))
    if not added_total:
        print("  none — linked recall is contributing nothing at this setting.")
        return
    precision = added_relevant / added_total
    without_precision = statistics.mean(r[0] for r in scores[False])
    lift = precision / chance if chance else 0
    print("  %.1f%% relevant (micro-average over added items; the table rows above"
          % (100 * precision))
    print("  are per-query means, so the figures are not directly comparable)")
    print("  vs the without-links arm at %.1f%% and chance at %.1f%% (%.1fx)"
          % (100 * without_precision, 100 * chance, lift))
    if precision <= chance:
        print("  Verdict: no better than chance. Links are padding briefs.")
    elif precision <= without_precision:
        print("  Verdict: above chance but below the baseline arm — links dilute briefs.")
    else:
        print("  Verdict: links are contributing context beyond the baseline arm.")


if __name__ == "__main__":
    main()

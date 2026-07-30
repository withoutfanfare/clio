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
M. Tags are model-assigned labels, so they are an imperfect proxy for relevance and
partially correlated with the embeddings — which is why every figure is reported
against a random baseline drawn from the same namespace. Treat this as a way to rank
configurations against each other, not as absolute relevance.

Always run against a COPY of a database. Briefs are read-only, but a copy keeps
access-count side effects off live scoring.

Usage:
    scripts/bench/recall-eval.py <db-path> [n-queries]

Environment:
    CLIO        path to the clio binary (default ~/.cargo/bin/clio)
    CLIO_SEED   RNG seed for query selection (default 20260730)
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


def brief_ids(db, query, namespace, include_links):
    """Return the memory ids a brief surfaces, in order, deduplicated."""
    args = [
        CLIO, "--db-path", db, "--local", "brief",
        "--preset", "custom", "--query", query,
        "--namespace", namespace, "--json",
    ]
    if include_links:
        args.append("--include-links")
    result = subprocess.run(args, capture_output=True, text=True)
    if result.returncode != 0:
        return []
    try:
        brief = json.loads(result.stdout)
    except json.JSONDecodeError:
        return []

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

    print("Queries: %d (each with >=%d tag-relevant neighbours)" % (len(queries), MIN_RELEVANT))

    chance = statistics.mean(
        len(rel) / (len(by_namespace[meta[mid]["ns"]]) - 1)
        for mid, rel in queries
        if len(by_namespace[meta[mid]["ns"]]) > 1
    )
    print("Chance baseline: a random same-namespace memory is relevant %.1f%% of the time"
          % (100 * chance))
    print()

    scores = {False: [], True: []}
    added_relevant = added_total = 0
    added_per_brief = []

    for mid, rel in queries:
        query, namespace = meta[mid]["title"], meta[mid]["ns"]
        without = [i for i in brief_ids(db, query, namespace, False) if i != mid]
        with_links = [i for i in brief_ids(db, query, namespace, True) if i != mid]

        for flag, got in ((False, without), (True, with_links)):
            if got:
                precision = sum(1 for i in got if i in rel) / len(got)
                reciprocal_rank = next(
                    (1 / (n + 1) for n, i in enumerate(got) if i in rel), 0.0
                )
                scores[flag].append((precision, reciprocal_rank, len(got)))

        extra = [i for i in with_links if i not in without]
        added_per_brief.append(len(extra))
        added_total += len(extra)
        added_relevant += sum(1 for i in extra if i in rel)

    row = "%-16s %9s %11s %8s"
    print(row % ("brief", "results", "precision", "MRR"))
    for flag, label in ((False, "without links"), (True, "with links")):
        rows = scores[flag]
        if not rows:
            print(row % (label, 0, "-", "-"))
            continue
        print("%-16s %9.1f %10.1f%% %8.3f" % (
            label,
            statistics.mean(r[2] for r in rows),
            100 * statistics.mean(r[0] for r in rows),
            statistics.mean(r[1] for r in rows),
        ))

    print()
    print("Memories that --include-links added:")
    print("  %d across %d queries (mean %.1f per brief)"
          % (added_total, len(queries), statistics.mean(added_per_brief)))
    if not added_total:
        print("  none — linked recall is contributing nothing at this setting.")
        return
    precision = added_relevant / added_total
    lift = precision / chance if chance else 0
    print("  %.1f%% relevant against a %.1f%% chance baseline — lift %.1fx"
          % (100 * precision, 100 * chance, lift))
    if precision > chance * 1.5:
        print("  Verdict: links are contributing relevant context.")
    elif precision > chance:
        print("  Verdict: marginally better than chance — weak contribution.")
    else:
        print("  Verdict: no better than chance. Links are padding briefs.")


if __name__ == "__main__":
    main()

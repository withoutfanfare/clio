#!/usr/bin/env python3
"""Prototype: replace clusters of auto-captured fragments with one topic note each.

Works only on a COPY of a snapshot: reads <private>/snapshot and writes only to
--out, which must not be the snapshot itself. Three steps:

  cluster    group active captured fragments per namespace by meaning (greedy, centroid >= --threshold)
  synth      a model writes one note per cluster of --min-size or more (resumable; notes.jsonl)
  build      backup the snapshot to --out, insert the notes, archive their fragments, embed the notes
             (--mode prune instead keeps each cluster's newest fragment and archives the rest; no model)

  uv run --with numpy scripts/bench/topic-notes.py cluster --private "$PRIV"
  scripts/bench/topic-notes.py synth --private "$PRIV" --jobs 6
  scripts/bench/topic-notes.py build --private "$PRIV" --clio <clio binary> --mode notes --out "$PRIV/snapshot-topics"

Fragments are archived, never deleted. Each note's metadata lists its fragments
and their source_refs, so resume-eval can hide a note built from a prompt's own session.
"""

import argparse
import collections
import concurrent.futures
import json
import os
import shutil
import sqlite3
import subprocess
import sys
import uuid

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import judge  # noqa: E402
import importlib.util  # noqa: E402

_spec = importlib.util.spec_from_file_location("resume_eval", os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                                                            "resume-eval.py"))
resume_eval = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(resume_eval)

CAPTURED = ("claude-session", "codex-session", "claude-code-session")
# Constraints, tasks and receipts carry their own lifecycle (attention items, session start); left alone.
KINDS = ("fact", "decision", "observation", "summary", "note")
SOURCE = "clio-topic"
CUT = 500  # characters of each fragment's content the model sees

PROMPT = """You maintain a memory store for an AI coding assistant. Each cluster below is a set of \
notes captured automatically from past sessions in one project, oldest first. They overlap and \
some are out of date. For each cluster write ONE topic note that replaces them all.

Rules:
- Keep every durable fact, decision (with its reason), gotcha and how-to that a future session could use.
- Where notes disagree, the newer one wins; say what changed only if the change itself matters.
- Leave out point-in-time status ("no tasks found", "tests passed today", "awaiting review") unless it is still the latest state of something.
- If the whole cluster is only such status noise with nothing durable, return {{"drop": true}} for it.
- title: at most 100 characters, names the topic. summary: at most 300 characters, the gist as it stands now.
- content: short bullet lines ("- "), at most 1,500 characters. British English. Do not invent anything.
- kind: "decision" if the topic is mainly a decision, otherwise "fact".

Reply with only a JSON object mapping each cluster label to its note, e.g.
{{"C1": {{"title": "...", "summary": "...", "content": "- ...", "kind": "fact"}}, "C2": {{"drop": true}}}}

{clusters}
"""


def die(msg):
    sys.exit(f"topic-notes: {msg}")


def ro(db):
    return sqlite3.connect(f"file:{db}?immutable=1", uri=True)


def snapshot_db(private):
    return os.path.join(private, "snapshot", "memory.db")


def cmd_cluster(args):
    if os.path.exists(os.path.join(args.private, "topics", "notes.jsonl")):
        # Cluster ids are positional: new clusters would silently inherit old notes.
        die("topics/notes.jsonl exists; move it aside before clustering again")
    import numpy as np
    conn = ro(snapshot_db(args.private))
    rows = conn.execute(
        f"select m.id, m.namespace, e.embedding from memories m join memory_embeddings e on e.memory_id = m.id "
        f"where m.archived_at is null and m.source in {CAPTURED} and m.kind in {KINDS} "
        "and length(e.embedding) = 1536 order by m.created_at").fetchall()
    by_ns = collections.defaultdict(list)
    for row in rows:
        by_ns[row[1]].append(row)
    clusters = []
    for ns, items in by_ns.items():
        vectors = np.stack([np.frombuffer(i[2], dtype=np.float32) for i in items]).copy()
        vectors /= np.linalg.norm(vectors, axis=1, keepdims=True) + 1e-9
        centroids, members = [], []
        for k in range(len(items)):
            if centroids:
                c = np.stack(centroids)
                sims = (c / np.linalg.norm(c, axis=1, keepdims=True)) @ vectors[k]
                sims[[len(m) >= args.cap for m in members]] = -1
                j = int(sims.argmax())
                if sims[j] >= args.threshold:
                    members[j].append(k)
                    centroids[j] = centroids[j] + vectors[k]
                    continue
            centroids.append(vectors[k].copy())
            members.append([k])
        clusters += [{"namespace": ns, "ids": [items[k][0] for k in m]} for m in members if len(m) >= args.min_size]
    for number, cluster in enumerate(clusters, 1):
        cluster["cid"] = f"t{number:05d}"
    path = os.path.join(args.private, "topics", "clusters.json")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as handle:
        json.dump(clusters, handle)
    print(f"{len(rows)} fragments; {len(clusters)} clusters of {args.min_size}+ holding "
          f"{sum(len(c['ids']) for c in clusters)} fragments -> {path}")


def fragment_text(conn, memory_id):
    kind, created, title, summary, content = conn.execute(
        "select kind, created_at, title, summary, content from memories where id = ?", (memory_id,)).fetchone()
    body = " ".join((content or "").split())[:CUT]
    return f"[{created[:10]} {kind}] {title or ''}: {summary or ''} | {body}"


def cmd_synth(args):
    clusters = json.load(open(os.path.join(args.private, "topics", "clusters.json")))
    out_path = os.path.join(args.private, "topics", "notes.jsonl")
    done = set()
    if os.path.exists(out_path):
        done = {json.loads(line)["cid"] for line in open(out_path)}
    todo = [c for c in clusters if c["cid"] not in done][: args.limit or None]
    batches = [todo[i:i + args.batch] for i in range(0, len(todo), args.batch)]
    db = snapshot_db(args.private)

    def run(batch):
        conn = ro(db)
        text = "\n\n".join(f"CLUSTER C{i} ({c['namespace']})\n" + "\n".join(fragment_text(conn, m) for m in c["ids"])
                           for i, c in enumerate(batch, 1))
        reply, cost = judge.ask(PROMPT.format(clusters=text), model=args.model)
        return [dict(cid=c["cid"], **(reply.get(f"C{i}") or {"error": "missing"})) for i, c in enumerate(batch, 1)], cost

    cost, failed = 0.0, 0
    with open(out_path, "a") as handle, concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        for index, future in enumerate(concurrent.futures.as_completed([pool.submit(run, b) for b in batches]), 1):
            try:
                notes, spent = future.result()
            except Exception as exc:  # the batch stays undone; a rerun picks it up
                failed += 1
                print(f"\nfailed batch: {exc}", file=sys.stderr)
                continue
            cost += spent
            for note in notes:
                if not note.get("drop") and not (note.get("title") and note.get("content")):
                    print(f"\nincomplete note for {note['cid']}; left for a rerun", file=sys.stderr)
                    continue
                if "error" not in note:
                    handle.write(json.dumps(note, ensure_ascii=False) + "\n")
            handle.flush()
            print(f"\rsynth {index}/{len(batches)}  ${cost:.2f}", end="", file=sys.stderr)
    print(f"\n{len(todo)} clusters in {len(batches)} batches, ${cost:.2f}, {failed} batches failed")


def cmd_build(args):
    out_db = os.path.join(args.out, "memory.db")
    if os.path.exists(out_db):
        die(f"{out_db} exists; remove it first")
    src = ro(snapshot_db(args.private))
    settings_path = os.path.join(args.private, "snapshot", "clio-settings.json")
    if args.mode == "notes":
        # Checked before anything is written, so a failure leaves nothing to clean up. Backfill
        # re-embeds every memory (archived too) lacking a vector from the configured model;
        # only the notes may gain one, or the notes arm differs by more than its notes.
        model = (json.load(open(settings_path)).get("embeddings") or {}).get("model", "all-MiniLM-L6-v2")
        missing, sizes = src.execute(
            "select (select count(*) from memories m where not exists (select 1 from memory_embeddings e "
            "where e.memory_id = m.id and e.model = ?1)), "
            "(select count(distinct dimensions) from memory_embeddings where model = ?1)", (model,)).fetchone()
        if missing or sizes > 1:
            die(f"{missing} memories without a {model} vector, {sizes} vector sizes for it; backfill would embed "
                "more than the notes, so the notes arm would differ from the others by more than its notes")
    os.makedirs(args.out, exist_ok=True)
    dst = sqlite3.connect(out_db)
    src.backup(dst)  # backup, never .dump: the full-text index is keyed on row number
    shutil.copyfile(settings_path, os.path.join(args.out, "clio-settings.json"))
    clusters = {c["cid"]: c for c in json.load(open(os.path.join(args.private, "topics", "clusters.json")))}
    notes = {}
    if args.mode == "notes":
        for line in open(os.path.join(args.private, "topics", "notes.jsonl")):
            note = json.loads(line)
            notes[note["cid"]] = note
    stamp = "2026-11-15T00:00:00.000Z"  # the snapshot's date: archived before any prompt runs
    added = archived = dropped = 0
    with dst:
        for cid, cluster in clusters.items():
            ids = cluster["ids"]
            marks = ",".join("?" * len(ids))
            if args.mode == "prune":
                keep = dst.execute(f"select id from memories where id in ({marks}) order by created_at desc limit 1",
                                   ids).fetchone()[0]
                archived += dst.execute(f"update memories set archived_at = ? where id in ({marks}) and id != ?",
                                        [stamp, *ids, keep]).rowcount
                # resume-eval restores these if it has to hide the survivor
                dst.execute("update memories set metadata_json = json_set(coalesce(metadata_json, '{}'), '$.pruned', "
                            "json(?)) where id = ?", (json.dumps([i for i in ids if i != keep]), keep))
                continue
            note = notes.get(cid)
            if note and note.get("drop"):
                dropped += dst.execute(f"update memories set archived_at = ? where id in ({marks})", [stamp, *ids]).rowcount
                continue
            if not note or not (note.get("title") and note.get("content")):
                continue  # not synthesised, or incomplete: fragments stay as they are
            created, updated, importance, access, last = dst.execute(
                f"select max(created_at), max(updated_at), max(importance), max(access_count), max(last_accessed_at) "
                f"from memories where id in ({marks})", ids).fetchone()
            refs = [r[0] for r in dst.execute(f"select source_ref from memories where id in ({marks})", ids)]
            tags = sorted({t for (text,) in dst.execute(f"select tags_text from memories where id in ({marks})", ids)
                           for t in text.split() if t})[:12]
            dst.execute(
                "insert into memories (id, namespace, kind, title, summary, content, tags_text, source, source_ref, "
                "importance, metadata_json, created_at, updated_at, last_accessed_at, access_count) "
                "values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                # the text is in the ID so a rewritten note never inherits the old one's relevance labels
                (str(uuid.uuid5(uuid.NAMESPACE_URL, "clio-topic:" + ",".join(sorted(ids)) + json.dumps(
                    [note.get("title"), note.get("summary"), note.get("content")]))),
                 cluster["namespace"], note.get("kind") if note.get("kind") in ("fact", "decision") else "fact",
                 (note.get("title") or "")[:240], (note.get("summary") or "")[:1000], note.get("content") or note.get("summary") or "",
                 " ".join(tags), SOURCE, f"topic:{cid}", importance,
                 json.dumps({"fragments": ids, "fragment_refs": refs}), created, updated, last, access))
            added += 1
            # The model saw only the first CUT characters; a longer fragment stays active beside the note.
            seen = [i for i, c in dst.execute(f"select id, content from memories where id in ({marks})", ids)
                    if len(" ".join((c or "").split())) <= CUT]
            archived += dst.execute(f"update memories set archived_at = ? where id in ({','.join('?' * len(seen))})",
                                    [stamp, *seen]).rowcount
    dst.close()
    print(f"{args.mode}: {added} notes added, {archived} fragments archived, {dropped} archived as noise")
    if added:
        env = dict(os.environ, CLIO_DB_PATH=out_db)
        # Same embedding model cache as resume-eval, so nothing is downloaded twice.
        models = os.path.join(args.private, "models")
        if not os.path.exists(os.path.join(args.out, "models")):
            os.symlink(models, os.path.join(args.out, "models"))
        subprocess.run([args.clio, "--db-path", out_db, "--local", "embed", "backfill", "--batch-size", "1000000"], env=env, check=True)
        os.remove(os.path.join(args.out, "models"))
    conn = sqlite3.connect(out_db)
    conn.execute("pragma wal_checkpoint(truncate)")
    conn.execute("pragma journal_mode=delete")
    conn.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    for name in ("cluster", "synth", "build"):
        p = sub.add_parser(name)
        p.add_argument("--private", required=True)
    sub.choices["cluster"].add_argument("--threshold", type=float, default=0.70)
    sub.choices["cluster"].add_argument("--min-size", type=int, default=3)
    sub.choices["cluster"].add_argument("--cap", type=int, default=30)
    sub.choices["synth"].add_argument("--model", default="haiku")
    sub.choices["synth"].add_argument("--batch", type=int, default=6)
    sub.choices["synth"].add_argument("--jobs", type=int, default=6)
    sub.choices["synth"].add_argument("--limit", type=int, default=0, help="clusters, for a pilot")
    sub.choices["build"].add_argument("--clio", required=True)
    sub.choices["build"].add_argument("--mode", choices=("notes", "prune"), default="notes")
    sub.choices["build"].add_argument("--out", required=True)
    args = parser.parse_args()
    os.umask(0o077)  # clusters, notes and store copies hold memory contents
    args.private = resume_eval.private_dir(args.private)
    if args.command == "build" and resume_eval.git_repo_above(os.path.realpath(args.out)):
        die(f"--out {args.out} is inside a git repository; store copies are private data")
    if not os.path.isfile(snapshot_db(args.private)):
        die(f"no snapshot at {snapshot_db(args.private)}")
    if args.command == "build" and os.path.realpath(os.path.join(args.out, "memory.db")) == os.path.realpath(
            snapshot_db(args.private)):
        die("--out must not be the snapshot itself")
    {"cluster": cmd_cluster, "synth": cmd_synth, "build": cmd_build}[args.command](args)


if __name__ == "__main__":
    main()

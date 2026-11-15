#!/usr/bin/env python3
"""Score what Clio actually hands a model on the first prompt of a session.

recall-eval.py scores `clio brief` against a tag-overlap proxy. This one replays
the path that matters — session start, then the first prompt, the way the hook
scripts do — over a fixed set of real prompts, and scores the injected text
against judgements stored once in a labels file. It also drives the MCP tools a
model would reach for (`memory_recall`, `memory_search`) over stdio.

An ARM is a binary plus how the query is prepared. Every prompt of every arm
runs on its own fresh copy of one frozen snapshot, because `resume` records what
it showed and `memory_recall` bumps access counts, both of which feed later
results.

    resume-eval.py run      --private DIR --arms arms.json [--runs 2]
    resume-eval.py report   --private DIR [--results DIR/results/<name>]
    resume-eval.py unjudged --private DIR [--results ...]   # what still needs a label
    resume-eval.py prompts  --private DIR --clio BIN [--days 30]  # candidate prompts
    resume-eval.py selftest

DIR holds private data and must not be inside a git repository:

    DIR/snapshot/memory.db           frozen copy of the store (sqlite .backup, never .dump)
    DIR/snapshot/clio-settings.json  the scoring/embedding settings the store runs with
    DIR/prompts.jsonl                {"id","namespace","category","expect_nothing","prompt","session"}
    DIR/labels.jsonl                 {"prompt_id","memory_id","label"}
    DIR/models/                      local embedding model cache (downloaded on first use)
    DIR/results/<name>/              raw per-prompt output of each arm and run, plus report.txt

Labels: relevant, partly, stale (on topic but superseded or a dead status note),
irrelevant. A memory with no label is UNJUDGED: counted on its own, never as
irrelevant. See README.md for arms, labelling and the limits of all this.
"""

import argparse
import array
import collections
import glob
import hashlib
import json
import math
import os
import random
import re
import shutil
import sqlite3
import statistics
import subprocess
import sys
import tempfile
import threading
import time

GAIN = {"relevant": 2, "partly": 1, "stale": 0, "irrelevant": 0}
SLOTS = 7  # task-matched slots a model gets today; rel@7 and nDCG@7 are cut here
MATCHED_HEADING = "Relevant knowledge"  # the one resume section matched to the prompt
UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
SECRETISH = re.compile(
    r"sk-[A-Za-z0-9_-]{20,}|BEGIN [A-Z ]*PRIVATE KEY|Bearer\s+[A-Za-z0-9._-]{20,}|"
    r"(password|passwd|secret|api[_-]?key|token)\s*[:=]\s*\S{6,}|"
    r"\b(?=[A-Za-z0-9]*\d)(?=[A-Za-z0-9]*[A-Za-z])[A-Za-z0-9]{32,}\b",
    re.I,
)


def die(message):
    sys.exit(f"resume-eval: {message}")


# --------------------------------------------------------------------------
# Isolation
# --------------------------------------------------------------------------


def git_repo_above(path):
    """The repository `path` sits in (work tree or bare), or None."""
    current = os.path.realpath(path)
    while True:
        if os.path.exists(os.path.join(current, ".git")) or all(
            os.path.exists(os.path.join(current, name)) for name in ("HEAD", "objects", "refs")
        ):
            return current
        parent = os.path.dirname(current)
        if parent == current:
            return None
        current = parent


def live_database_paths():
    """Every database path Clio may resolve without an explicit flag."""
    paths = [
        os.path.expanduser("~/.local/share/clio/memory.db"),
        os.path.expanduser("~/Library/Application Support/clio/memory.db"),
    ]
    for var, tail in (("CLIO_DB_PATH", ""), ("XDG_DATA_HOME", "clio/memory.db"), ("APPDATA", "clio/memory.db")):
        value = os.environ.get(var, "").strip()
        if value:
            paths.append(os.path.join(os.path.expanduser(value), tail) if tail else os.path.expanduser(value))
    return [os.path.realpath(p) for p in paths]


def private_dir(path):
    path = os.path.realpath(path)
    repo = git_repo_above(path)
    if repo:
        die(f"--private {path} is inside the git repository {repo}; prompts, labels and snapshots are private data")
    if not os.path.isdir(path):
        die(f"--private {path} is not a directory")
    return path


def snapshot_dir(private):
    snap = os.path.join(private, "snapshot")
    db = os.path.join(snap, "memory.db")
    if not os.path.isfile(db):
        die(f"no snapshot at {db} (see README: take one with a read-only sqlite backup)")
    if os.path.realpath(db) in live_database_paths():
        die(f"{db} is a live Clio database path; the snapshot must be a copy")
    wal = db + "-wal"
    if os.path.exists(wal) and os.path.getsize(wal):
        die(f"{wal} is not empty: something has the snapshot open, or it was copied mid-write")
    return snap


def work_copy(private, snap, settings_overrides=None):
    """A fresh, writable copy of the snapshot with its settings and model cache."""
    os.makedirs(os.path.join(private, "work"), exist_ok=True)
    work = tempfile.mkdtemp(dir=os.path.join(private, "work"))
    db = os.path.join(work, "memory.db")
    # APFS clone: instant and copy-on-write. Falls back to a real copy elsewhere.
    if subprocess.run(["cp", "-c", os.path.join(snap, "memory.db"), db], capture_output=True).returncode != 0:
        shutil.copyfile(os.path.join(snap, "memory.db"), db)
    os.chmod(db, 0o600)

    settings = {}
    settings_path = os.path.join(snap, "clio-settings.json")
    if os.path.exists(settings_path):
        with open(settings_path) as handle:
            settings = json.load(handle)
    for key, value in (settings_overrides or {}).items():
        if isinstance(value, dict) and isinstance(settings.get(key), dict):
            settings[key].update(value)
        else:
            settings[key] = value
    provider = (settings.get("embeddings") or {}).get("provider", "local")
    if provider != "local":
        die(f"embeddings.provider is {provider!r}; only the local model is allowed (no paid calls)")
    # Whatever the snapshot's settings said: no shared route, no paid capture, no daemon.
    settings["remote"] = None
    settings["capture"] = {"enabled": False, "api_key": None}
    settings["daemon"] = {"enabled": False, "inbox_paths": []}
    with open(os.path.join(work, "clio-settings.json"), "w") as handle:
        json.dump(settings, handle)

    # Clio looks for the embedding model beside the database it resolves from
    # CLIO_DB_PATH, so without this every copy would download its own.
    models = os.path.join(private, "models")
    os.makedirs(models, exist_ok=True)
    os.symlink(models, os.path.join(work, "models"))
    return work


def hide_own_session(work, session):
    """Archive, in this copy only, what the store later learned from the
    prompt's own session. When that prompt was typed none of it existed; left
    in, every arm is handed the write-up of the very task it is being asked
    to do and looks far better than it is."""
    conn = sqlite3.connect(os.path.join(work, "memory.db"))
    with conn:
        hidden = conn.execute(
            "update memories set archived_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') "
            "where archived_at is null and substr(source_ref, 1, length(?1)) = ?1", (session,)).rowcount
    conn.close()
    return hidden


def child_env(work):
    """A bare environment: no forwarded namespace, no provider keys, and a
    database variable that points at the copy even if a flag were dropped."""
    env = {key: os.environ[key] for key in ("PATH", "HOME", "TMPDIR", "LANG") if key in os.environ}
    env["CLIO_DB_PATH"] = os.path.join(work, "memory.db")
    return env


# --------------------------------------------------------------------------
# Files
# --------------------------------------------------------------------------


def load_jsonl(path):
    rows = []
    with open(path) as handle:
        for number, line in enumerate(handle, 1):
            if line.strip():
                try:
                    rows.append(json.loads(line))
                except json.JSONDecodeError as exc:
                    die(f"{path}:{number}: {exc}")
    return rows


def load_prompts(private):
    path = os.path.join(private, "prompts.jsonl")
    if not os.path.exists(path):
        die(f"no prompt set at {path}")
    prompts = load_jsonl(path)
    for prompt in prompts:
        for key in ("id", "namespace", "prompt"):
            if not prompt.get(key):
                die(f"{path}: a prompt is missing {key!r}")
    if len({p["id"] for p in prompts}) != len(prompts):
        die(f"{path}: duplicate prompt ids")
    return prompts


def load_labels(private):
    """{(prompt_id, memory_id): label}; a later line overrides an earlier one."""
    path = os.path.join(private, "labels.jsonl")
    labels = {}
    if os.path.exists(path):
        for row in load_jsonl(path):
            if row.get("label") not in GAIN:
                die(f"{path}: label {row.get('label')!r} for {row.get('prompt_id')}/{row.get('memory_id')} "
                    f"is not one of {', '.join(GAIN)}")
            labels[(row["prompt_id"], row["memory_id"])] = row["label"]
    return labels


def load_arms(path):
    with open(path) as handle:
        arms = json.load(handle)
    names = [arm.get("name") for arm in arms]
    if len(set(names)) != len(names) or not all(names):
        die(f"{path}: every arm needs a unique name")
    for arm in arms:
        binary = arm.get("clio") if arm.get("kind") == "hook" else arm.get("mcp")
        if arm.get("kind") not in ("hook", "mcp") or not binary or (arm["kind"] == "mcp" and not arm.get("tool")):
            die(f"{path}: arm {arm['name']} needs kind hook (with clio) or mcp (with mcp and tool)")
        if not os.access(binary, os.X_OK):
            die(f"{path}: arm {arm['name']}: {binary} is not an executable")
        if os.path.realpath(binary) == os.path.realpath(shutil.which(os.path.basename(binary)) or ""):
            print(f"warning: arm {arm['name']} uses the {os.path.basename(binary)} on PATH, which may be stale",
                  file=sys.stderr)
    return arms


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


# --------------------------------------------------------------------------
# Hook replay. Mirrors ~/.claude/skills/clio-hooks/scripts as of 14 Nov 2026:
# session_start.py get_resume() and prompt_recall.py run_resume()/render().
# If the hooks change how they render, change render() here to match, or the
# character counts drift (which items are injected does not).
# --------------------------------------------------------------------------


def prepare_query(arm, prompt):
    if arm.get("query_cmd"):  # any extractor: prompt on stdin, query on stdout
        done = subprocess.run(arm["query_cmd"], shell=True, input=prompt, capture_output=True, text=True, timeout=30)
        if done.returncode != 0:
            die(f"arm {arm['name']}: query_cmd failed: {done.stderr.strip()[-200:]}")
        return done.stdout.strip()
    query = " ".join(prompt.split())
    return query[: arm["query_chars"]] if arm.get("query_chars") else query


def render(brief, seen, start=False):
    """The injected markdown, the items in it, and how many of its characters
    are the task-matched section. `seen` drops what session start already showed."""
    blocks, items, matched_chars = [], [], 0
    for section in brief.get("sections", []):
        heading = section.get("heading", "Context")
        kept = [i for i in section.get("items", []) if start or (i.get("memory_id") and i["memory_id"] not in seen)]
        if not kept:
            continue
        lines = [("### " if start else "## ") + heading]
        for item in kept:
            title = item.get("title") or "(untitled)"
            suffix = "" if start else f" ({item['memory_id']})"
            lines.append(f"- [{item.get('kind', 'note')}] **{title}**{suffix}")
            if item.get("reason"):
                lines.append(f"  why: {item['reason']}")
            if item.get("content"):
                lines.append(f"  {item['content']}")
            if item.get("memory_id"):
                items.append({"id": item["memory_id"], "section": heading, "matched": heading == MATCHED_HEADING})
        block = "\n".join(lines)
        if heading == MATCHED_HEADING:
            matched_chars += len(block)
        blocks.append(block)
    if not items:
        return "", [], 0
    header = "## Resume\n\n" if start else "# Clio — relevant to this task\n"
    return (header + "\n\n".join(blocks)).strip(), items, matched_chars


def clio_json(command, work, timeout):
    """(parsed stdout or None, error or None, milliseconds)."""
    began = time.monotonic()
    try:
        done = subprocess.run(command, capture_output=True, text=True, timeout=timeout, env=child_env(work), cwd=work)
    except subprocess.TimeoutExpired:
        return None, f"timeout after {timeout}s", (time.monotonic() - began) * 1000
    elapsed = (time.monotonic() - began) * 1000
    if done.returncode != 0:
        return None, f"exit {done.returncode}: {done.stderr.strip()[-300:]}", elapsed
    try:
        return json.loads(done.stdout), None, elapsed
    except json.JSONDecodeError as exc:
        return None, f"bad json: {exc}", elapsed


def run_hook(arm, prompt, work):
    base = [arm["clio"], "--db-path", os.path.join(work, "memory.db"), "--local", "--json", "resume",
            "--namespace", prompt["namespace"], "--session", "bench-" + prompt["id"]]
    # Session start: 5s inside the hook's budget. A failed start shows nothing,
    # so nothing is marked seen — exactly what the hook does.
    start, start_error, _ = clio_json(base + ["--max-items", str(arm.get("start_max_items", 12))], work, 5)
    start_text, start_items, _ = render(start or {}, set(), start=True)
    seen = {item["id"] for item in start_items}

    query = prepare_query(arm, prompt["prompt"])
    brief, error, elapsed = clio_json(base + ["--query", query] + arm.get("extra_args", []), work, 8)
    text, items, matched_chars = render(brief or {}, seen)
    return {
        "ok": error is None and start_error is None,
        "error": error or (start_error and f"session start: {start_error}"),
        "ms": elapsed, "query_chars": len(query), "text": text, "items": items,
        "matched_chars": matched_chars, "other_chars": len(text) - matched_chars,
        "start_items": len(start_items), "start_chars": len(start_text),
    }


# --------------------------------------------------------------------------
# MCP arms: one server per prompt on its own copy, driven over stdio.
# --------------------------------------------------------------------------


class McpServer:
    def __init__(self, binary, work, timeout=60):
        self.proc = subprocess.Popen([binary], env=child_env(work), cwd=work, text=True,
                                     stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        self.next_id, self.timeout = 0, timeout
        self.rpc("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                "clientInfo": {"name": "resume-eval", "version": "0"}})
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}})

    def send(self, message):
        self.proc.stdin.write(json.dumps(message) + "\n")
        self.proc.stdin.flush()

    def rpc(self, method, params):
        self.next_id += 1
        watchdog = threading.Timer(self.timeout, self.proc.kill)  # a hung server must not hang the run
        watchdog.start()
        try:
            self.send({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params})
            while True:
                line = self.proc.stdout.readline()
                if not line:
                    raise RuntimeError(f"server closed the pipe during {method}")
                reply = json.loads(line)
                if reply.get("id") == self.next_id:
                    return reply
        finally:
            watchdog.cancel()

    def close(self):
        self.proc.kill()
        self.proc.wait()


def run_mcp(arm, prompt, work):
    query = prepare_query(arm, prompt["prompt"])
    # `_clio_namespace` is what the local bridge sends after resolving the
    # model's `cwd`, so this takes the same scoped-then-global path as production.
    template = arm.get("args") or {"query": "{query}", "_clio_namespace": "{namespace}"}
    fill = {"{query}": query, "{namespace}": prompt["namespace"]}
    arguments = {key: fill.get(value, value) if isinstance(value, str) else value for key, value in template.items()}
    result = {"ok": False, "error": None, "ms": 0.0, "query_chars": len(query), "text": "", "items": [],
              "matched_chars": 0, "other_chars": 0, "start_items": 0, "start_chars": 0}
    server = None
    try:
        server = McpServer(arm["mcp"], work)
        began = time.monotonic()
        reply = server.rpc("tools/call", {"name": arm["tool"], "arguments": arguments})
        result["ms"] = (time.monotonic() - began) * 1000  # the call only; server start-up is not counted
    except Exception as exc:  # noqa: BLE001 - any failure is a scored failure, never a dropped sample
        result["error"] = str(exc)
        return result
    finally:
        if server:
            server.close()
    payload = reply.get("result") or {}
    text = "".join(part.get("text", "") for part in payload.get("content", []))
    if reply.get("error") or payload.get("isError"):
        result["error"] = (json.dumps(reply.get("error")) if reply.get("error") else text)[:300]
        result["text"] = text  # an error message is still characters the model has to read
        result["matched_chars"] = len(text)
        return result
    # Anything shaped like an id, kept only if it is a memory: previews quote
    # session and worktree ids too.
    found = list(dict.fromkeys(UUID.findall(text)))
    conn = sqlite3.connect(os.path.join(work, "memory.db"))
    real = {row[0] for row in conn.execute(
        f"select id from memories where id in ({','.join('?' * len(found))})", found)}
    conn.close()
    ids = [memory_id for memory_id in found if memory_id in real]
    if not ids and len(text) > 400:
        result["error"] = "reply has content but no memory ids could be read from it"
    else:
        result["ok"] = True
    result.update(text=text, matched_chars=len(text),
                  items=[{"id": mid, "section": arm["tool"], "matched": True} for mid in ids])
    return result


# --------------------------------------------------------------------------
# Scoring
# --------------------------------------------------------------------------


def dcg(gains):
    return sum(gain / math.log2(position + 2) for position, gain in enumerate(gains))


def cosine(a, b):
    dot = sum(x * y for x, y in zip(a, b))
    norm = math.sqrt(sum(x * x for x in a)) * math.sqrt(sum(y * y for y in b))
    return dot / norm if norm else 0.0


class Corpus:
    """Read-only look-ups on the snapshot: titles for labelling, vectors for duplicates."""

    def __init__(self, snap):
        self.conn = sqlite3.connect(f"file:{os.path.join(snap, 'memory.db')}?immutable=1", uri=True)
        self.vectors, self.meta = {}, {}

    def vector(self, memory_id):
        if memory_id not in self.vectors:
            row = self.conn.execute("select embedding from memory_embeddings where memory_id = ?", (memory_id,)).fetchone()
            self.vectors[memory_id] = array.array("f", row[0]) if row else None
        return self.vectors[memory_id]

    def describe(self, memory_id):
        if memory_id not in self.meta:
            row = self.conn.execute(
                "select kind, namespace, title, coalesce(nullif(summary, ''), substr(content, 1, 400)), created_at "
                "from memories where id = ?", (memory_id,)).fetchone()
            self.meta[memory_id] = dict(zip(("kind", "namespace", "title", "summary", "created_at"), row)) if row else {}
        return self.meta[memory_id]

    def duplicate_slots(self, ids, threshold):
        """Slots spent on a memory near-identical to one earlier in the same brief."""
        duplicates = 0
        for index, memory_id in enumerate(ids):
            vector = self.vector(memory_id)
            title = " ".join((self.describe(memory_id).get("title") or "").lower().split())
            for earlier in ids[:index]:
                other = self.vector(earlier)
                other_title = " ".join((self.describe(earlier).get("title") or "").lower().split())
                if (title and title == other_title) or (vector and other and cosine(vector, other) >= threshold):
                    duplicates += 1
                    break
        return duplicates


def score_row(row, prompt, labels, ideal, corpus, dup_cosine):
    label = lambda item: labels.get((prompt["id"], item["id"]), "unjudged")  # noqa: E731
    matched = [item for item in row["items"] if item["matched"]]
    top = [label(item) for item in matched[:SLOTS]]
    ideal_dcg = dcg(ideal.get(prompt["id"], []))
    return {
        "matched": len(matched),
        "rel7": top.count("relevant"),
        "part7": top.count("partly"),
        # Unjudged slots score nothing here, so nDCG is a floor until they are labelled.
        "ndcg": dcg([GAIN.get(l, 0) for l in top]) / ideal_dcg if ideal_dcg else None,
        "all": collections.Counter(label(item) for item in row["items"]),
        "top": collections.Counter(label(item) for item in matched),
        "dups": corpus.duplicate_slots([item["id"] for item in row["items"]], dup_cosine),
    }


def bootstrap_interval(differences, draws=2000):
    """95% interval for a mean paired difference, resampling the prompts. Runs
    on one snapshot barely vary, so this — how far the mean moves with a
    different draw of prompts — is the noise that decides a comparison."""
    rng = random.Random(20261115)
    means = sorted(statistics.fmean(rng.choices(differences, k=len(differences))) for _ in range(draws))
    return means[int(0.025 * draws)], means[int(0.975 * draws) - 1]


def percentile(values, fraction):
    ordered = sorted(values)
    return ordered[max(0, math.ceil(fraction * len(ordered)) - 1)] if ordered else 0.0


def summarise(rows, scores, prompts):
    """One arm, one run -> the headline numbers."""
    by_id = {p["id"]: p for p in prompts}
    answerable = [pid for pid in rows if not by_id[pid].get("expect_nothing")]
    no_answer = [pid for pid in rows if by_id[pid].get("expect_nothing")]
    total = sum((scores[pid]["all"] for pid in rows), collections.Counter())
    top = sum((scores[pid]["top"] for pid in rows), collections.Counter())
    injected, matched = sum(total.values()) or 1, sum(top.values()) or 1
    ndcgs = [scores[pid]["ndcg"] for pid in rows if scores[pid]["ndcg"] is not None]
    mean = lambda values: statistics.fmean(values) if values else 0.0  # noqa: E731
    return {
        "items": mean([len(rows[pid]["items"]) for pid in rows]),
        "rel@7": mean([scores[pid]["rel7"] for pid in answerable]),
        "partly@7": mean([scores[pid]["part7"] for pid in answerable]),
        "nDCG@7": mean(ndcgs),
        "junk%": 100 * total["irrelevant"] / injected,
        "stale%": 100 * total["stale"] / injected,
        "unjudged%": 100 * total["unjudged"] / injected,
        "junk%m": 100 * top["irrelevant"] / matched,
        "dup/brief": mean([scores[pid]["dups"] for pid in rows]),
        "chars m": mean([rows[pid]["matched_chars"] for pid in rows]),
        "chars other": mean([rows[pid]["other_chars"] for pid in rows]),
        "chars start": mean([rows[pid]["start_chars"] for pid in rows]),
        "abstain": sum(scores[pid]["matched"] == 0 for pid in no_answer),
        "silent": sum(scores[pid]["matched"] == 0 for pid in answerable),
        "p50 ms": statistics.median(rows[pid]["ms"] for pid in rows) if rows else 0.0,
        "p95 ms": percentile([rows[pid]["ms"] for pid in rows], 0.95),
        "failed": sum(not rows[pid]["ok"] for pid in rows),
        "_unjudged": total["unjudged"], "_no_answer": len(no_answer), "_answerable": len(answerable),
    }


QUALITY = ("items", "rel@7", "partly@7", "nDCG@7", "junk%", "junk%m", "stale%", "unjudged%", "dup/brief")
COST = ("chars m", "chars other", "chars start", "abstain", "silent", "p50 ms", "p95 ms", "failed")


def table(header, rows, out):
    widths = [max(len(str(row[i])) for row in [header] + rows) for i in range(len(header))]
    for row in [header] + rows:
        out.append("  ".join(str(cell).ljust(width) if i == 0 else str(cell).rjust(width)
                             for i, (cell, width) in enumerate(zip(row, widths))).rstrip())


def fmt(key, value):
    if key in ("abstain", "silent", "failed"):
        return str(int(value))
    if key in ("rel@7", "partly@7", "dup/brief"):
        return f"{value:.2f}"
    if key == "items":
        return f"{value:.1f}"
    if key == "nDCG@7":
        return f"{value:.3f}"
    if key.endswith("%") or key.endswith("%m"):
        return f"{value:.1f}"
    return f"{value:.0f}"


def results_dir(private, wanted):
    if wanted:
        return os.path.realpath(wanted)
    runs = sorted(glob.glob(os.path.join(private, "results", "*", "meta.json")), key=os.path.getmtime)
    if not runs:
        die("no results yet; use `run` first")
    return os.path.dirname(runs[-1])


def load_results(directory):
    """{arm: [ {prompt_id: row}, ... one per run ]} in arm order."""
    with open(os.path.join(directory, "meta.json")) as handle:
        meta = json.load(handle)
    results = {}
    for arm in meta["arms"]:
        results[arm["name"]] = [
            {row["prompt_id"]: row for row in load_jsonl(os.path.join(directory, f"{arm['name']}.run{run}.jsonl"))}
            for run in range(1, meta["runs"] + 1)
        ]
    return meta, results


def report(private, directory, dup_cosine):
    prompts = load_prompts(private)
    labels = load_labels(private)
    corpus = Corpus(snapshot_dir(private))
    meta, results = load_results(directory)
    by_id = {p["id"]: p for p in prompts}

    by_prompt = collections.defaultdict(list)
    for (prompt_id, _), label in labels.items():
        by_prompt[prompt_id].append(GAIN[label])
    ideal = {pid: sorted(gains, reverse=True)[:SLOTS] for pid, gains in by_prompt.items()}

    out = [f"results: {directory}",
           f"snapshot sha256 {meta['snapshot_sha256'][:16]}  run at {meta['started']}  "
           f"prompts {len(prompts)}  judgements {len(labels)}  runs {meta['runs']}", ""]

    scored, summaries = {}, {}
    for arm, runs in results.items():
        scored[arm] = [{pid: score_row(row, by_id[pid], labels, ideal, corpus, dup_cosine)
                        for pid, row in run.items() if pid in by_id} for run in runs]
        summaries[arm] = [summarise({pid: row for pid, row in run.items() if pid in by_id}, scores, prompts)
                          for run, scores in zip(runs, scored[arm])]

    for arm, runs in results.items():
        first, scores = runs[0], scored[arm][0]
        out.append(f"== {arm}: per prompt (run 1; * = injected items differed in a later run)")
        rows = []
        for prompt in prompts:
            row, score = first.get(prompt["id"]), scores.get(prompt["id"])
            if row is None:
                continue
            ids = [item["id"] for item in row["items"]]
            changed = any([i["id"] for i in run[prompt["id"]]["items"]] != ids for run in runs[1:])
            counts = score["all"]
            rows.append([
                prompt["id"] + ("*" if changed else ""), prompt.get("category", ""),
                prompt["namespace"].replace("project:", "")[:16], score["matched"], score["rel7"], score["part7"],
                "-" if score["ndcg"] is None else f"{score['ndcg']:.2f}",
                counts["irrelevant"], counts["stale"], counts["unjudged"], score["dups"],
                row["matched_chars"], row["other_chars"], f"{row['ms']:.0f}",
                "" if row["ok"] else "FAILED " + (row["error"] or "")[:60],
            ])
        table(["prompt", "category", "namespace", "n", "rel7", "part7", "nDCG", "irr", "stale", "unj", "dup",
               "chars m", "other", "ms", ""], rows, out)
        out.append("")

    out.append("== summary (run 1)")
    out.append("items: mean memories injected per prompt, all sections. The percentages are shares of those.")
    out.append("rel@7, partly@7: mean count in the first 7 task-matched slots, answerable prompts only.")
    out.append("junk%, stale%, unjudged%: share of every injected item; junk%m: of task-matched items only.")
    table(["arm"] + list(QUALITY), [[arm] + [fmt(k, runs[0][k]) for k in QUALITY] for arm, runs in summaries.items()], out)
    out.append("")
    some = next(iter(summaries.values()))[0]
    out.append("chars: mean per prompt (m = task-matched, other = task-independent, start = session-start resume).")
    out.append(f"abstain: of {some['_no_answer']} no-answer prompts, how many got no task-matched item (higher is better).")
    out.append(f"silent: of {some['_answerable']} answerable prompts, how many got none (lower is better). ms: the timed call.")
    table(["arm"] + list(COST), [[arm] + [fmt(k, runs[0][k]) for k in COST] for arm, runs in summaries.items()], out)
    out.append("")

    if meta["runs"] > 1:
        out.append(f"== run-to-run spread (max - min over {meta['runs']} runs of the same arm on the same snapshot)")
        out.append("A difference between two arms smaller than this is noise. Quality barely varies between runs on")
        out.append("one snapshot, so for quality the interval in the paired table below is the test that matters.")
        keys = list(QUALITY) + ["chars m", "chars other", "p50 ms", "p95 ms"]
        rows = []
        for arm, runs in summaries.items():
            differing = sum(
                len({json.dumps([i["id"] for i in run[pid]["items"]]) for run in results[arm] if pid in run}) > 1
                for pid in results[arm][0])
            rows.append([arm] + [fmt(k, max(r[k] for r in runs) - min(r[k] for r in runs)) for k in keys] + [differing])
        table(["arm"] + keys + ["prompts differing"], rows, out)
        out.append("")

    ran = next(iter(scored.values()))[0]
    categories = collections.Counter(p.get("category", "") for p in prompts
                                     if p["id"] in ran and dcg(ideal.get(p["id"], [])))
    if len(categories) > 1:
        out.append("== nDCG@7 by prompt category (run 1; prompts with something relevant or partly relevant known)")
        rows = []
        for category, count in sorted(categories.items()):
            cells = [f"{category} ({count})"]
            for arm in results:
                values = [scored[arm][0][p["id"]]["ndcg"] for p in prompts
                          if p.get("category", "") == category and (scored[arm][0].get(p["id"]) or {}).get("ndcg") is not None]
                cells.append(f"{statistics.fmean(values):.3f}" if values else "-")
            rows.append(cells)
        table(["category"] + list(results), rows, out)
        out.append("")

    reference = next(iter(results))
    if len(results) > 1:
        out.append(f"== against {reference}, prompt by prompt (run 1)")
        out.append("better / same / worse counts prompts; diff is the mean difference with a 95% interval from")
        out.append("resampling the prompts. An interval that includes 0 means the change has not been shown.")
        rows = []
        for arm in list(results)[1:]:
            cells = [arm]
            for key in ("ndcg", "rel7"):
                pairs = [(scored[arm][0][pid][key], scored[reference][0][pid][key]) for pid in scored[reference][0]
                         if pid in scored[arm][0] and scored[reference][0][pid][key] is not None
                         and (key == "ndcg" or not by_id[pid].get("expect_nothing"))]
                cells.append(f"{sum(a > b for a, b in pairs)} / {sum(a == b for a, b in pairs)} / {sum(a < b for a, b in pairs)}")
                if pairs:
                    low, high = bootstrap_interval([a - b for a, b in pairs])
                    cells.append(f"{statistics.fmean(a - b for a, b in pairs):+.3f} [{low:+.3f}, {high:+.3f}]")
                else:
                    cells.append("-")
            rows.append(cells)
        table(["arm", "nDCG@7", "diff", "rel@7", "diff"], rows, out)
        out.append("")

    unjudged = sum(runs[0]["_unjudged"] for runs in summaries.values())
    if unjudged:
        out.append(f"UNJUDGED: {unjudged} injected slots have no label, so the quality figures above are floors, "
                   "not results. Run `unjudged`, label them, then `report` again.")
    for prompt in prompts:
        if prompt.get("expect_nothing") and any(g > 0 for g in by_prompt.get(prompt["id"], [])):
            out.append(f"CHECK: {prompt['id']} is marked expect_nothing but has relevant or partly labels.")
    failed = {arm: runs[0]["failed"] for arm, runs in summaries.items() if runs[0]["failed"]}
    if failed:
        out.append(f"FAILED CALLS (scored as injecting nothing, never dropped): {failed}")

    text = "\n".join(out)
    print(text)
    with open(os.path.join(directory, "report.txt"), "w") as handle:
        handle.write(text + "\n")


# --------------------------------------------------------------------------
# Commands
# --------------------------------------------------------------------------


def cmd_run(args):
    private = private_dir(args.private)
    snap = snapshot_dir(private)
    prompts = load_prompts(private)
    arms = load_arms(args.arms)
    if args.only:
        arms = [arm for arm in arms if arm["name"] in args.only.split(",")]
    if args.limit:
        prompts = prompts[: args.limit]

    # A `.dump` restore renumbers rows and silently misaligns the full-text
    # index. The check needs a writable handle, so it runs on a copy.
    probe = work_copy(private, snap)
    try:
        conn = sqlite3.connect(os.path.join(probe, "memory.db"))
        conn.execute("insert into memory_fts(memory_fts, rank) values('integrity-check', 1)")
        conn.close()
    except sqlite3.DatabaseError as exc:
        die(f"snapshot's full-text index does not match its rows ({exc}); take a new snapshot with a sqlite backup")
    finally:
        shutil.rmtree(probe, ignore_errors=True)

    directory = os.path.join(private, "results", args.name or time.strftime("%Y%m%d-%H%M%S"))
    os.makedirs(directory, exist_ok=True)
    meta = {
        "started": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "runs": args.runs, "arms": arms,
        "snapshot_sha256": sha256(os.path.join(snap, "memory.db")),
        "binaries": {b: sha256(b) for b in sorted({arm.get("clio") or arm.get("mcp") for arm in arms})},
        "prompts": len(prompts),
    }
    with open(os.path.join(directory, "meta.json"), "w") as handle:
        json.dump(meta, handle, indent=1)

    for run in range(1, args.runs + 1):
        for arm in arms:
            path = os.path.join(directory, f"{arm['name']}.run{run}.jsonl")
            with open(path, "w") as handle:
                for index, prompt in enumerate(prompts, 1):
                    work = work_copy(private, snap, arm.get("settings"))
                    try:
                        hidden = hide_own_session(work, prompt["session"]) if prompt.get("session") else 0
                        row = (run_hook if arm["kind"] == "hook" else run_mcp)(arm, prompt, work)
                    finally:
                        shutil.rmtree(work, ignore_errors=True)
                    row.update(prompt_id=prompt["id"], hidden_own_session=hidden)
                    handle.write(json.dumps(row) + "\n")
                    print(f"\rrun {run}/{args.runs}  {arm['name']}  {index}/{len(prompts)}", end="", file=sys.stderr)
            print(file=sys.stderr)
    report(private, directory, args.dup_cosine)


def cmd_report(args):
    private = private_dir(args.private)
    report(private, results_dir(private, args.results), args.dup_cosine)


def cmd_unjudged(args):
    private = private_dir(args.private)
    prompts = {p["id"]: p for p in load_prompts(private)}
    labels = load_labels(private)
    corpus = Corpus(snapshot_dir(private))
    _, results = load_results(results_dir(private, args.results))
    pending = collections.OrderedDict()
    for arm, runs in results.items():
        for run in runs:
            for prompt_id, row in run.items():
                for item in row["items"]:
                    key = (prompt_id, item["id"])
                    if key not in labels and prompt_id in prompts:
                        pending.setdefault(key, set()).add(arm)
    if not pending:
        print("nothing unjudged: every injected memory in these results has a label")
        return
    path = os.path.join(private, "unjudged.jsonl")
    with open(path, "w") as handle:
        for (prompt_id, memory_id), arms in sorted(pending.items()):
            handle.write(json.dumps({
                "prompt_id": prompt_id, "memory_id": memory_id, "label": "",
                "prompt": " ".join(prompts[prompt_id]["prompt"].split())[:400],
                **corpus.describe(memory_id), "arms": sorted(arms),
            }, ensure_ascii=False) + "\n")
    print(f"{len(pending)} unjudged (prompt, memory) pairs written to {path}")
    print(f"Fill in each \"label\" ({', '.join(GAIN)}), then append the file to labels.jsonl and run `report`.")


def cmd_prompts(args):
    """Candidate prompts: the first substantive user prompt of each recent
    Claude Code session, with the namespace its working directory resolves to."""
    private = private_dir(args.private)
    cutoff = time.time() - args.days * 86400
    namespaces, candidates = {}, []
    temp_roots = ("/private/tmp", "/tmp", "/private/var/folders", "/var/folders")  # the hooks skip these
    for path in sorted(glob.glob(os.path.expanduser("~/.claude/projects/*/*.jsonl")), key=os.path.getmtime):
        if os.path.getmtime(path) < cutoff:
            continue
        with open(path, errors="replace") as handle:
            for line in handle:
                try:
                    entry = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if entry.get("type") != "user" or entry.get("isSidechain") or entry.get("isMeta"):
                    continue
                content = (entry.get("message") or {}).get("content")
                if isinstance(content, list):
                    if any(part.get("type") == "tool_result" for part in content if isinstance(part, dict)):
                        continue
                    content = "\n".join(p.get("text", "") for p in content if isinstance(p, dict) and p.get("type") == "text")
                text = (content or "").strip()
                # prompt_recall.py is_substantive(): the hook waits for the first prompt that passes.
                if len(text) < 12 or text.startswith(("/", "!", "<")):
                    continue
                cwd = entry.get("cwd") or ""
                if not os.path.isdir(cwd) or os.path.realpath(cwd).startswith(temp_roots):
                    break
                if cwd not in namespaces:
                    done = subprocess.run(  # `context` only reads cwd; the path keeps it off the live store regardless
                        [args.clio, "--db-path", os.path.join(private, "work", "unused.db"), "--local", "--json", "context"],
                        cwd=cwd, capture_output=True, text=True, env={"PATH": os.environ.get("PATH", "")})
                    namespaces[cwd] = json.loads(done.stdout)["namespace"] if done.returncode == 0 else None
                if namespaces[cwd]:
                    candidates.append({
                        "session": entry.get("sessionId"), "date": (entry.get("timestamp") or "")[:10], "cwd": cwd,
                        "branch": entry.get("gitBranch"), "namespace": namespaces[cwd], "chars": len(text),
                        "sensitive": bool(SECRETISH.search(text)), "prompt": text,
                    })
                break
    out = os.path.join(private, "candidates.jsonl")
    with open(out, "w") as handle:
        for candidate in candidates:
            handle.write(json.dumps(candidate, ensure_ascii=False) + "\n")
    print(f"{len(candidates)} candidates written to {out}; "
          f"{sum(c['sensitive'] for c in candidates)} flagged sensitive (leave those out)")


def cmd_selftest(_args):
    assert abs(dcg([2, 1]) - (2 + 1 / math.log2(3))) < 1e-9
    assert abs(cosine([1, 0], [1, 0]) - 1) < 1e-9 and cosine([1, 0], [0, 1]) == 0
    low, high = bootstrap_interval([0.2] * 30)
    assert abs(low - 0.2) < 1e-9 and abs(high - 0.2) < 1e-9  # no spread in, no spread out
    low, high = bootstrap_interval([1.0, -1.0] * 30)
    assert low < 0 < high  # a coin-flip difference must not look like a win

    brief = {"sections": [
        {"heading": "Active constraints", "items": [{"memory_id": "a", "kind": "constraint", "title": "A", "reason": "r", "content": "c"}]},
        {"heading": MATCHED_HEADING, "items": [{"memory_id": "b", "kind": "fact", "title": "B", "reason": "r", "content": "c"},
                                               {"memory_id": "c", "kind": "fact", "title": None, "reason": "", "content": ""}]},
    ]}
    text, items, matched = render(brief, {"a"})
    assert [i["id"] for i in items] == ["b", "c"] and all(i["matched"] for i in items), items
    assert text == "# Clio — relevant to this task\n## Relevant knowledge\n- [fact] **B** (b)\n  why: r\n  c\n- [fact] **(untitled)** (c)", text
    assert matched == len(text) - len("# Clio — relevant to this task\n")
    assert render(brief, {"a", "b", "c"}) == ("", [], 0)  # everything already seen: the hook abstains
    start_text, start_items, _ = render(brief, set(), start=True)
    assert len(start_items) == 3 and start_text.startswith("## Resume\n\n### Active constraints\n- [constraint] **A**\n")

    class Fake(Corpus):
        def __init__(self):
            self.vectors = {"x": [1.0, 0.0], "y": [0.99, 0.05], "z": [0.0, 1.0], "w": None}
            self.meta = {"x": {"title": "One"}, "y": {"title": "Two"}, "z": {"title": "Three"}, "w": {"title": " one "}}

        def vector(self, memory_id):
            return self.vectors.get(memory_id)

        def describe(self, memory_id):
            return self.meta.get(memory_id, {})

    assert Fake().duplicate_slots(["x", "y", "z", "w"], 0.9) == 2  # y by vector, w by title

    prompt = {"id": "p1"}
    labels = {("p1", "b"): "relevant", ("p1", "a"): "irrelevant", ("p1", "s"): "stale"}
    row = {"items": [{"id": "a", "matched": False}, {"id": "u", "matched": True}, {"id": "b", "matched": True}]}
    score = score_row(row, prompt, labels, {"p1": [2, 0, 0]}, Fake(), 0.9)
    assert score["rel7"] == 1 and score["all"]["unjudged"] == 1 and score["all"]["irrelevant"] == 1, score
    assert abs(score["ndcg"] - (2 / math.log2(3)) / 2) < 1e-9  # the unjudged slot scores nothing, but is not junk
    assert score_row(row, prompt, labels, {}, Fake(), 0.9)["ndcg"] is None  # nothing relevant known: undefined

    with tempfile.TemporaryDirectory() as scratch:
        scratch = os.path.realpath(scratch)
        inside = os.path.join(scratch, "repo", "data")
        os.makedirs(os.path.join(scratch, "repo", ".git"))
        os.makedirs(inside)
        assert git_repo_above(scratch) is None
        with open(os.path.join(scratch, "labels.jsonl"), "w") as handle:
            handle.write('{"prompt_id": "p", "memory_id": "m", "label": ""}\n')
        for call, refusal in ((lambda: private_dir(inside), "inside the git repository"),
                              (lambda: load_labels(scratch), "is not one of")):  # a blank label is never a default
            try:
                call()
            except SystemExit as stop:
                assert refusal in str(stop), stop
            else:
                raise AssertionError(f"expected a refusal: {refusal}")
    print("selftest ok")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    commands = parser.add_subparsers(dest="command", required=True)

    def command(name, handler, private=True):
        sub = commands.add_parser(name)
        sub.set_defaults(handler=handler)
        if private:
            sub.add_argument("--private", required=True, help="directory of private data, outside any git repository")
        return sub

    run = command("run", cmd_run)
    run.add_argument("--arms", required=True, help="JSON list of arms (see README)")
    run.add_argument("--runs", type=int, default=2, help="repeats of every arm, for the noise figure (default 2)")
    run.add_argument("--only", help="comma-separated arm names")
    run.add_argument("--limit", type=int, help="first N prompts only (smoke test)")
    run.add_argument("--name", help="results folder name (default: timestamp)")
    for sub in (run, command("report", cmd_report)):
        sub.add_argument("--dup-cosine", type=float, default=0.85,
                         help="embedding cosine at or above which two memories in one brief count as duplicates")
    commands.choices["report"].add_argument("--results", help="results folder (default: the latest)")
    command("unjudged", cmd_unjudged).add_argument("--results", help="results folder (default: the latest)")
    sample = command("prompts", cmd_prompts)
    sample.add_argument("--clio", required=True, help="clio binary, used only to resolve namespaces")
    sample.add_argument("--days", type=int, default=30)
    command("selftest", cmd_selftest, private=False)

    args = parser.parse_args()
    args.handler(args)


if __name__ == "__main__":
    main()

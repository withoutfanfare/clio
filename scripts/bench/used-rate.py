#!/usr/bin/env python3
"""Did the model actually use what Clio injected? Scored on real sessions.

resume-eval.py asks whether an injected memory was relevant to a prompt. This
asks the question that matters more: across a whole real session, did the work
draw on it? For a sample of recent Claude Code sessions it reads every memory
the hooks injected (session start and per prompt) and a condensed copy of what
happened afterwards, and a judge model labels each memory:

    used        the work drew on it: followed it, acted on it, or it changed
                what was said or done (the judge must quote the evidence)
    on-task     about the specific task the session worked on, but not needed
    background  about the same project or setup, but not about this task
    irrelevant  about neither

    used-rate.py run    --private DIR [--days 14] [--sample 40] [--sample-seed N] [--jobs 6]
    used-rate.py report --private DIR
    used-rate.py selftest

Judged sessions are cached in DIR/judged/<session>.json and never re-judged,
so each `run` adds a fresh sample and `report` shows the trend week by week.
DIR holds transcript excerpts and must not be inside a git repository.
"""

import argparse
import collections
import concurrent.futures
import datetime
import glob
import importlib.util
import json
import os
import random
import re
import statistics
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import judge  # noqa: E402

_spec = importlib.util.spec_from_file_location("resume_eval", os.path.join(HERE, "resume-eval.py"))
resume_eval = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(resume_eval)
private_dir, die = resume_eval.private_dir, resume_eval.die

LABELS = ("used", "on-task", "background", "irrelevant")
BUDGET = 60_000  # characters of condensed transcript per judge call
ITEM = re.compile(r"^- \[([^\]·]+?)(?: · [^\]]*)?\] \*\*(.+?)\*\*(?: \(([0-9a-f-]{36})\))?\s*$")
HEADING = re.compile(r"^#{2,3} (.+?)\s*$")
TEMP_ROOTS = ("/private/tmp", "/tmp", "/private/var/folders", "/var/folders")

# --------------------------------------------------------------------------
# Reading a transcript
# --------------------------------------------------------------------------


def hook_text(attachment):
    """The context a hook injected: plain stdout, or SessionStart's JSON envelope."""
    text = attachment.get("stdout") or attachment.get("content") or ""
    try:
        return json.loads(text)["hookSpecificOutput"]["additionalContext"]
    except (ValueError, KeyError, TypeError):
        return text


def parse_items(text):
    """Clio memory cards in injected text: [{key, id, kind, title, section, body}]."""
    items, section, current = [], "", None
    for line in text.splitlines():
        heading = HEADING.match(line)
        if heading:
            section, current = heading.group(1), None
            continue
        match = ITEM.match(line)
        if match:
            kind, title, memory_id = match.groups()
            # Session start prints no ids, so a title stands in for one there.
            current = {"key": memory_id or "title:" + title, "id": memory_id, "kind": kind.strip(),
                       "title": title, "section": section, "body": []}
            items.append(current)
        elif current is not None and line.startswith("  "):
            current["body"].append(line.strip())
        else:
            current = None
    for item in items:
        item["body"] = " ".join(item["body"])[:600]
    return items


def message_text(message):
    content = message.get("content")
    if isinstance(content, str):
        return [content]
    parts = []
    for part in content or []:
        if not isinstance(part, dict):
            continue
        if part.get("type") == "text":
            parts.append(part.get("text", ""))
        elif part.get("type") == "tool_use":
            parts.append(f"[tool {part.get('name')}: {json.dumps(part.get('input'), ensure_ascii=False)[:400]}]")
    return parts


def read_session(path):
    """(events, memories, meta). events: ("user"|"assistant"|"clio", text or [keys])."""
    events, memories, meta = [], {}, {"chars": 0, "injections": 0}
    with open(path, errors="replace") as handle:
        for line in handle:
            try:
                entry = json.loads(line)
            except json.JSONDecodeError:
                continue
            if entry.get("isSidechain"):
                continue
            if entry.get("cwd"):
                meta.setdefault("cwd", entry["cwd"])
            if entry.get("timestamp"):
                meta.setdefault("date", entry["timestamp"][:10])
            attachment = entry.get("attachment") or {}
            if attachment.get("type") == "hook_success":
                text = hook_text(attachment)
                items = parse_items(text) if "Clio" in text or "## Resume" in text else []
                if items:
                    source = "start" if attachment.get("hookEvent") == "SessionStart" else "prompt"
                    meta["chars"] += len(text)
                    meta["injections"] += 1
                    fresh = []
                    for item in items:
                        if item["key"] not in memories:
                            memories[item["key"]] = dict(item, source=source, turn=meta["injections"])
                            fresh.append(item["key"])
                    events.append(("clio", fresh))
                continue
            kind = entry.get("type")
            if kind not in ("user", "assistant"):
                continue
            message = entry.get("message") or {}
            content = message.get("content")
            if kind == "user" and isinstance(content, list) and any(
                    isinstance(p, dict) and p.get("type") == "tool_result" for p in content):
                continue  # tool output: what the model read, not what it did
            text = "\n".join(t for t in message_text(message) if t.strip()).strip()
            if text:
                events.append((kind, text))
    return events, memories, meta


def condense(events, numbering, budget=BUDGET):
    """The session as one string under `budget`, with a marker where Clio injected."""
    talk = [e for e in events if e[0] != "clio"]
    cap = max(150, budget // max(1, len(talk)))
    out = []
    for kind, payload in events:
        if kind == "clio":
            labels = [numbering[key] for key in payload if key in numbering]
            if labels:
                out.append(f"[[Clio injected {', '.join(labels)} here]]")
            continue
        text = " ".join(payload.split())
        if len(text) > cap:
            text = text[: cap // 2] + " … " + text[-cap // 2:]
        out.append(f"{kind.upper()}: {text}")
    return "\n".join(out)[: budget + 2000]


PROMPT = """You are auditing a memory system that injects notes into an AI coding \
assistant's context. Below are the notes it injected into one real session \
(M1, M2, ...), then a condensed transcript of that session, with markers \
showing where each note was injected.

For every note, decide one label:
- "used": the session's work drew on this note after it was injected: the \
assistant followed it, acted on it, repeated a fact from it, or it changed what \
was said or done. You must quote or point to the specific evidence. Merely \
being on topic is not use.
- "on-task": about the specific task this session worked on, but nothing \
shows it was needed.
- "background": about the same project, codebase or general setup, but not \
about the specific task this session worked on. A standing project rule or \
fact that the task never touched is background.
- "irrelevant": about neither this task nor this project.

Be strict: when unsure between used and on-task, choose on-task; between \
on-task and background, choose background.

Reply with only a JSON object, one key per note:
{{"M1": {{"label": "used", "evidence": "<15 words or fewer>"}}, "M2": {{"label": "irrelevant", "evidence": ""}}, ...}}

NOTES
{notes}

TRANSCRIPT
{transcript}
"""


def judge_session(path, model=judge.MODEL):
    events, memories, meta = read_session(path)
    keys = list(memories)
    numbering = {key: f"M{index}" for index, key in enumerate(keys, 1)}
    notes = "\n".join(
        f"{numbering[k]} [{memories[k]['kind']}, {memories[k]['section'] or 'context'}] "
        f"{memories[k]['title']}: {memories[k]['body']}" for k in keys)
    verdicts, cost = judge.ask(PROMPT.format(notes=notes, transcript=condense(events, numbering)), model=model)
    for key in keys:
        verdict = verdicts.get(numbering[key]) or {}
        label = verdict.get("label") if isinstance(verdict, dict) else verdict
        memories[key]["label"] = label if label in LABELS else "unjudged"
        memories[key]["evidence"] = (verdict.get("evidence") if isinstance(verdict, dict) else "") or ""
    return {"session": os.path.basename(path)[:-6], "date": meta.get("date"), "cwd": meta.get("cwd"),
            "chars": meta["chars"], "injections": meta["injections"], "cost": cost, "model": model,
            "judged_at": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "memories": list(memories.values())}


# --------------------------------------------------------------------------
# Commands
# --------------------------------------------------------------------------


def candidates(days, done):
    cutoff = time.time() - days * 86400
    found = []
    for path in glob.glob(os.path.expanduser("~/.claude/projects/*/*.jsonl")):
        session = os.path.basename(path)[:-6]
        if os.path.getmtime(path) < cutoff or session in done:
            continue
        # Cheap pre-filter; read_session decides properly.
        with open(path, errors="replace") as handle:
            head = handle.read()
        if "hook_success" not in head or "**" not in head:
            continue
        cwd = re.search(r'"cwd":"([^"]*)"', head)
        if cwd and cwd.group(1).startswith(TEMP_ROOTS):
            continue
        found.append(path)
    return sorted(found)


def cmd_run(args):
    private = private_dir(args.private)
    store = os.path.join(private, "judged")
    os.makedirs(store, exist_ok=True)
    done = {name[:-5] for name in os.listdir(store)}
    paths = candidates(args.days, done)
    # Skip sessions too short to show any use: no assistant reply after an injection.
    usable = []
    for path in paths:
        events, memories, _ = read_session(path)
        if memories and any(kind == "assistant" for kind, _ in events):
            usable.append(path)
    rng = random.Random(args.sample_seed if args.sample_seed is not None else time.time())
    sample = rng.sample(usable, min(args.sample, len(usable)))
    print(f"{len(usable)} unjudged sessions with Clio context in the last {args.days} days; judging {len(sample)}",
          file=sys.stderr)
    failures = 0
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        futures = {pool.submit(judge_session, path, args.model): path for path in sample}
        for index, future in enumerate(concurrent.futures.as_completed(futures), 1):
            path = futures[future]
            try:
                result = future.result()
            except Exception as exc:  # one bad reply must not lose the rest
                failures += 1
                print(f"\nfailed {os.path.basename(path)}: {exc}", file=sys.stderr)
                continue
            with open(os.path.join(store, result["session"] + ".json"), "w") as handle:
                json.dump(result, handle, ensure_ascii=False)
            print(f"\rjudged {index}/{len(sample)}", end="", file=sys.stderr)
    print(f"\n{failures} failed" if failures else "", file=sys.stderr)
    report(private)


def share(counter):
    total = sum(counter[label] for label in LABELS) or 1
    return [f"{100 * counter[label] / total:.0f}%" for label in LABELS]


def report(private):
    sessions = []
    for path in sorted(glob.glob(os.path.join(private, "judged", "*.json"))):
        with open(path) as handle:
            sessions.append(json.load(handle))
    if not sessions:
        die("nothing judged yet; use `run` first")
    out = []
    table = resume_eval.table

    def counts(rows):
        return collections.Counter(m["label"] for s in rows for m in s["memories"])

    total = counts(sessions)
    used = total["used"]
    chars = sum(s["chars"] for s in sessions)
    out.append(f"{len(sessions)} sessions, {sum(len(s['memories']) for s in sessions)} injected memories, "
               f"judged by {', '.join(sorted({s.get('model', judge.MODEL) for s in sessions}))} "
               f"for ${sum(s.get('cost', 0) for s in sessions):.2f}")
    out.append("")
    out.append("== headline (share of injected memories)")
    rows = [["all"] + share(total) + [f"{statistics.median(len(s['memories']) for s in sessions):.0f}",
                                       f"{statistics.median(s['chars'] for s in sessions):.0f}",
                                       f"{chars / used:.0f}" if used else "-", total["unjudged"]]]
    table(["", *LABELS, "per session", "chars/session", "chars per use", "unjudged"],
          rows, out)
    out.append("used: the work drew on it. chars per use: injected characters for each memory used (lower is better).")
    out.append("")

    out.append("== by where it came from")
    groups = collections.defaultdict(collections.Counter)
    for s in sessions:
        for m in s["memories"]:
            groups[(m["source"], m["section"] or "-")][m["label"]] += 1
    table(["source", "section", "n", *LABELS],
          [[src, sec, sum(c[l] for l in LABELS)] + share(c)
           for (src, sec), c in sorted(groups.items(), key=lambda kv: -sum(kv[1].values()))], out)
    out.append("")

    out.append("== by week the session started: the trend to watch after a change")
    weeks = collections.defaultdict(list)
    for s in sessions:
        if s.get("date"):
            year, week, _ = datetime.date.fromisoformat(s["date"]).isocalendar()
            weeks[f"{year}-W{week:02d}"].append(s)
    table(["week", "sessions", *LABELS, "per session", "chars/session"],
          [[w, len(rows)] + share(counts(rows)) +
           [f"{statistics.median(len(s['memories']) for s in rows):.0f}",
            f"{statistics.median(s['chars'] for s in rows):.0f}"] for w, rows in sorted(weeks.items())], out)
    out.append("")

    seen = collections.defaultdict(lambda: [0, 0, ""])
    for s in sessions:
        for m in s["memories"]:
            entry = seen[m["title"]]  # session start has no ids, so the title joins both sources
            entry[0] += 1
            entry[1] += m["label"] == "used"
            entry[2] = m["title"]
    repeat = sorted(((n, u, t) for n, u, t in seen.values() if n >= 3 and u == 0), reverse=True)[:10]
    if repeat:
        out.append("== injected in 3+ sampled sessions and never used (candidates to archive or demote)")
        table(["sessions", "title"], [[n, t[:90]] for n, _, t in repeat], out)
        out.append("")
    out.append("Spot-check before acting: the judge saw a condensed transcript, and 'used' needs its evidence.")
    text = "\n".join(out)
    print(text)
    with open(os.path.join(private, "report.txt"), "w") as handle:
        handle.write(text + "\n")


def cmd_report(args):
    report(private_dir(args.private))


def cmd_selftest(_args):
    start = json.dumps({"hookSpecificOutput": {"additionalContext": (
        "# [clio/develop] recent context\n\n## Resume\n\n### Needs attention\n"
        "- [task · 2026-11-16] **Deploy the fixes**\n  why: you asked\n  Deploy at a convenient time.\n"
        "=== Project Context ===\nBranch: develop\n")}})
    items = parse_items(hook_text({"stdout": start}))
    assert [(i["key"], i["section"], i["kind"], i["body"]) for i in items] == [
        ("title:Deploy the fixes", "Needs attention", "task", "why: you asked Deploy at a convenient time.")], items
    uid = "01a1e4f9-fa4b-7a63-98c8-b2583544d176"
    prompt = f"# Clio — relevant to this task\n## Relevant knowledge\n- [fact · 2026-08-15] **A fact** ({uid})\n  why: matches\n"
    items = parse_items(hook_text({"stdout": prompt}))
    assert items[0]["key"] == uid and items[0]["section"] == "Relevant knowledge", items

    events = [("clio", ["k1"]), ("user", "x" * 1000), ("assistant", "short")]
    text = condense(events, {"k1": "M1"}, budget=300)
    assert text.startswith("[[Clio injected M1 here]]\nUSER: ") and " … " in text and "ASSISTANT: short" in text, text

    assert judge.parse_object('Sure:\n```json\n{"M1": {"label": "used"}}\n```') == {"M1": {"label": "used"}}
    print("selftest ok")


def main():
    os.umask(0o077)  # judged files hold transcript excerpts
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    commands = parser.add_subparsers(dest="command", required=True)
    run = commands.add_parser("run")
    run.set_defaults(handler=cmd_run)
    run.add_argument("--days", type=int, default=14)
    run.add_argument("--sample", type=int, default=40, help="sessions to judge this run (default 40)")
    run.add_argument("--sample-seed", type=int, help="fix the sample (default: random)")
    run.add_argument("--jobs", type=int, default=6, help="judge calls in parallel")
    run.add_argument("--model", default=judge.MODEL, help=f"judge model (default {judge.MODEL})")
    rep = commands.add_parser("report")
    rep.set_defaults(handler=cmd_report)
    for sub in (run, rep):
        sub.add_argument("--private", required=True, help="directory of private data, outside any git repository")
    commands.add_parser("selftest").set_defaults(handler=cmd_selftest)
    args = parser.parse_args()
    args.handler(args)


if __name__ == "__main__":
    main()

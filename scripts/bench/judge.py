"""One model judgement through Claude Code in headless safe mode.

Safe mode loads no hooks, MCP servers or CLAUDE.md, so the judge's own call
neither receives Clio context nor gets captured into Clio, and
--no-session-persistence keeps it out of ~/.claude/projects (which used-rate.py
reads). It uses the Claude Code sign-in, so no API key is needed.
"""

import json
import re
import subprocess

MODEL = "sonnet"  # Haiku agreed on "used" but called project background on-task (17 Nov 2026)


def ask(prompt, model=MODEL, timeout=300):
    """(parsed JSON object from the reply, cost in US dollars). Raises on failure."""
    done = subprocess.run(
        ["claude", "-p", "--safe-mode", "--model", model, "--no-session-persistence",
         "--tools", "", "--output-format", "json"],
        input=prompt, capture_output=True, text=True, timeout=timeout)
    if done.returncode != 0:
        raise RuntimeError(f"claude exit {done.returncode}: {(done.stderr or done.stdout).strip()[-300:]}")
    envelope = json.loads(done.stdout)
    if envelope.get("is_error"):
        raise RuntimeError(f"claude error: {str(envelope.get('result'))[:300]}")
    return parse_object(envelope.get("result") or ""), float(envelope.get("total_cost_usd") or 0.0)


def parse_object(text):
    """The first JSON object in a reply, fenced or not."""
    match = re.search(r"\{.*\}", text, re.S)
    if not match:
        raise ValueError(f"no JSON object in reply: {text[:200]!r}")
    return json.loads(match.group(0))

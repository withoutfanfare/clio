# Handoff Briefs and Receipts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Clio the context courier for the Cadence/Linear work queue: a `handoff` brief preset that packages everything an agent or human needs to pick up a ticket, and session `receipt` memories captured from both Claude Code and Codex so the next session knows what was done.

**Architecture:** Two small additions to `clio-core` (a `receipt` kind accepted by the distillation pipeline in `capture.rs`, and a `Handoff` preset in `assembly.rs`), surfaced through the existing thin adapters (MCP + CLI) with doc updates only. Outside this repo: a new Codex stop-hook script that reuses the existing Claude Code distillation pipeline, and two-line additions to the Cadence loop skills so specs/builds pull the handoff brief. No schema change — `kind` is free text (≤50 chars) and tags are already FTS-indexed (verified empirically: a memory tagged `ticket:cad-42` is found by an FTS query for "CAD-42").

**Tech Stack:** Rust (rusqlite, serde), Python 3 hook scripts, Codex `~/.codex/hooks.json` (Claude-style hooks).

## Global Constraints

- British English in all documentation, comments, and user-facing text.
- Conventional commits. Never merge/push to `main` (human-only; hook-enforced).
- All business logic lives in `clio-core`; CLI/MCP/daemon/Tauri are thin adapters.
- MCP defaults must match CLI/core semantics exactly.
- No schema change in this plan — therefore no migration. Do not add one.
- Archive semantics, tag/FTS sync triggers, and upsert keying must not be touched.
- Work on branch `feat/handoff-and-receipts` off `develop` in the clio repo.
- Verify with `cargo test -p clio-core`, `cargo clippy`, `cargo fmt`.
- Hook scripts live in `~/.claude/personal-skills/clio-hooks/scripts/` (reached via the `~/.claude/skills/clio-hooks` symlink); that directory is inside the `~/.claude` git repo — commit there too.
- Cadence skill files live in `/Users/dannyharding/Development/Code/Project/cadence/skills/` — commit in that repo.
- Every meaningful change requires a DOX pass (root `CLAUDE.md` rule): check whether the nearest `CLAUDE.md` files need updating; here the durable contracts land in `context/DOMAIN_RULES.md` and `docs/reference/mcp-contract.md` (Tasks 3), so the crate-level CLAUDE.md files need no edits.

---

### Task 0: Branch setup

**Files:** none (git only)

- [ ] **Step 1: Create the working branch**

```bash
cd /Users/dannyharding/Development/Code/Project/clio
git checkout develop && git pull && git checkout -b feat/handoff-and-receipts
```

Expected: on branch `feat/handoff-and-receipts`. Note: `cadence/tasks.md` may show as modified in the working tree — leave it alone; it is unrelated.

---

### Task 1: `receipt` kind in the distillation pipeline (core)

The stop hooks call `clio distill`, which uses the LLM prompt in `capture.rs` and validates kinds in `classification_from_value`. A receipt is a short record of one working session — what was done, what was left undone, why it stopped. It is the single deliberate exception to the prompt's "do not capture activity" rule.

**Files:**
- Modify: `crates/clio-core/src/capture.rs`
- Test: inline `#[cfg(test)]` mod in the same file (follow the existing tests there)

**Interfaces:**
- Consumes: nothing new.
- Produces: `parse_distillation` (existing signature `pub fn parse_distillation(raw: &str) -> Result<Vec<DistilledMemory>>`) now preserves `kind == "receipt"` instead of downgrading it to `"note"`. Later tasks rely on memories with `kind = "receipt"` existing in the store.

- [ ] **Step 1: Write the failing test**

Add to the tests module at the bottom of `crates/clio-core/src/capture.rs`, next to the existing `parse_distillation` tests:

```rust
#[test]
fn parse_distillation_accepts_receipt_kind() {
    let raw = r#"[{"content":"Implemented the handoff preset and its tests; did not touch the CLI; stopped once cargo test passed.","kind":"receipt","title":"Session receipt","summary":"Work record for the session","tags":["receipt"],"namespace":"project:clio","importance":2,"confidence":0.9}]"#;
    let memories = parse_distillation(raw).expect("parse failed");
    assert_eq!(memories.len(), 1);
    assert_eq!(memories[0].kind, "receipt");
    assert_eq!(memories[0].importance, 2);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p clio-core parse_distillation_accepts_receipt_kind`
Expected: FAIL — assertion `memories[0].kind == "receipt"` fails because the kind is downgraded to `"note"` (not in `valid_kinds`).

- [ ] **Step 3: Add `receipt` to the valid kinds**

In `classification_from_value` (around line 268), change:

```rust
    let valid_kinds = [
        "note",
        "fact",
        "decision",
        "summary",
        "task",
        "observation",
        "constraint",
    ];
```

to:

```rust
    let valid_kinds = [
        "note",
        "fact",
        "decision",
        "summary",
        "task",
        "observation",
        "constraint",
        "receipt",
    ];
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p clio-core parse_distillation_accepts_receipt_kind`
Expected: PASS. Also run `cargo test -p clio-core capture` — all existing capture tests still pass.

- [ ] **Step 5: Teach the distillation prompt about receipts**

Three edits inside `DISTILLATION_SYSTEM_PROMPT` in `capture.rs` (the distillation prompt only — leave `CLASSIFICATION_SYSTEM_PROMPT` alone; receipts come from session distillation, not ad-hoc capture):

Edit A — the "Do NOT capture" line about activity. Change:

```bash
- Routine activity, step-by-step narration, or "what was done" ("I edited file X, ran the tests")
```

to:

```bash
- Routine activity, step-by-step narration, or "what was done" ("I edited file X, ran the tests") — EXCEPT the single session receipt described below
```

Edit B — after the paragraph beginning `Each captured memory must be SELF-CONTAINED:` insert a new paragraph:

```bash
Additionally, if (and only if) the session performed substantive work — commits made, files changed, a bug diagnosed, a document produced — emit EXACTLY ONE extra memory with "kind": "receipt": a 2–4 sentence record of what was done, what was deliberately left undone, and why the session stopped where it did. Write it so someone picking the work up cold understands the state of play. Use "importance": 2 and include the tag "receipt". Sessions with no substantive work get no receipt.
```

Edit C — the distillation response-schema kind line. Change:

```text
- "kind": one of "note", "fact", "decision", "summary", "task", "observation", "constraint"
```

to:

```text
- "kind": one of "note", "fact", "decision", "summary", "task", "observation", "constraint", "receipt"
```

Also update the two doc comments on `ClassificationResult.kind` and `DistilledMemory.kind` (lines ~32 and ~56), which both read `/// Memory kind — one of note, fact, decision, summary, task, observation, constraint.` — append `, receipt` to each.

- [ ] **Step 6: Full check and commit**

```bash
cargo test -p clio-core && cargo clippy -p clio-core && cargo fmt
git add crates/clio-core/src/capture.rs
git commit -m "feat(core): accept receipt kind in distillation and prompt for one session receipt"
```

---

### Task 2: `handoff` context preset (core)

A handoff brief answers "I'm picking up ticket X — what do I need to know?". Sections, in budget-priority order: **Directly Relevant** (FTS on the ticket id or topic — this also catches memories tagged `ticket:<id>` because tags are FTS-indexed), **Active Constraints** (rules of the road for anyone touching the project), **Recent Receipts** (what agents recently did). The query is required.

**Files:**
- Modify: `crates/clio-core/src/assembly.rs`
- Test: inline tests mod in the same file

**Interfaces:**
- Consumes: `recall_section` (existing private helper in the same file), `ClioError::Validation` (already imported).
- Produces: `ContextPreset::Handoff` variant parsing from/printing as `"handoff"`; `build_context` returns a brief with sections headed exactly `"Directly Relevant"`, `"Active Constraints"`, `"Recent Receipts"`. Task 3 (MCP/CLI/docs) and Task 5 (Cadence) rely on the preset name `handoff` and on the query being required.

- [ ] **Step 1: Write the failing tests**

Add to the tests module in `crates/clio-core/src/assembly.rs`:

```rust
#[test]
fn handoff_preset_requires_query() {
    let conn = test_db();
    let request = ContextRequest {
        namespace: Some("project:test".into()),
        preset: ContextPreset::Handoff,
        ..Default::default()
    };
    let result = build_context(&conn, &request);
    assert!(result.is_err(), "handoff without a query must error");
}

#[test]
fn handoff_brief_gathers_ticket_memories_constraints_and_receipts() {
    let conn = test_db();

    // Tagged with the ticket id only — the content never mentions "cad-42".
    // This pins the convention that tags are FTS-indexed, so a query for the
    // ticket id finds tag-only memories.
    repository::remember(
        &conn,
        &RememberInput {
            namespace: "project:test".into(),
            kind: "decision".into(),
            title: Some("Index approach".into()),
            summary: None,
            content: "Chose the composite index approach for the orders table.".into(),
            tags: vec!["ticket:cad-42".into()],
            source: None,
            source_ref: None,
            confidence: None,
            importance: 3,
            metadata: serde_json::json!({}),
            valid_from: None,
            valid_until: None,
            upsert: false,
        },
        &crate::settings::Settings::default(),
    )
    .unwrap();

    make_memory(&conn, "project:test", "constraint", "Never edit applied migrations.");
    make_memory(&conn, "project:test", "receipt", "Implemented the index; left the backfill undone.");

    let request = ContextRequest {
        namespace: Some("project:test".into()),
        preset: ContextPreset::Handoff,
        query: Some("CAD-42".into()),
        ..Default::default()
    };
    let brief = build_context(&conn, &request).unwrap();

    let headings: Vec<&str> = brief.sections.iter().map(|s| s.heading.as_str()).collect();
    assert_eq!(
        headings,
        vec!["Directly Relevant", "Active Constraints", "Recent Receipts"]
    );
    assert!(
        brief.sections[0].items.iter().any(|m| m.content.contains("composite index")),
        "tag-only ticket memory must surface in Directly Relevant"
    );
    assert!(brief.sections[1].items.iter().any(|m| m.content.contains("migrations")));
    assert!(brief.sections[2].items.iter().any(|m| m.kind == "receipt"));
}
```

Also add `ContextPreset::Handoff` to the vector in the existing `preset_round_trip` test.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p clio-core handoff`
Expected: COMPILE ERROR — `Handoff` variant does not exist. (A compile failure is the failing state here.)

- [ ] **Step 3: Implement the preset**

Four edits in `crates/clio-core/src/assembly.rs`:

Edit A — add the variant to `ContextPreset`:

```rust
pub enum ContextPreset {
    ProjectBrief,
    PersonBrief,
    DecisionHistory,
    ActiveConstraints,
    RecentActivity,
    Handoff,
    Custom,
}
```

Edit B — `Display`: add `Self::Handoff => "handoff",` alongside the other arms. `FromStr`: add `"handoff" => Ok(Self::Handoff),` and extend the error message list to `…recent-activity, handoff, custom`.

Edit C — add the match arm in `build_context`, before the `Custom` arm:

```rust
        ContextPreset::Handoff => build_handoff(
            conn,
            &ns,
            request.query.as_deref(),
            request.max_items,
            request.include_links,
            &scoring,
        )?,
```

Edit D — add the builder next to `build_custom`:

```rust
/// Handoff brief — everything an agent (or human) needs to pick up a ticket
/// or topic: memories matching the query (tags are FTS-indexed, so memories
/// tagged `ticket:<id>` surface too), the namespace's active constraints, and
/// recent session receipts. The query is required.
fn build_handoff(
    conn: &Connection,
    ns: &Option<String>,
    query: Option<&str>,
    max_items: u32,
    include_links: bool,
    scoring: &Option<ScoringConfig>,
) -> Result<Vec<ContextSection>> {
    let query = match query.map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => q,
        None => {
            return Err(ClioError::Validation(
                "the handoff preset requires a query — a ticket id or topic, e.g. \"CAD-42\""
                    .into(),
            ));
        }
    };

    // Relevance gets the lion's share of the budget; constraints and receipts
    // take what remains.
    let relevant_limit = 12.min(max_items);
    let constraint_limit = 5.min(max_items.saturating_sub(relevant_limit));
    let receipt_limit = max_items
        .saturating_sub(relevant_limit + constraint_limit)
        .min(3);

    let relevant = recall_section(
        conn,
        "Directly Relevant",
        ns,
        None,
        Some(query),
        relevant_limit,
        include_links,
        scoring,
    )?;
    let constraints = recall_section(
        conn,
        "Active Constraints",
        ns,
        Some("constraint"),
        None,
        constraint_limit,
        include_links,
        scoring,
    )?;
    let receipts = recall_section(
        conn,
        "Recent Receipts",
        ns,
        Some("receipt"),
        None,
        receipt_limit,
        include_links,
        scoring,
    )?;

    Ok(vec![relevant, constraints, receipts])
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p clio-core assembly`
Expected: PASS, including `handoff_preset_requires_query`, `handoff_brief_gathers_ticket_memories_constraints_and_receipts`, and the updated `preset_round_trip`. If the tag-only assertion in the second test fails, do not weaken the test — the FTS query path in `repository::recall` is mishandling the hyphenated query and that is the bug to fix (this exact behaviour passed against the live CLI on 2026-07-07).

- [ ] **Step 5: Commit**

```bash
cargo clippy -p clio-core && cargo fmt
git add crates/clio-core/src/assembly.rs
git commit -m "feat(core): add handoff context preset for ticket pickup briefs"
```

---

### Task 3: Surface the preset and conventions (MCP, CLI, docs)

No logic — the MCP and CLI both parse presets via the shared `FromStr`, so `handoff` already works after Task 2. This task updates the strings agents and humans actually read, and records the conventions in the contract docs.

**Files:**
- Modify: `crates/clio-mcp/src/main.rs` (doc comments ~line 350, instructions string ~line 1766)
- Modify: `crates/clio-cli/src/main.rs` (BriefArgs doc comments ~line 492)
- Modify: `docs/reference/mcp-contract.md`
- Modify: `context/DOMAIN_RULES.md`

**Interfaces:**
- Consumes: `ContextPreset::Handoff` from Task 2; `receipt` kind from Task 1.
- Produces: documented conventions (`ticket:<id>` tag, `receipt` kind, `handoff` preset) that Task 5 wires into Cadence.

- [ ] **Step 1: Update the MCP surface strings**

In `crates/clio-mcp/src/main.rs`:

`ContextParams.preset` doc comment — change to:

```rust
    /// Preset: project-brief, person-brief, decision-history, active-constraints, recent-activity, handoff, custom.
```

`ContextParams.query` doc comment — change to:

```rust
    /// FTS query for the custom and handoff presets (handoff requires it — pass the ticket id or topic).
```

In the server instructions string (the `memory_context` line, ~1766), change:

```text
- memory_context: assemble a scoped brief. Presets: project-brief, \
person-brief, decision-history, active-constraints, recent-activity, custom.\n\
```

to:

```text
- memory_context: assemble a scoped brief. Presets: project-brief, \
person-brief, decision-history, active-constraints, recent-activity, handoff, custom. \
The handoff preset requires `query` (a ticket id or topic) and returns a pickup \
brief: relevant memories, active constraints, recent receipts.\n\
```

And append a new line to the instructions after the `memory_inbox` line:

```bash
TICKET CONVENTION: when working a tracked issue, tag stored memories \
`ticket:<issue-id>` (lowercase). Tags are FTS-indexed, so a later handoff \
brief for that id finds them.\n\n\
```

- [ ] **Step 2: Update the CLI doc comments**

In `crates/clio-cli/src/main.rs`, `BriefArgs`:

```rust
    /// Preset: project-brief, person-brief, decision-history, active-constraints, recent-activity, handoff, custom.
    #[arg(long, default_value = "project-brief")]
    preset: String,

    /// FTS query (required by --preset handoff, used with --preset custom).
    #[arg(long)]
    query: Option<String>,
```

- [ ] **Step 3: Build and verify by hand**

```bash
cargo build -p clio-cli -p clio-mcp
./target/debug/clio --db-path /tmp/handoff-check.db remember --content "Chose composite index." --kind decision --tags "ticket:cad-42" --namespace project:test
./target/debug/clio --db-path /tmp/handoff-check.db brief --preset handoff --query CAD-42 --namespace project:test
./target/debug/clio --db-path /tmp/handoff-check.db brief --preset handoff --namespace project:test
rm -f /tmp/handoff-check.db*
```

Expected: the second command prints a brief with the three headings and the decision under "Directly Relevant"; the third prints a validation error naming the query requirement.

- [ ] **Step 4: Update the contract docs**

In `docs/reference/mcp-contract.md`, find the `memory_context` tool entry and (a) add `handoff` to its preset list, (b) add this sentence to its description:

> The `handoff` preset requires `query` (a ticket id or topic) and assembles a pickup brief: Directly Relevant (FTS, which includes `ticket:<id>`-tagged memories), Active Constraints, Recent Receipts.

In `context/DOMAIN_RULES.md`, append this section at the end:

```markdown
## Ticket and receipt conventions

- **Ticket tag** — memories created while working a tracked issue carry the tag
  `ticket:<issue-id>` (lowercase, e.g. `ticket:cad-42`). Tags are FTS-indexed,
  so a handoff brief for "CAD-42" finds tagged memories even when the content
  does not mention the id.
- **Receipt kind** — a `receipt` is a short record of one working session: what
  was done, what was left undone, and why it stopped. Emitted by the session
  stop hooks via `clio distill` (importance 2, tagged `receipt`). Receipts are
  activity, not knowledge: they surface in the handoff preset's "Recent
  Receipts" section and in recent-activity listings.
- **Handoff preset** — `memory_context` / `clio brief` preset `handoff`
  requires a query (ticket id or topic) and assembles: Directly Relevant (FTS),
  Active Constraints, Recent Receipts. Designed to be pasted into a ticket so
  another agent or human can pick the work up with its context attached.
```

- [ ] **Step 5: Full check and commit**

```bash
cargo test && cargo clippy && cargo fmt
git add crates/clio-mcp/src/main.rs crates/clio-cli/src/main.rs docs/reference/mcp-contract.md context/DOMAIN_RULES.md
git commit -m "docs: surface handoff preset and ticket/receipt conventions across MCP, CLI and contracts"
```

---

### Task 4: Codex stop hook (outside this repo)

Codex on this machine fires Claude-style hooks from `~/.codex/hooks.json` (there is already a populated `Stop` array). The Stop stdin JSON carries `session_id` and `cwd` but no transcript path, so the script locates the rollout JSONL under `~/.codex/sessions/YYYY/MM/DD/rollout-*<session_id>.jsonl` (format verified 2026-07-07: `session_meta` payload has `cwd`; `event_msg` payloads of type `user_message`/`agent_message` carry the conversation; `response_item` payloads of type `function_call` carry tool calls). It reuses the existing distillation pipeline from `session_stop.py`.

**Files:**
- Modify: `~/.claude/personal-skills/clio-hooks/scripts/session_stop.py` (one-line signature change)
- Create: `~/.claude/personal-skills/clio-hooks/scripts/codex_stop.py`
- Modify: `~/.codex/hooks.json` (register the hook)

**Interfaces:**
- Consumes: from `session_stop.py`: `MAX_DIGEST_CHARS`, `claim_session(session_id) -> bool`, `build_git_context(cwd, start_head, current_head) -> str`, `consolidate_if_due(cwd)`, and `distill_to_clio` (extended below).
- Produces: memories in Clio with `source = "codex-session"`, `source_ref = <codex session id>` (the core dedup/upsert path already handles per-memory refs and content dedup).

- [ ] **Step 1: Parameterise the distill source in session_stop.py**

In `~/.claude/personal-skills/clio-hooks/scripts/session_stop.py`, change the signature of `distill_to_clio`:

```python
def distill_to_clio(cwd: str, text: str, session_id: str, source: str = "claude-code-session") -> list | None:
```

and inside it change the literal `"claude-code-session"` argument after `"--source"` to `source`. No other changes — the default keeps the Claude Code hook's behaviour identical.

- [ ] **Step 2: Write codex_stop.py**

Create `~/.claude/personal-skills/clio-hooks/scripts/codex_stop.py`:

```python
#!/usr/bin/env python3
"""
Codex Stop hook — distil session knowledge into Clio.

Codex fires Stop via ~/.codex/hooks.json with Claude-style stdin JSON
({"session_id": ..., "cwd": ...}). No transcript path is provided, so this
script locates the rollout JSONL under ~/.codex/sessions/, builds a digest of
user and assistant messages plus tool calls, and reuses the shared
distillation pipeline from session_stop.py (source: "codex-session").

Test mode: `codex_stop.py --print-digest < input.json` prints the digest and
exits without claiming the session or calling clio.
"""

import json
import sys
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from session_stop import (  # noqa: E402
    MAX_DIGEST_CHARS,
    build_git_context,
    claim_session,
    consolidate_if_due,
    distill_to_clio,
)

LOG_FILE = Path.home() / ".claude" / ".clio-hook-log.txt"


def log(message: str) -> None:
    """Append a timestamped message to the shared hook log file."""
    try:
        timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
        with LOG_FILE.open("a") as f:
            f.write(f"[{timestamp}] [codex-stop] {message}\n")
    except Exception:
        pass


def find_rollout(session_id: str) -> Path | None:
    """Locate the rollout JSONL for this session under ~/.codex/sessions/."""
    root = Path.home() / ".codex" / "sessions"
    matches = sorted(root.glob(f"*/*/*/rollout-*{session_id}*.jsonl"))
    return matches[-1] if matches else None


def build_digest(rollout: Path) -> tuple[str, str]:
    """Parse the rollout JSONL into (cwd, digest)."""
    turns: list[str] = []
    cwd = "."
    with rollout.open() as f:
        for line in f:
            try:
                obj = json.loads(line)
            except json.JSONDecodeError:
                continue
            t = obj.get("type")
            p = obj.get("payload") or {}
            if t == "session_meta":
                cwd = p.get("cwd") or cwd
            elif t == "event_msg" and p.get("type") == "user_message":
                msg = (p.get("message") or "").strip()
                if msg:
                    turns.append(f"## User\n{msg[:4000]}")
            elif t == "event_msg" and p.get("type") == "agent_message":
                msg = (p.get("message") or "").strip()
                if msg:
                    turns.append(f"## Assistant\n{msg[:4000]}")
            elif t == "response_item" and p.get("type") == "function_call":
                name = p.get("name") or "tool"
                args = str(p.get("arguments") or "")[:220]
                turns.append(f"[tool: {name} {args}]")
    digest = "\n\n".join(turns).strip()
    if len(digest) > MAX_DIGEST_CHARS:
        digest = "…(earlier turns truncated)…\n\n" + digest[-MAX_DIGEST_CHARS:]
    return cwd, digest


def main() -> None:
    try:
        print_only = "--print-digest" in sys.argv
        hook_input = json.load(sys.stdin)
        session_id = hook_input.get("session_id", "")
        if not session_id:
            log("No session_id in hook input — skipping")
            sys.exit(0)

        rollout = find_rollout(session_id)
        if rollout is None:
            log(f"No rollout file found for {session_id} — skipping")
            sys.exit(0)

        rollout_cwd, digest = build_digest(rollout)
        cwd = hook_input.get("cwd") or rollout_cwd

        if print_only:
            print(digest)
            sys.exit(0)

        if not digest:
            log(f"Empty digest for {session_id} — skipping")
            sys.exit(0)

        if not claim_session(session_id):
            log(f"Session {session_id} already claimed — skipping")
            sys.exit(0)

        # No start-head marker exists for Codex sessions, so the git context
        # carries project/branch/changed-files but no commits-made section.
        payload = build_git_context(cwd, None, "") + "\n\n" + digest
        results = distill_to_clio(cwd, payload, session_id, source="codex-session")
        if results is None:
            log(f"Distillation failed for {session_id} — nothing captured")
        else:
            log(f"Session {session_id}: {len(results)} memory(ies) distilled")
            if results:
                consolidate_if_due(cwd)
        sys.exit(0)

    except Exception as e:
        print(f"Codex clio stop hook error: {e}", file=sys.stderr)
        sys.exit(0)


if __name__ == "__main__":
    main()
```

Then: `chmod +x ~/.claude/personal-skills/clio-hooks/scripts/codex_stop.py`

- [ ] **Step 3: Test digest building against a real rollout**

```bash
echo '{"session_id":"019f397e-0423-76d3-afaa-e3415cfcbb16","cwd":"/tmp"}' \
  | python3 ~/.claude/skills/clio-hooks/scripts/codex_stop.py --print-digest | head -20
```

Expected: digest text beginning with `## User` (that session is a review prompt). If that specific session has been pruned by the time this runs, substitute any session id from a recent file under `~/.codex/sessions/` (the id is the trailing UUID in the rollout filename).

- [ ] **Step 4: Test the full pipeline without storing**

```bash
echo '{"session_id":"019f397e-0423-76d3-afaa-e3415cfcbb16","cwd":"/tmp"}' \
  | python3 ~/.claude/skills/clio-hooks/scripts/codex_stop.py --print-digest \
  | clio distill - --dry-run --source codex-session --source-ref smoke-test --json
```

Expected: a JSON array (possibly empty — a short review session may yield no durable memories; the command succeeding without error is the pass condition). Also confirm the Claude hook still works: `python3 -c "import sys; sys.path.insert(0, '$HOME/.claude/skills/clio-hooks/scripts'); import session_stop; print(session_stop.distill_to_clio.__defaults__)"` — expected output includes `('claude-code-session',)`.

- [ ] **Step 5: Register the hook in ~/.codex/hooks.json**

This file contains supacode-managed entries — append, never rewrite:

```bash
python3 - <<'EOF'
import json, pathlib
p = pathlib.Path.home() / ".codex" / "hooks.json"
data = json.loads(p.read_text())
entry = {"hooks": [{"command": "python3 '/Users/dannyharding/.claude/skills/clio-hooks/scripts/codex_stop.py'", "type": "command", "timeout": 150}]}
stops = data["hooks"].setdefault("Stop", [])
if any("codex_stop.py" in json.dumps(e) for e in stops):
    print("already registered")
else:
    stops.append(entry)
    p.write_text(json.dumps(data, indent=2))
    print("registered")
EOF
```

Expected: `registered`. Verify with `python3 -m json.tool ~/.codex/hooks.json > /dev/null && echo valid`.

- [ ] **Step 6: Commit the hook scripts (in the ~/.claude repo)**

```bash
cd ~/.claude
git add personal-skills/clio-hooks/scripts/codex_stop.py personal-skills/clio-hooks/scripts/session_stop.py
git commit -m "feat(clio-hooks): capture Codex sessions into Clio via Stop hook"
```

(`~/.codex/hooks.json` is not in a repo; nothing to commit there.)

---

### Task 5: Wire the handoff brief into the Cadence loops

Two skill files in the cadence repo gain a handoff-brief pull and the ticket-tag rule. This is prose, not code — the loops execute these instructions.

**Files:**
- Modify: `/Users/dannyharding/Development/Code/Project/cadence/skills/cadence-loop-spec/SKILL.md`
- Modify: `/Users/dannyharding/Development/Code/Project/cadence/skills/cadence-loop-build/SKILL.md`

**Interfaces:**
- Consumes: `clio brief --preset handoff` (Task 2/3) and the `ticket:<id>` tag convention (Task 3).
- Produces: nothing downstream.

- [ ] **Step 1: Add the brief pull to the spec loop**

In `cadence-loop-spec/SKILL.md`, procedure step 3 (**Investigate**), append this paragraph to the step:

```markdown
   Also pull the Clio handoff brief for this ticket:
   `clio brief --preset handoff --query <ISSUE-ID> --char-budget 4000`
   (run from `$PROJECT_DIR` so the namespace auto-detects). Fold anything
   relevant into the spec's Findings; an empty brief is fine — skip it
   silently.
```

And in the **Writing rules** section, add:

```markdown
- When storing anything in Clio while working an issue, tag it
  `ticket:<issue-id>` (lowercase, e.g. `ticket:cad-42`) so future handoff
  briefs find it.
```

- [ ] **Step 2: Add the brief pull to the build loop**

Read `cadence-loop-build/SKILL.md`, find the procedure step where the spec document is read before implementation begins, and append the same handoff-brief paragraph from Step 1 there (adjusted indentation to match the file). Add the same ticket-tag rule to that file's writing-rules (or equivalent) section.

- [ ] **Step 3: Verify and commit (in the cadence repo)**

```bash
cd /Users/dannyharding/Development/Code/Project/cadence
grep -n "handoff" skills/cadence-loop-spec/SKILL.md skills/cadence-loop-build/SKILL.md
git add skills/cadence-loop-spec/SKILL.md skills/cadence-loop-build/SKILL.md
git commit -m "docs(loops): pull Clio handoff brief during spec/build and tag memories ticket:<id>"
```

Expected: grep shows the new paragraphs in both files. Check the cadence repo's branch conventions before committing (commit on its default working branch, never `main`).

---

### Task 6: Ship and record

**Files:** none new (build, verify, remember)

- [ ] **Step 1: Full verification in the clio repo**

```bash
cd /Users/dannyharding/Development/Code/Project/clio
cargo test && cargo clippy && cargo fmt --check
./build.sh
```

Expected: all tests pass; build.sh installs binaries and restarts the daemon.

- [ ] **Step 2: End-to-end smoke test against the live DB**

```bash
clio brief --preset handoff --query clio --char-budget 2000
```

Expected: a brief with the three handoff headings against the real database (contents will vary; "Recent Receipts" will be empty until the first post-change session ends — that is correct).

- [ ] **Step 3: Merge to develop and push**

```bash
git checkout develop && git merge --no-ff feat/handoff-and-receipts && git push
```

(Allowed: develop is not main. Do not touch main.)

- [ ] **Step 4: Record the decision in Clio**

```bash
clio remember --content "Clio stays the context courier for the Cadence/Linear queue, not a second queue. Added: handoff context preset (requires query; sections Directly Relevant / Active Constraints / Recent Receipts), receipt memory kind emitted by session stop hooks, ticket:<id> tag convention (tags are FTS-indexed so handoff queries find them), and a Codex Stop hook (codex_stop.py) reusing the Claude Code distillation pipeline with source codex-session. OpenCode capture deliberately deferred." --kind decision --tags "handoff,receipts,architecture" --source claude-code --source-ref handoff-receipts-2026-07-07 --importance 4 --namespace project:clio
```

---

## Out of scope (deliberate)

- **OpenCode capture** — OpenCode stores transcripts in `opencode.db` (SQLite) with an unverified schema; port it in its own pass once the Codex hook has proven the shared pipeline.
- **Codex session-start brief injection** — capture first; injecting briefs into Codex sessions is a separate follow-up.
- **Project-brief changes** — receipts already reach the existing "Recent Activity" section with no code change; the dedicated section is handoff-only for now.
- **Phase 4 performance items** — still gated behind corpus-size triggers per the 2026-07-02 plan.

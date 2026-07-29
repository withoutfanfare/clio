# CLI Reference

Quick reference for all Clio CLI commands and flags.

For a full walkthrough with explanations, see [Getting Started](getting-started.md). For connecting AI agents, see [MCP Agent Setup](mcp-agent-setup.md).

---

## Setup

```sh
# Install both binaries
cargo install --path crates/clio-cli
cargo install --path crates/clio-mcp

# Initialise the database
clio init

# Initialise with a project namespace
clio init --namespace project:my-project
```

---

## Remote MCP Bridge

Forward MCP traffic to a private Clio server over SSH while detecting project
namespaces on the client computer:

```sh
clio --db-path /remote/memory.db remote-mcp <ssh-alias> \
  --remote-binary /remote/clio-mcp
```

`--db-path` and `--remote-binary` are paths on the remote server. The SSH alias
must support non-interactive key authentication. Explicit namespaces and
global requests are preserved; scoped recall still combines the detected
project namespace with `global` memories.

Persist the bridge once to route normal CLI commands, session hooks, generated
MCP client configurations and the desktop app through the same remote database:

```sh
clio settings use-remote \
  --host <ssh-alias> \
  --remote-db-path /remote/memory.db \
  --mcp-binary /remote/clio-mcp \
  --cli-binary /remote/clio \
  --bridge-command /absolute/local/path/to/clio
```

The daemon remains local and should stay disabled on a shared-memory client.
Configure embeddings and capture on the remote server if you need those
features there. See
[MCP Agent Setup](mcp-agent-setup.md#shared-memory-over-ssh) for Codex and JSON
client configuration.

---

## Storing Memories

```sh
# Basic
clio remember --content "Your content here"

# Full options
clio remember \
  --content "We use WAL mode for SQLite" \
  --title "SQLite WAL decision" \
  --kind decision \
  --tags sqlite,architecture \
  --importance 4

# From stdin
echo "Redis for caching" | clio remember --content -

# Upsert (idempotent write)
clio remember \
  --content "Updated content" \
  --source notes \
  --source-ref unique-key-001 \
  --upsert
```

**Kinds:** `note` `fact` `decision` `snippet` `preference` `process` `knowledgebase` `observation`

**Importance:** 1 (low) to 5 (critical), default 3

---

## Retrieving Memories

```sh
# Full-text search (keyword matching)
clio recall --query "redis caching"

# Semantic search (meaning-based)
clio search --query "database performance optimisation"

# Recent memories (supports all filters)
clio recent
clio recent --kind decision --importance-min 4
clio recent --sort importance-desc --limit 20
clio recent --tags rust,sqlite --match-any

# Show one memory
clio show <id>

# Filters (work with recall, recent, and search)
--namespace project:my-project
--kind decision
--tags sqlite,architecture
--match-any              # match ANY tag instead of ALL
--importance-min 3       # minimum importance (1–5)
--importance-max 5       # maximum importance (1–5)
--sort importance-desc   # sort order (see below)
--include-archived
--limit 20
--offset 40

# JSON output
clio recall --query "redis" --json
```

**Sort orders:** `updated-desc` (default) `updated-asc` `importance-desc` `importance-asc` `created-desc` `created-asc`

### Brief (context assembly)

`brief` assembles a scoped context brief from presets: `project-brief`
(default), `person-brief`, `decision-history`, `active-constraints`,
`recent-activity`, `handoff`, `custom`.

```sh
# Project brief for the current directory's namespace
clio brief

# Handoff brief for picking up a ticket — relevant memories and receipts
# (including any tagged ticket:<id>), plus active constraints
clio brief --preset handoff --query CAD-42 --char-budget 4000

# Custom FTS query
clio brief --preset custom --query "embedding backend"
```

The `handoff` preset requires `--query` (a ticket id or topic). Useful flags:
`--namespace`, `--max-items` (default 20), `--char-budget` (truncates sections
greedily once reached), `--include-links`, `--json`.

---

## Knowledge Graph

```sh
# Link two memories
clio link <from-id> <to-id> --relationship supports

# Get links from a memory
clio show <id>    # links shown in detail view

# Find similar unlinked memories
clio suggest-links --memory-id <id>
clio suggest-links --memory-id <id> --threshold 0.8 --limit 10
```

**Relationships:** `relates_to` `supports` `contradicts` `supersedes` `derived_from` or any custom string

---

## Capture Pipeline

Sends unstructured text to an LLM for automatic classification.

```sh
# Enable capture (one-time setup)
clio settings use-capture --api-key sk-...

# Preview classification without storing
clio capture "We decided to use Redis for caching" --dry-run

# Compare another model without changing the active setting
clio --json capture "We decided to use Redis for caching" \
  --dry-run --model gpt-5.6-luna --metrics

# Capture and store
clio capture "We decided to use Redis for caching"
```

Capture reports either `Stored` with the memory or `Queued` with a review item
when confidence is below the configured threshold.

### Distil (transcript → durable memories)

`distill` sends a long body of text — typically a whole session transcript — to
the LLM and extracts **zero or more** self-contained, durable memories
(decisions, facts, constraints, insights). Routine input yields nothing, so
noise is filtered by design. Uses the same capture pipeline (review routing,
auto-embed) per extracted memory.

```sh
# Preview the durable memories without storing
clio distill - --dry-run < session-digest.txt

# Distil and store, tagging provenance
clio distill - --source claude-code-session --source-ref <session-id> < session-digest.txt
```

Pass `-` to read the text from stdin. Requires the capture pipeline to be enabled.

By default each memory is filed under the **working directory's namespace**
(detected the same way as `clio context`), so a session's memories land in the
right project rather than wherever the model guesses. The model may still
promote a genuinely cross-project fact to `global`. `--namespace` overrides both,
forcing every extracted memory into the given namespace.

### Checkpoint (exact-once session capture)

`checkpoint` distils a session delta like `distill`, but commits it **exactly
once** under the identity `source + session-id + cursor`. Retrying a delivered
key — after a lost response, provider error or outage — replays the stored
result (the same memory and review IDs) instead of creating duplicates. All
extracted memories, review items and the checkpoint record commit in one
transaction; an empty extraction is a successful checkpoint and is never
redistilled.

```sh
clio checkpoint - \
  --source claude-session \
  --session-id <session-id> \
  --cursor <transcript-offset> \
  --branch develop \
  < session-delta-digest.txt
```

Pass `-` to read the digest from stdin. `--namespace` overrides every extracted
memory's namespace; `--branch` and `--ticket` record session context on the
checkpoint. Requires the capture pipeline to be enabled. With `--json` the
result envelope includes `replayed`, `stored_memory_ids` and
`queued_review_ids`.

---

## Resume (pick up where you left off)

`clio resume` builds a deterministic brief of what deserves attention now:
eligible open work first (each with the reason it surfaced — overdue, reminder
due, project-session trigger, dormant), then blocked items, active constraints
(project plus a modest global prior), recent decisions, prompt-relevant
knowledge (only with `--query`) and recent activity. All reads are untracked —
an automatic resume never changes recall ranking. With `--session`, items
surface once per session and are suppressed on repeats until their state
changes.

```sh
clio resume                                  # project-level, auto-detected namespace
clio resume --query "checkpoint retries"     # task-aware, adds relevant knowledge
clio resume --session <session-id>           # once-per-session surfacing
```

---

## Follow-up Attention (open loops)

`clio action` manages the attention lifecycle: follow-ups you committed to,
things you are waiting on, decisions still owed. Statuses are `open`,
`snoozed`, `resolved` and `cancelled`; resolved and cancelled are terminal.
Completing an item never rewrites the underlying memory — it records an event
and, with `--evidence`, a `resolved_by` link to the proof.

```sh
# Open attention with a new task memory, or on an existing memory
clio action add "Verify the deployment after release" --owner user --due 2026-08-01T00:00:00Z
clio action add --memory <id> --trigger project-session

# What needs attention now, and why (overdue, reminder_due, project_session, dormant)
clio action eligible

clio action list --status open
clio action snooze <id> --until 2026-08-15T00:00:00Z
clio action complete <id> --evidence <memory-id> --reason "shipped"
clio action cancel <id> --reason "obsolete"
clio action attach-external <id> --system things --ref <external-id>
clio action history <id>
```

`<id>` accepts either the attention item ID or the memory ID.

---

## Archiving & Deletion

```sh
clio archive <id>       # soft-archive (hidden, restorable)
clio unarchive <id>     # restore
clio delete <id>        # permanent delete of a single memory
```

---

## Namespace Management

```sh
# Check detected namespace
clio context

# List all namespaces
clio namespaces
```

Detection order: search all ancestors for the nearest `.clio-namespace` first,
then use the nearest `.git` marker, then the nearest `Cargo.toml` or
`package.json`, then `global`. An ancestor `.clio-namespace` therefore overrides
a nested package manifest.

### Cleanup (stale namespaces)

`cleanup` finds namespaces that are no longer useful and can purge them. It is
**dry-run by default** — pass `--execute` to actually delete, and a database
backup is always taken first.

```sh
# Dry run — show stale candidates and why they were flagged (all criteria)
clio cleanup

# Restrict to specific criteria
clio cleanup --stale-months 6      # no activity for 6 months
clio cleanup --archived            # every memory already archived
clio cleanup --folder-gone         # project:<slug> with no folder on disk

# Actually purge (backup taken first)
clio cleanup --folder-gone --execute
```

Criteria:
- **stale-months** — last activity older than N months (default from settings).
- **archived** — the namespace has no live memories (all archived).
- **folder-gone** — a `project:<slug>` namespace whose folder is not found under
  any configured dev root (a heuristic — see `cleanup.dev_roots` in settings).

With no criterion flag, all three are applied. The `global` namespace is never
flagged. See `reference/settings.md` for `cleanup.*` configuration.

### Consolidate (project memory)

`consolidate` rolls a namespace's atomic memories into a single AI-curated
"consolidated memory" document — a coherent, deduplicated summary of the
project. It is stored as a singleton memory per namespace (`kind = summary`,
upserted in place), and the session-start brief leads with it.

```sh
# Consolidate the current project (namespace auto-detected from cwd)
clio consolidate

# Consolidate a specific namespace
clio consolidate --namespace project:clio
```

The document is a **derived cache**: each run reconciles it from the current
atomic memories (no iterative self-editing, so it can't drift), and the atomic
memories are left untouched. Requires the capture pipeline to be enabled.

Triggers:

```sh
# Every namespace
clio consolidate --all

# Only namespaces with enough new memories since last run (the configured
# consolidate.auto_threshold) — cheap to run often, no-op when nothing's due
clio consolidate --if-due
clio consolidate --all --if-due
```

The Stop hook (see `clio-hooks`) runs `clio consolidate --if-due` after each
session that produced memories, so the consolidated document refreshes
automatically once a project accrues enough new material.

**Scheduling (macOS launchd):** to also refresh on a timer, drop a LaunchAgent
at `~/Library/LaunchAgents/com.clio.consolidate.plist` and
`launchctl load` it:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>com.clio.consolidate</string>
    <key>ProgramArguments</key>
    <array>
        <string>/Users/YOU/.cargo/bin/clio</string>
        <string>consolidate</string>
        <string>--all</string>
        <string>--if-due</string>
    </array>
    <key>StartCalendarInterval</key>
    <dict><key>Hour</key><integer>6</integer><key>Minute</key><integer>0</integer></dict>
</dict>
</plist>
```

---

## Stats & Activity

```sh
clio stats
clio stats --namespace project:clio
clio activity
clio activity --namespace project:clio --limit 20
```

---

## Import & Export

```sh
# Export
clio export --output memories.jsonl
clio export --output memories.jsonl --namespace project:clio

# Import
clio import --input memories.jsonl

# Migrate from AI assistants
clio migrate --source claude --file conversations.json
clio migrate --source chatgpt --file export.json
clio migrate --source claude --file conversations.json --classify --dry-run
```

---

## Embeddings

```sh
# Show embedding coverage
clio embed status

# Backfill missing embeddings and replace vectors from an old model
clio embed backfill

# Process more than the default 100 memories
clio embed backfill --batch-size 1000
```

---

## Settings

```sh
# View current settings
clio settings show

# Embedding providers
clio settings use-local                    # default, no API key needed
clio settings use-openai --api-key sk-...  # higher quality, needs key

# Capture pipeline
clio settings use-capture --api-key sk-... --model gpt-4o-mini
clio settings show-capture
clio settings set-capture-model gpt-5.6-luna

# Route shared operations through an SSH host
clio settings use-remote \
  --host atlas \
  --remote-db-path /home/ubuntu/.local/share/clio/memory.db \
  --mcp-binary /home/ubuntu/.local/bin/clio-mcp \
  --cli-binary /home/ubuntu/.local/bin/clio \
  --bridge-command /absolute/local/path/to/clio

# Return to local-only storage
clio settings disable-remote
```

After changing the embedding provider or model, restart MCP clients and run
`clio embed backfill` until all stale vectors have been replaced.

`settings show-capture` and `settings set-capture-model` follow a configured
shared route, so they read or update Atlas when run from a connected Mac.
`settings show` remains local because it also shows that Mac's route.

---

## Global Flags

| Flag | Description |
|---|---|
| `--db-path <path>` | Override database location |
| `--json` | JSON output |
| `--local` | Bypass a configured shared route for deliberate local maintenance |

Default DB: `~/Library/Application Support/clio/memory.db` (macOS)

---

## Related Documentation

- [Getting Started](getting-started.md) — full walkthrough with explanations
- [MCP Agent Setup](mcp-agent-setup.md) — connecting AI agents to Clio
- [Settings Reference](reference/settings.md) — all configuration keys and defaults
- [Schema Reference](reference/schema.md) — database structure
- [Documentation Index](README.md) — all available documentation
